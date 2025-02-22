use crate::{
    components::{
        cache_hierarchy::{
            common::{
                CacheHierarchyAccessResult, PrivateCacheEvictedSlot, PrivateCachePokeResult,
                PrivateCaches, SharedCache, SharedCacheLookupResult,
            },
            mmu::{AbstractMMU, MMUFlushMode, MMUTranslationResult},
            CacheBlockRequest, MemoryAccessRequest, MemoryHierarchy,
        },
        debug::{
            cache_line_history::{CacheLineCoherenceHistory, CacheOperationType},
            statistics::{EventType, Statistics},
        },
    },
    parameter,
};

use super::ParallelMemoryHierarchy;

impl<
        MMU: AbstractMMU,
        PCache: PrivateCaches,
        SCache: SharedCache,
        const PRECISE_COHERENCE_RECONSTRUCTION: bool,
        const FILL_SCACHE_ON_FILLING_PCACHE: bool,
        const FILL_SCACLE_ON_PCACHE_EVICTION: bool,
        const FILL_SCACHE_ON_PCACHE_WRITEBACK: bool,
        const DIRECTORY_SHARD_COUNT: usize,
        const CORE_COUNT: usize,
    > MemoryHierarchy
    for ParallelMemoryHierarchy<
        MMU,
        PCache,
        SCache,
        PRECISE_COHERENCE_RECONSTRUCTION,
        FILL_SCACHE_ON_FILLING_PCACHE,
        FILL_SCACLE_ON_PCACHE_EVICTION,
        FILL_SCACHE_ON_PCACHE_WRITEBACK,
        DIRECTORY_SHARD_COUNT,
        CORE_COUNT,
    >
{
    fn access_memory_pblock_id(
        &self,
        r: &CacheBlockRequest,
        ts: u64,
    ) -> CacheHierarchyAccessResult {
        let is_os = r.is_os();
        let core_id = r.core_id;
        let is_instruction = r.is_instruction();
        let is_store = r.is_store();
        let is_page_walk = r.is_page_walk();
        let block_id = r.block_id;
        let is_prefetch = r.is_prefetch();

        if !is_prefetch && self.with_statistics {
            Statistics::global_record(core_id, EventType::MemoryAccess, is_os);
            if is_instruction {
                Statistics::global_record(core_id, EventType::InstructionAccess, is_os);
            } else {
                Statistics::global_record(core_id, EventType::DataAccess, is_os);
            }
        }

        // first, we need to check the private cache.
        let private_hit = self.private_caches.poke_and_update(r, ts);

        if private_hit == PrivateCachePokeResult::Hit {
            // we don't have to anything. Just return.
            return CacheHierarchyAccessResult::HitInSelfPrivateCache;
        }

        let evicted_slot = match private_hit {
            PrivateCachePokeResult::Hit => unreachable!(),
            PrivateCachePokeResult::Miss(ref slot) => slot.clone(),
            PrivateCachePokeResult::PermissionViolation(ref slot) => slot.clone(),
        };

        // Alright, we may need to get another directory entry of the eviction.
        // This entry may bot be used, because other entry in the same set can be evicted. But we need to get it ahead of time to avoid deadlock.
        let (mut miss_directory_set_guard, evict_directory) = {
            match evicted_slot {
                PrivateCacheEvictedSlot::Valid(_, potential_evicted_id) => {
                    let (m_guard, e_guard) = self
                        .directory
                        .fetch_two_entries(block_id, potential_evicted_id);
                    (m_guard, Some((potential_evicted_id, e_guard)))
                }
                _ => {
                    let directory_set_guard = self.directory.fetch_one_entry(block_id);
                    (directory_set_guard, None)
                }
            }
        };

        let miss_directory_guard = miss_directory_set_guard.get_or_create(block_id);

        let sharers = miss_directory_guard.sharers;

        // Now, we communicate with the directory. Current miss is mapped to the following position in the directory entry share list.
        let p_cache_id = PCache::get_cache_id_by_cache_info(core_id, is_instruction);

        let record_op = if is_store {
            CacheOperationType::GetM
        } else {
            CacheOperationType::GetR
        };

        if PRECISE_COHERENCE_RECONSTRUCTION && miss_directory_guard.recent_writer_ts > ts {
            // This means that the current operation is not ordered. (even later than the first writer)
            // There is no need to continue, because this memory operation is whatever blocked by a writer before the eviction.
            CacheLineCoherenceHistory::global_record_history(
                block_id,
                record_op,
                p_cache_id,
                ts,
                false,
                sharers,
                line!(),
            );

            // Well, this is not very accurate. The truth is that we don't know whether this is a miss or hit,
            // because the history has been cleaned up by an earlier write
            if !is_prefetch && self.with_statistics {
                Statistics::global_record(core_id, EventType::UnknownPrivateCacheMisses, is_os);
                // accordingly, we don't know whether this access would have cause a shared cache miss.
                Statistics::global_record(core_id, EventType::UnknownSharedCacheMisses, is_os);
            }

            return CacheHierarchyAccessResult::Unknown;
        }

        // if it is miss, we need to access the last level cache as well, and add it.
        if sharers.count_ones() == 0 {
            let shared_cache_result = if FILL_SCACHE_ON_FILLING_PCACHE && !is_store {
                // here we take the ownership of the cache line from the shared cache to the private cache.
                // So abandon_dirty is true.
                // We also don't need to write through to the LLC, so the is_store is false.
                let lookup_result = self.shared_cache.lookup_and_insert_on_miss(r, ts, true);

                match lookup_result {
                    SharedCacheLookupResult::Hit => Some(true),
                    SharedCacheLookupResult::Miss => Some(false),
                    SharedCacheLookupResult::ColdMiss => {
                        if self.with_statistics {
                            Statistics::global_record(
                                core_id,
                                EventType::SharedCacheColdMiss,
                                is_os,
                            );
                        }
                        Some(false)
                    }
                    SharedCacheLookupResult::Unknown(_) => {
                        if self.with_statistics {
                            Statistics::global_record(
                                core_id,
                                EventType::UnknownSharedCacheMisses,
                                is_os,
                            );
                        }
                        None
                    }
                }
            } else {
                let lookup_result = self.shared_cache.lookup(r, ts);

                match lookup_result {
                    SharedCacheLookupResult::Hit => Some(true),
                    SharedCacheLookupResult::Miss => Some(false),
                    SharedCacheLookupResult::ColdMiss => {
                        if self.with_statistics {
                            Statistics::global_record(
                                core_id,
                                EventType::SharedCacheColdMiss,
                                is_os,
                            );
                        }
                        Some(false)
                    }
                    SharedCacheLookupResult::Unknown(_) => {
                        if self.with_statistics {
                            Statistics::global_record(
                                core_id,
                                EventType::UnknownSharedCacheMisses,
                                is_os,
                            );
                        }
                        None
                    }
                }
            };

            miss_directory_guard.update_lru_ts(ts);
            miss_directory_guard.sharers.set(p_cache_id, true);
            miss_directory_guard.insertion_ts = ts; // this is the moment when the block is inserted to the directory.

            if is_store && PRECISE_COHERENCE_RECONSTRUCTION {
                miss_directory_guard.recent_writer_ts = ts;
            }

            let modified = is_store;

            let writable = if !parameter::ENABLE_EXCLUSIVE_CACHE_STATE {
                modified
            } else {
                !is_instruction && !is_page_walk
            };

            // directory keep the writable information.
            miss_directory_guard.shared = !writable;

            CacheLineCoherenceHistory::global_record_history(
                block_id,
                record_op,
                p_cache_id,
                ts,
                true,
                miss_directory_guard.sharers,
                line!(),
            );

            // Now, it is time to fill the private cache.

            let mut set_to_fill = self.private_caches.get_set_for_fill(r);

            // handle eviction now.
            if let Some(evicted_line_is_modified) = set_to_fill.fill_with_potential_eviction_slot(
                evicted_slot,
                block_id,
                ts,
                is_instruction,
                writable,
                modified,
            ) {
                let (evicted_block_id, mut evicted_block_directory_guard) =
                    evict_directory.unwrap();
                self.handle_eviction(
                    evicted_block_directory_guard
                        .as_mut()
                        .unwrap_or(&mut miss_directory_set_guard),
                    p_cache_id,
                    evicted_block_id,
                    ts,
                    evicted_line_is_modified,
                    is_os,
                );
            }

            if !is_prefetch && self.with_statistics {
                // Here it is a miss in the private cache.
                Statistics::global_record(core_id, EventType::PrivateCacheMiss, is_os);

                if is_instruction {
                    Statistics::global_record(core_id, EventType::PrivateICacheMiss, is_os);
                } else if is_page_walk {
                    Statistics::global_record(core_id, EventType::PrivateCacheMissDueToPTW, is_os);
                } else {
                    Statistics::global_record(core_id, EventType::PrivateDCacheMiss, is_os);
                }

                Statistics::global_record(core_id, EventType::SharedCacheAccess, is_os);
            }

            return match shared_cache_result {
                Some(true) => CacheHierarchyAccessResult::HitInSharedCache,
                Some(false) => {
                    if !is_prefetch && self.with_statistics {
                        if is_page_walk {
                            Statistics::global_record(
                                core_id,
                                EventType::SharedCacheMissDueToPTW,
                                is_os,
                            );
                        } else if is_instruction {
                            Statistics::global_record(
                                core_id,
                                EventType::SharedCacheMissDueToInstructionFetch,
                                is_os,
                            );
                        } else if is_store {
                            Statistics::global_record(
                                core_id,
                                EventType::SharedCacheMissDueToDataWrite,
                                is_os,
                            );
                        } else {
                            Statistics::global_record(
                                core_id,
                                EventType::SharedCacheMissDueToDataRead,
                                is_os,
                            );
                        }
    
                        Statistics::global_record(core_id, EventType::SharedCacheMiss, is_os);    
                    }
                    
                    CacheHierarchyAccessResult::Miss
                },
                None => CacheHierarchyAccessResult::Unknown,
            };
        }

        if is_prefetch {
            return CacheHierarchyAccessResult::Miss;
        }

        if self.with_statistics {
            if is_instruction {
                Statistics::global_record(
                    core_id,
                    EventType::PrivateCacheMissTriggerCoherenceDueToFetch,
                    is_os,
                );
            } else if is_store {
                Statistics::global_record(
                    core_id,
                    EventType::PrivateCacheMissTriggerCoherenceDueToWrite,
                    is_os,
                );
            } else {
                Statistics::global_record(
                    core_id,
                    EventType::PrivateCacheMissTriggerCoherenceDueToRead,
                    is_os,
                );
            }
        }

        // If the directory entry suggests the cache line be in the shared state,
        // and the current operation is a read operation, we don't need to acquire the sharer list.
        // It is a fast path: we can just put this replica in the shared list.
        let (evicted, res) = if miss_directory_guard.shared && !is_store {
            // add myself to the sharer list.
            miss_directory_guard.update_lru_ts(ts);
            miss_directory_guard.sharers.set(p_cache_id, true);

            // get the lock of the private cache for refilling.
            let mut set_for_refill_lock = self.private_caches.get_set_for_fill(r);

            // refill and evict.
            let evicted = set_for_refill_lock.fill_with_potential_eviction_slot(
                evicted_slot,
                block_id,
                ts,
                is_instruction,
                false,
                false,
            );

            CacheLineCoherenceHistory::global_record_history(
                block_id,
                record_op,
                p_cache_id,
                ts,
                true,
                miss_directory_guard.sharers,
                line!(),
            );

            if ts < miss_directory_guard.insertion_ts {
                // This access is earlier than the directory creation.
                // Its result should be unknowl
                if self.with_statistics {
                    Statistics::global_record(core_id, EventType::UnknownPrivateCacheMisses, is_os);
                    Statistics::global_record(core_id, EventType::UnknownSharedCacheMisses, is_os);
                }

                // Mark the current access as the insertion file of the directory.
                miss_directory_guard.insertion_ts = ts;

                (evicted, CacheHierarchyAccessResult::Unknown)
            } else {
                (evicted, CacheHierarchyAccessResult::HitInOtherPrivateCache)
            }
        } else {
            let mut acquire_list = sharers;
            // this list should either
            // - Not contain the current core, so it is a miss, or
            // - Contain the current core, because the permission is wrong.
            assert!(
                acquire_list.get(p_cache_id).unwrap() == false
                    || private_hit.permission_violation()
            );

            // the core itself should be also part of the acquire_list.
            acquire_list.set(p_cache_id, true);

            // Now, acquire the lock of all sets of the private cache, suggested by the directory.
            let mut acquired_sets = self
                .private_caches
                .get_set_guard_by_sharer_list(block_id, acquire_list);

            if PRECISE_COHERENCE_RECONSTRUCTION {
                // Here for MESI, there are two cases that we need to consider:
                // - Another core has written the cache line with a larger timestamp.
                //   In this case, we need to only keep the latest writer, and ignore this operation.

                // - Another core has the write permission but it is not written yet.
                //   In this case, the logic is different from the reader and the writer.
                //     For the reader, it just needs to reclaim the write permission.
                //     For the writer, it just needs to invalidate this cache line.

                // The following code handles the first case.

                // Do we have another sharer that has a write permission with a larger timestamp?
                let mut other_has_written_with_large_ts = false;
                let mut other_write_ts = 0;
                for (replica_cache_id, set, index) in acquired_sets.iter() {
                    if let Some(index) = index {
                        let line = &set.lines[*index];
                        assert_eq!(line.block_id(), block_id);
                        if line.write_ts() > ts {
                            other_has_written_with_large_ts = true;
                        }

                        // also, find the largest timestamp of the write operation.
                        if line.write_ts() > other_write_ts {
                            other_write_ts = line.write_ts();
                        }
                    } else {
                        // Well, the only case that we can see a miss in the private cache is that the cache is waiting for refilling.
                        if *replica_cache_id != p_cache_id {
                            if parameter::ENABLE_CACHE_LINE_HISTORY {
                                CacheLineCoherenceHistory::global_get_block_history(block_id)
                                    .unwrap()
                                    .value()
                                    .print_history();
                            }
                            assert_eq!(*replica_cache_id, p_cache_id);
                        }
                    }
                }

                if other_has_written_with_large_ts {
                    // a write operation has been done by another core with a larger timestamp.
                    // This write operation is not propagated to the directory, so we cannot see it until we scan it.
                    assert!(miss_directory_guard.recent_writer_ts <= other_write_ts);
                }

                // Well, if you find another core that has written the cache line with a larger timestamp,
                // update the directory's timestamp immediately.
                if other_write_ts > miss_directory_guard.recent_writer_ts {
                    miss_directory_guard.recent_writer_ts = other_write_ts;
                    miss_directory_guard.update_lru_ts(other_write_ts);
                }

                if other_has_written_with_large_ts {
                    // Well, this cache line is already touched by another core with a later timestamp.
                    // Only that core should be kept.

                    CacheLineCoherenceHistory::global_record_history(
                        block_id,
                        record_op,
                        p_cache_id,
                        ts,
                        false,
                        miss_directory_guard.sharers,
                        line!(),
                    );

                    // Now, release the lock of the private cache.
                    drop(acquired_sets);

                    if self.with_statistics {
                        // The truth is that we don't know whether this is a miss or hit, because a previous write operation has cleaned the history.
                        Statistics::global_record(
                            core_id,
                            EventType::UnknownPrivateCacheMisses,
                            is_os,
                        );
                        // Accordingly, we don't know whether this access would have cause a shared cache miss.
                        Statistics::global_record(
                            core_id,
                            EventType::UnknownSharedCacheMisses,
                            is_os,
                        );
                    }

                    return CacheHierarchyAccessResult::Unknown;
                }
            }

            let mut res = if !private_hit.permission_violation() {
                CacheHierarchyAccessResult::HitInOtherPrivateCache
            } else {
                CacheHierarchyAccessResult::MissDueToPermission
            };

            let evicted = if is_store {
                let mut incoming_sharer = miss_directory_guard.sharers;
                let mut set_for_refill_lock = None;

                for (replica_cache_id, set, index) in acquired_sets.iter_mut() {
                    if let Some(index) = index {
                        let entry = &set.lines[*index];
                        assert_eq!(entry.block_id(), block_id);
                        if !PRECISE_COHERENCE_RECONSTRUCTION || entry.access_ts() <= ts {
                            // invalid the directory entry.
                            incoming_sharer.set(*replica_cache_id, false);
                            // invalid the private cache entry.
                            set.invalidate(*index);

                            if self.with_statistics {
                                Statistics::global_record(
                                    core_id,
                                    EventType::PrivateCacheInvalidation,
                                    is_os,
                                );
                            }

                            CacheLineCoherenceHistory::global_record_history(
                                block_id,
                                CacheOperationType::Invalidate(p_cache_id),
                                *replica_cache_id,
                                ts,
                                false,
                                incoming_sharer,
                                line!(),
                            )
                        } else {
                            // This means you will only get the read permission, because there is a core with read permission and large timestamp.
                            assert!(entry.write_ts() <= ts);
                            if *replica_cache_id == p_cache_id {
                                CacheLineCoherenceHistory::global_get_block_history(block_id)
                                    .unwrap()
                                    .value()
                                    .print_history();

                                assert!(*replica_cache_id != p_cache_id);
                            }

                            // remove the write permission.
                            set.request_sharer(*index, ts);
                        }
                    } else {
                        // This is the only case that we can see a the private cache does not have this block.
                        assert!(*replica_cache_id == p_cache_id);
                        incoming_sharer.set(*replica_cache_id, false);
                    }

                    if *replica_cache_id == p_cache_id {
                        set_for_refill_lock = Some(set);
                    }
                }

                let set_for_refill_lock = set_for_refill_lock.unwrap();

                let evicted = if incoming_sharer.count_ones() == 0 {
                    // The writable permission is allocated.
                    miss_directory_guard.shared = false;

                    // This means there is no sharer. The core will get modified permission.
                    set_for_refill_lock.fill_with_potential_eviction_slot(
                        evicted_slot,
                        block_id,
                        ts,
                        is_instruction,
                        true,
                        true,
                    )
                } else {
                    // No write permission to this cache line by any core.
                    miss_directory_guard.shared = true;

                    // There are sharers. So unfortunately, you can only get shared permission.
                    set_for_refill_lock.fill_with_potential_eviction_slot(
                        evicted_slot,
                        block_id,
                        ts,
                        is_instruction,
                        false,
                        false,
                    )
                };

                if !private_hit.permission_violation() {
                    if miss_directory_guard.insertion_ts < ts {
                        res = CacheHierarchyAccessResult::HitInOtherPrivateCache;
                    } else {
                        // This access turns out to be a earlier request
                        res = CacheHierarchyAccessResult::MissInPrivateCache;
                        // We extend the life time of this directory by considering this access.
                        miss_directory_guard.insertion_ts = ts;

                        // This memory access is supposed to access the shared cache, but now it is served by other private cache.
                        // Even though we make it access the shared cache now, we don't really know whether it was a hit or a miss, because state of the shared cache is different.
                        // This might have triggered a shared cache miss.
                        if self.with_statistics {
                            Statistics::global_record(
                                core_id,
                                EventType::UnknownSharedCacheMisses,
                                is_os,
                            );
                        }
                    }
                } else if PRECISE_COHERENCE_RECONSTRUCTION {
                    // This memory access is definitely not the first one to this cache line.
                    assert!(miss_directory_guard.insertion_ts <= ts);
                }

                // add self to the incoming sharer list.
                incoming_sharer.set(p_cache_id, true);

                // update the directory.
                miss_directory_guard.update_lru_ts(ts);
                miss_directory_guard.sharers = incoming_sharer;

                // We have a new write exposed to the directory.
                if PRECISE_COHERENCE_RECONSTRUCTION {
                    assert!(miss_directory_guard.recent_writer_ts <= ts);
                    miss_directory_guard.recent_writer_ts = ts;
                }

                CacheLineCoherenceHistory::global_record_history(
                    block_id,
                    record_op,
                    p_cache_id,
                    ts,
                    true,
                    miss_directory_guard.sharers,
                    line!(),
                );

                drop(acquired_sets);

                evicted
            } else {
                // You need to find currently whether there are cores that have modified permission.
                let mut find_writable_replica = false;
                let mut set_for_refill_lock = None;

                for (replica_cache_id, set, index) in acquired_sets.iter_mut() {
                    if let Some(index) = index {
                        let entry = &set.lines[*index];
                        assert_eq!(entry.block_id(), block_id);
                        if entry.has_write_permission() {
                            // well, if you have write permission, you have to yield the write permission.
                            assert!(!find_writable_replica);

                            if entry.is_modified()
                                && entry.write_ts() > ts
                                && PRECISE_COHERENCE_RECONSTRUCTION
                            {
                                // OK, this read operation is also not ordered.
                                // There is nothing we need to do.

                                CacheLineCoherenceHistory::global_record_history(
                                    block_id,
                                    record_op,
                                    p_cache_id,
                                    ts,
                                    false,
                                    miss_directory_guard.sharers,
                                    line!(),
                                );

                                drop(acquired_sets);

                                if self.with_statistics {
                                    // This read happens after a early arrival write operation, so
                                    // we don't know the state of this cache line for this specific case.
                                    Statistics::global_record(
                                        core_id,
                                        EventType::UnknownPrivateCacheMisses,
                                        is_os,
                                    );
                                    // Accordingly, we don't know whether this access would have cause a shared cache miss.
                                    Statistics::global_record(
                                        core_id,
                                        EventType::UnknownSharedCacheMisses,
                                        is_os,
                                    );
                                }

                                return CacheHierarchyAccessResult::Unknown;
                            }

                            // Alright, we find the modifier of this cache line.
                            set.request_sharer(*index, ts);
                            find_writable_replica = true;
                        }
                    }

                    if *replica_cache_id == p_cache_id {
                        set_for_refill_lock = Some(set);
                    }
                }

                if find_writable_replica {
                    assert!(!miss_directory_guard.shared);
                }

                // Then, we need to add self to the directory.
                miss_directory_guard.update_lru_ts(ts);
                miss_directory_guard.sharers.set(p_cache_id, true);
                miss_directory_guard.shared = true;

                if !private_hit.permission_violation() {
                    if miss_directory_guard.insertion_ts < ts {
                        res = CacheHierarchyAccessResult::HitInOtherPrivateCache;
                    } else {
                        // This access turns out to be earlier than the directory creation.
                        res = CacheHierarchyAccessResult::MissInPrivateCache;
                        // We decide to create a replica for this cache line, so we expand this directory life time.
                        miss_directory_guard.insertion_ts = ts;

                        if self.with_statistics {
                            // This memory access is supposed to access the shared cache, but now it is served by other private cache.
                            // Even though we make it access the shared cache now, we don't really know whether it was a hit or a miss, because state of the shared cache is different.
                            // This might have triggered a shared cache miss.
                            Statistics::global_record(
                                core_id,
                                EventType::UnknownSharedCacheMisses,
                                is_os,
                            );
                        }
                    }
                } else {
                    // read should never see a permission violation.
                    panic!("Permission violation should not be seen by a read operation.");
                }

                // We can insert the block to the private cache now.
                let set_for_refill_lock = set_for_refill_lock.unwrap();

                let evicted = set_for_refill_lock.fill_with_potential_eviction_slot(
                    evicted_slot,
                    block_id,
                    ts,
                    is_instruction,
                    false,
                    false,
                );

                CacheLineCoherenceHistory::global_record_history(
                    block_id,
                    record_op,
                    p_cache_id,
                    ts,
                    true,
                    miss_directory_guard.sharers,
                    line!(),
                );

                drop(acquired_sets);

                evicted
            };

            (evicted, res)
        };

        if let Some(is_modified) = evicted {
            let (evicted_block_id, mut evicted_block_directory_guard) = evict_directory.unwrap();
            self.handle_eviction(
                evicted_block_directory_guard
                    .as_mut()
                    .unwrap_or(&mut miss_directory_set_guard),
                p_cache_id,
                evicted_block_id,
                ts,
                is_modified,
                is_os,
            );
        }

        if self.with_statistics {
            Statistics::global_record(core_id, EventType::PrivateCacheMiss, is_os);

            if is_instruction {
                Statistics::global_record(core_id, EventType::PrivateICacheMiss, is_os);
            } else if is_page_walk {
                Statistics::global_record(core_id, EventType::PrivateCacheMissDueToPTW, is_os);
            } else {
                Statistics::global_record(core_id, EventType::PrivateDCacheMiss, is_os);
            }

            if matches!(res, CacheHierarchyAccessResult::HitInOtherPrivateCache) && is_store {
                Statistics::global_record(
                    core_id,
                    EventType::PrivateCacheMissTriggerInvalidation,
                    is_os,
                );
            }
        }

        res
    }

    fn translate(&self, r: &MemoryAccessRequest, ts: u64) -> MMUTranslationResult {
        unsafe {
            self.mmus[r.core_id as usize]
                .get()
                .as_mut()
                .unwrap()
                .translate_and_refill(r.core_id, r.va, ts, r.is_instruction())
        }
    }

    fn flush_mmu(&self, core_id: u32, info: MMUFlushMode) {
        unsafe {
            self.mmus[core_id as usize]
                .get()
                .as_mut()
                .unwrap()
                .flush(info);
        }
    }

    fn serialize(&self, name: &str, numa_node_id: usize) {
        println!("Serializing private caches.");
        self.private_caches.serialize(name, numa_node_id);
        println!("Serializing directory.");
        self.directory.serialize(name, numa_node_id);
        println!("Serializing shared cache.");
        self.shared_cache.serialize(name, numa_node_id);
        println!("Serialize MMUs");
        self.serialize_mmus(name, numa_node_id);
    }

    fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        println!("Deserializing private caches.");
        self.private_caches.deserialize(name, numa_node_id);
        println!("Deserializing directory.");
        self.directory.deserialize(name, numa_node_id);
        println!("Deserializing shared cache.");
        self.shared_cache.deserialize(name, numa_node_id);
        println!("Deserialize MMUs");
        self.deserialize_mmus(name, numa_node_id);
    }
}
