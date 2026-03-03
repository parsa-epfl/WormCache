use std::panic;

use crate::{
    components::{
        cache_hierarchy::{
            common::{
                CacheAccessType, CacheHierarchyAccessResult, DirectorySet, PrivateCacheEvictedSlot,
                PrivateCachePokeResult, PrivateCaches, SharedCache, SharedCacheAccessRequest,
                SharedCacheAccessSource, SharedCacheLookupResult, SharerList,
            }, hierarchy::timing_bridge::{timing_bridge_push, ICT}, mmu::{AbstractMMU, MMUFlushMode, MMUTranslationResult}, CacheBlockRequest, MemoryAccessRequest, MemoryHierarchy
        },
        debug::{
            cache_line_history::{CacheLineCoherenceHistory, CacheOperationType},
            statistics::{EventType, Statistics},
        },
    },
    parameter::{CACHE_LINE_SIZE, ENABLE_EXCLUSIVE_CACHE_STATE},
};

use super::ParallelMemoryHierarchy;

impl<
    MMU: AbstractMMU,
    PCache: PrivateCaches,
    SCache: SharedCache,
    const FILL_SCACHE_ON_FILLING_PCACHE: bool,
    const FILL_SCACLE_ON_PCACHE_EVICTION: bool,
    const FILL_SCACHE_ON_PCACHE_WRITEBACK: bool,
    const FILL_SCACLE_ON_PCACPE_REPLICA_CREATION: bool,
    const DIRECTORY_SHARD_COUNT: usize,
    const DIRECTORY_ASSO: usize,
    const CORE_COUNT: usize,
> MemoryHierarchy
    for ParallelMemoryHierarchy<
        MMU,
        PCache,
        SCache,
        FILL_SCACHE_ON_FILLING_PCACHE,
        FILL_SCACLE_ON_PCACHE_EVICTION,
        FILL_SCACHE_ON_PCACHE_WRITEBACK,
        FILL_SCACLE_ON_PCACPE_REPLICA_CREATION,
        DIRECTORY_SHARD_COUNT,
        DIRECTORY_ASSO,
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
        let paddr = block_id << CACHE_LINE_SIZE.trailing_zeros();

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

        let (miss_directory_guard, evicted) = miss_directory_set_guard.get_or_create(block_id);

        let sharers = miss_directory_guard.sharers;

        // Now, we communicate with the directory. Current miss is mapped to the following position in the directory entry share list.
        let p_cache_id = PCache::get_cache_id_by_cache_info(core_id, is_instruction);

        let record_op = if is_store {
            CacheOperationType::GetM
        } else {
            CacheOperationType::GetR
        };

        // if it is miss, we need to access the last level cache as well, and add it.
        if sharers.count_ones() == 0 {
            // directory miss
            if let Some(evicted_directory_entry) = evicted {
                // trigger eviction to the private cache.
                let mut acquired_sets = self.private_caches.get_set_guard_by_sharer_list(
                    evicted_directory_entry.0,
                    evicted_directory_entry.1.sharers,
                );

                // invalidte these blocks.
                for (replica_cache_id, set, index) in acquired_sets.iter_mut() {
                    if let Some(index) = index {
                        let entry = &set.lines[*index];
                        assert_eq!(entry.block_id(), evicted_directory_entry.0);
                        let access_ts = entry.access_ts();
                        // require recording the timestamp of the operation.
                        set.invalidate(*index);

                        if self.with_statistics {
                            Statistics::global_record(
                                core_id,
                                EventType::PrivateCacheInvalidation,
                                is_os,
                            );

                            if access_ts > ts {
                                Statistics::global_record(
                                    core_id,
                                    EventType::PrivateCacheInvalidationCausailityViolation,
                                    is_os,
                                );
                            }
                        }

                        CacheLineCoherenceHistory::global_record_history(
                            evicted_directory_entry.0,
                            CacheOperationType::Invalidate(p_cache_id),
                            *replica_cache_id,
                            ts,
                            false,
                            evicted_directory_entry.1.sharers,
                            line!(),
                        )
                    }
                }

                // write this back to the shared cache.
                // TODO: Here we should record the causality violation.
                let (_, eviction_violated, _, dirty_wb) = self.shared_cache.insert(
                    SharedCacheAccessSource::Core(core_id),
                    evicted_directory_entry.0,
                    ts,
                    false,
                    true,
                );

                if let Some(dirty_wb_addr) = dirty_wb {
                    timing_bridge_push(
                        4096,
                        dirty_wb_addr,
                        SharerList::ZERO,
                        false,
                        false,
                        false,
                        self.do_ict(dirty_wb_addr, ts),
                        ts
                    );
                }

                if eviction_violated {
                    if self.with_statistics {
                        Statistics::global_record(
                            core_id,
                            EventType::SharedCacheEvictionCausalityViolation,
                            is_os,
                        );
                    }
                }
            }

            let bring_into_shared_cache = if FILL_SCACHE_ON_FILLING_PCACHE {
                match r.access_type {
                    CacheAccessType::InstructionFetch => true,
                    CacheAccessType::DataRead => !ENABLE_EXCLUSIVE_CACHE_STATE,
                    CacheAccessType::DataWrite => false,
                    CacheAccessType::PageWalkRead => true,
                    CacheAccessType::PrefetchRead => !ENABLE_EXCLUSIVE_CACHE_STATE,
                    CacheAccessType::PrefetchWrite => false,
                }
            } else {
                false
            };

            // For MESI coherence protocol, we need to change the access type to write to get the writable permission.
            let request_to_llc = SharedCacheAccessRequest {
                source: SharedCacheAccessSource::Core(core_id),
                block_id,
                access_type: match r.access_type {
                    CacheAccessType::InstructionFetch => CacheAccessType::InstructionFetch,
                    CacheAccessType::DataRead => {
                        if ENABLE_EXCLUSIVE_CACHE_STATE {
                            CacheAccessType::DataWrite
                        } else {
                            CacheAccessType::DataRead
                        }
                    }
                    CacheAccessType::DataWrite => CacheAccessType::DataWrite,
                    CacheAccessType::PageWalkRead => CacheAccessType::PageWalkRead,
                    CacheAccessType::PrefetchRead => {
                        if ENABLE_EXCLUSIVE_CACHE_STATE {
                            CacheAccessType::PrefetchWrite
                        } else {
                            CacheAccessType::PrefetchRead
                        }
                    }
                    CacheAccessType::PrefetchWrite => CacheAccessType::PrefetchWrite,
                },
                is_os,
            };

            let shared_cache_result = if bring_into_shared_cache {
                // here we take the ownership of the cache line from the shared cache to the private cache.
                // So abandon_dirty is true.
                // We also don't need to write through to the LLC, so the is_store is false.
                self.shared_cache
                    .lookup_and_insert_on_miss(&request_to_llc, ts, true)
            } else {
                self.shared_cache.lookup(&request_to_llc, ts)
            };

            miss_directory_guard.update_lru_ts(ts);
            miss_directory_guard.sharers.set(p_cache_id, true);

            let modified = match shared_cache_result {
                SharedCacheLookupResult::Hit(is_modified) => is_store || is_modified,
                SharedCacheLookupResult::Miss(_)
                | SharedCacheLookupResult::ColdMiss
                | SharedCacheLookupResult::EvictedLate(_) => is_store,
                SharedCacheLookupResult::LookupLate(_, _) => false,
            };

            let writable = match shared_cache_result {
                SharedCacheLookupResult::Hit(is_modified) => {
                    if is_modified {
                        true
                    } else {
                        request_to_llc.is_store()
                    }
                }
                SharedCacheLookupResult::Miss(_)
                | SharedCacheLookupResult::ColdMiss
                | SharedCacheLookupResult::EvictedLate(_) => request_to_llc.is_store(),
                SharedCacheLookupResult::LookupLate(_, _) => false,
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

            let shared_cache_hit = matches!(shared_cache_result, SharedCacheLookupResult::Hit(_));

            timing_bridge_push(
                core_id,
                paddr,
                sharers, // 0
                // both read and write need to access memory
                shared_cache_hit,
                false,
                false,
                if shared_cache_hit { 0 } else { self.do_ict(paddr, ts) },
                ts,
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
                SharedCacheLookupResult::Hit(_) => CacheHierarchyAccessResult::HitInSharedCache,
                SharedCacheLookupResult::Miss(_) | SharedCacheLookupResult::ColdMiss => {
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
                }
                SharedCacheLookupResult::LookupLate(_, _) => {
                    Statistics::global_record(
                        core_id,
                        EventType::SharedCacheAccessCausalityViolation,
                        is_os,
                    );
                    CacheHierarchyAccessResult::Unknown
                }
                SharedCacheLookupResult::EvictedLate(_) => {
                    Statistics::global_record(
                        core_id,
                        EventType::SharedCacheEvictionCausalityViolation,
                        is_os,
                    );
                    CacheHierarchyAccessResult::Miss
                }
            };
        }

        // directory hit

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
            // read a shared block
            let result = if ts < miss_directory_guard.lru_ts {
                // violation happens, but it does not change the result of this access.
                CacheHierarchyAccessResult::Unknown
            } else {
                CacheHierarchyAccessResult::HitInOtherPrivateCache
            };

            // add myself to the sharer list.
            miss_directory_guard.update_lru_ts(ts);
            miss_directory_guard.sharers.set(p_cache_id, true);

            if FILL_SCACLE_ON_PCACPE_REPLICA_CREATION {
                self.shared_cache.insert(
                    SharedCacheAccessSource::Core(core_id),
                    block_id,
                    ts,
                    false,
                    true,
                );
            }

            // don't even bother to forward
            timing_bridge_push(
                core_id,
                paddr,
                sharers, // not relevant
                true,
                false,
                false,
                0,
                ts,
            );

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

            (evicted, result)
        } else {
            // read a potentially-modified block, or write a block
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

            let mut res = if !private_hit.permission_violation() {
                CacheHierarchyAccessResult::HitInOtherPrivateCache
            } else {
                CacheHierarchyAccessResult::MissDueToPermission
            };

            let evicted = if is_store {
                // write a block
                let mut incoming_sharer = miss_directory_guard.sharers;
                let mut set_for_refill_lock = None;
                let mut causality_violation = false;

                for (replica_cache_id, set, index) in acquired_sets.iter_mut() {
                    if let Some(index) = index {
                        let entry = &set.lines[*index];
                        assert_eq!(entry.block_id(), block_id);
                        let access_ts = entry.access_ts();
                        // invalid the directory entry.
                        incoming_sharer.set(*replica_cache_id, false);
                        // invalid the private cache entry.
                        set.invalidate(*index);

                        causality_violation |= access_ts > ts;

                        if self.with_statistics {
                            Statistics::global_record(
                                core_id,
                                EventType::PrivateCacheInvalidation,
                                is_os,
                            );

                            if access_ts > ts {
                                Statistics::global_record(
                                    core_id,
                                    EventType::PrivateCacheInvalidationCausailityViolation,
                                    is_os,
                                );
                            }
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
                        // This is the only case that we can see a the private cache does not have this block.
                        assert!(*replica_cache_id == p_cache_id);
                        incoming_sharer.set(*replica_cache_id, false);
                    }

                    if *replica_cache_id == p_cache_id {
                        set_for_refill_lock = Some(set);
                    }
                }

                // Because a write operation has happened, we need to invalidate the shared cache.
                let inv_result = self.shared_cache
                    .invalidate(SharedCacheAccessSource::Core(core_id), block_id, ts);

                // when a write only requires invalidation, the core must
                // already be in the sharer list, and there must be other
                // sharers to invalidate
                let mut next_sharers = acquire_list.clone();
                next_sharers.set(p_cache_id, false);

                if sharers.get(p_cache_id).unwrap() == true && next_sharers.count_ones() > 0 {
                    timing_bridge_push(
                        core_id,
                        paddr,
                        next_sharers,
                        // skip memory access as only invalidation is needed
                        true,
                        false,
                        true,
                        0,
                        ts,
                    );
                } else {
                    // different semantics: a write can miss in the directory or
                    // the core is not in the sharer list. treat the write as
                    // GetX instead
                    let hit = matches!(inv_result, SharedCacheLookupResult::Hit(_)) ||
                                !FILL_SCACLE_ON_PCACPE_REPLICA_CREATION;

                    timing_bridge_push(
                        core_id,
                        paddr,
                        next_sharers,
                        // skip memory access if
                        //   1. the block is already in llc
                        //   2. the read is non-allocating
                        hit,
                        // might be true
                        false,
                        next_sharers.count_ones() > 0,
                        if hit { 0 } else { self.do_ict(paddr, ts) },
                        ts,
                    );
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
                    panic!();
                };

                if !private_hit.permission_violation() {
                    if miss_directory_guard.lru_ts < ts && !causality_violation {
                        res = CacheHierarchyAccessResult::HitInOtherPrivateCache;
                    } else {
                        // This access turns out to be a earlier request
                        res = CacheHierarchyAccessResult::Unknown;
                    }
                }

                // add self to the incoming sharer list.
                incoming_sharer.set(p_cache_id, true);

                // update the directory.
                miss_directory_guard.update_lru_ts(ts);
                miss_directory_guard.sharers = incoming_sharer;

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
                // read a non-shared block
                // You need to find currently whether there are cores that have modified permission.
                let mut find_writable_replica = false;
                let mut modified_replica = false;
                let mut set_for_refill_lock = None;
                let mut causality_violation = false;

                for (replica_cache_id, set, index) in acquired_sets.iter_mut() {
                    if let Some(index) = index {
                        let entry = &set.lines[*index];
                        assert_eq!(entry.block_id(), block_id);
                        if entry.has_write_permission() {
                            // well, if you have write permission, you have to yield the write permission.
                            assert!(!find_writable_replica);

                            if entry.is_modified() {
                                modified_replica = true;
                            }

                            let access_ts = entry.access_ts();
                            causality_violation |= access_ts > ts;

                            // Alright, we find the modifier of this cache line.
                            set.request_sharer(*index, ts);
                            find_writable_replica = true;

                            if self.with_statistics {
                                if access_ts > ts {
                                    Statistics::global_record(
                                        core_id,
                                        EventType::PrivateCacheDowngradeCausalityViolation,
                                        is_os,
                                    );
                                }
                            }
                        }
                    }

                    if *replica_cache_id == p_cache_id {
                        set_for_refill_lock = Some(set);
                    }
                }

                if find_writable_replica {
                    assert!(!miss_directory_guard.shared);
                }

                let (_, _, llc_hit, dirty_wb) = if FILL_SCACLE_ON_PCACPE_REPLICA_CREATION || modified_replica {
                    self.shared_cache.insert(
                        SharedCacheAccessSource::Core(core_id),
                        block_id,
                        ts,
                        modified_replica,
                        true,
                    )
                } else {
                    (false, false, false, None)
                };

                if let Some(dirty_wb_addr) = dirty_wb {
                    timing_bridge_push(
                        4096,
                        dirty_wb_addr,
                        SharerList::ZERO,
                        false,
                        false,
                        false,
                        self.do_ict(dirty_wb_addr, ts),
                        ts
                    );
                }

                // cpu reads a non-shared block
                assert_eq!(sharers.get(p_cache_id).unwrap(), false);

                let hit = llc_hit && !modified_replica || !FILL_SCACLE_ON_PCACPE_REPLICA_CREATION;

                timing_bridge_push(
                    core_id,
                    paddr,
                    sharers,
                    // skip memory access if
                    //   1. llc hits and no writeback is needed
                    //   2. the read is non-allocating
                    hit,
                    // even if no core keeps a modified copy, the broadcast is
                    // still needed
                    false,
                    true,
                    if hit { 0 } else { self.do_ict(paddr, ts) },
                    ts,
                );

                if !private_hit.permission_violation() {
                    if miss_directory_guard.lru_ts < ts && !causality_violation {
                        res = CacheHierarchyAccessResult::HitInOtherPrivateCache;
                    } else {
                        // This access turns out to be earlier than the directory creation. We don't know what happened.
                        res = CacheHierarchyAccessResult::Unknown;
                    }
                } else {
                    // read should never see a permission violation.
                    panic!("Permission violation should not be seen by a read operation.");
                }

                // Then, we need to add self to the directory.
                miss_directory_guard.update_lru_ts(ts);
                miss_directory_guard.sharers.set(p_cache_id, true);
                miss_directory_guard.shared = true;

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

    fn access_from_device_with_pa(
        &self,
        dev_id: u32,
        paddr: u64,
        access_type: CacheAccessType,
        ts: u64,
    ) -> CacheHierarchyAccessResult {
        let block_id = paddr >> CACHE_LINE_SIZE.trailing_zeros();
        // get directory lock.
        let mut directory_set_lock_guard = self.directory.fetch_one_entry(block_id);

        let (require_llc_access, broadcast, sharers) =
            if let Some(directory_entry) = directory_set_lock_guard.get(block_id) {
                let mut acquire_list = self
                    .private_caches
                    .get_set_guard_by_sharer_list(block_id, directory_entry.sharers);
                match access_type {
                    CacheAccessType::InstructionFetch => unreachable!(),
                    CacheAccessType::DataRead | CacheAccessType::PageWalkRead => {
                        let res = (acquire_list.len() == 0,
                                  !directory_entry.shared && directory_entry.sharers.count_ones() > 0,
                                   directory_entry.sharers);

                        if !directory_entry.shared {
                            for (_, set, index) in acquire_list.iter_mut() {
                                if let Some(index) = index {
                                    // TODO: update the counter.
                                    set.request_sharer(*index, ts);
                                }
                            }

                            if acquire_list.len() > 0 {
                                directory_entry.shared = true;
                            }
                        }

                        res
                    }

                    CacheAccessType::DataWrite => {
                        for (_, set, index) in acquire_list.iter_mut() {
                            if let Some(index) = index {
                                set.invalidate(*index);
                            }
                        }

                        let old_sharers = directory_entry.sharers;

                        directory_entry.shared  = false;
                        directory_entry.sharers = SharerList::ZERO;

                        (true, old_sharers.count_ones() > 0, old_sharers)
                    }

                    CacheAccessType::PrefetchRead => unreachable!(),
                    CacheAccessType::PrefetchWrite => unreachable!(),
                }
            } else {
                (true, false, SharerList::ZERO)
            };

        if require_llc_access {
            let llc_request = SharedCacheAccessRequest {
                is_os: true, // Note: I/O request is always treated as OS.
                source: SharedCacheAccessSource::Device,
                block_id,
                access_type: access_type.clone(),
            };

            // Well, this cache line should be inserted into LLC, because there might be reuse by the I/O device in the future.
            return match self
                .shared_cache
                .lookup_and_insert_on_miss(&llc_request, ts, true)
            {
                SharedCacheLookupResult::Hit(_) => {
                    timing_bridge_push(
                        dev_id,
                        paddr,
                        sharers,
                        true,
                        false,
                        broadcast,
                        0,
                        ts,
                    );
                    CacheHierarchyAccessResult::HitInSharedCache
                }
                SharedCacheLookupResult::Miss(dirty_wb) => {
                    if let Some(dirty_wb_addr) = dirty_wb {
                        timing_bridge_push(
                            4096,
                            dirty_wb_addr,
                            SharerList::ZERO,
                            false,
                            false,
                            false,
                            self.do_ict(dirty_wb_addr, ts),
                            ts
                        );
                    }
                    timing_bridge_push(
                        dev_id,
                        paddr,
                        sharers,
                        false,
                        false,
                        broadcast,
                        self.do_ict(paddr, ts),
                        ts,
                    );
                    CacheHierarchyAccessResult::Miss
                }
                SharedCacheLookupResult::ColdMiss => {
                    timing_bridge_push(
                        dev_id,
                        paddr,
                        sharers,
                        false,
                        false,
                        broadcast,
                        self.do_ict(paddr, ts),
                        ts,
                    );
                    CacheHierarchyAccessResult::Miss
                }
                SharedCacheLookupResult::LookupLate(_, _) => CacheHierarchyAccessResult::Unknown,
                SharedCacheLookupResult::EvictedLate(_) => CacheHierarchyAccessResult::Miss,
            };
        } else {
            timing_bridge_push(
                dev_id,
                paddr,
                sharers,
                false, // irrelevant
                true,
                false,
                0,
                ts,
            );
            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        }
    }
}

impl<
    MMU: AbstractMMU,
    PCache: PrivateCaches,
    SCache: SharedCache,
    const FILL_SCACHE_ON_FILLING_PCACHE: bool,
    const FILL_SCACLE_ON_PCACHE_EVICTION: bool,
    const FILL_SCACHE_ON_PCACHE_WRITEBACK: bool,
    const FILL_SCACLE_ON_PCACPE_REPLICA_CREATION: bool,
    const DIRECTORY_SHARD_COUNT: usize,
    const DIRECTORY_ASSO: usize,
    const CORE_COUNT: usize,
> ParallelMemoryHierarchy<
        MMU,
        PCache,
        SCache,
        FILL_SCACHE_ON_FILLING_PCACHE,
        FILL_SCACLE_ON_PCACHE_EVICTION,
        FILL_SCACHE_ON_PCACHE_WRITEBACK,
        FILL_SCACLE_ON_PCACPE_REPLICA_CREATION,
        DIRECTORY_SHARD_COUNT,
        DIRECTORY_ASSO,
        CORE_COUNT,
> {
    fn do_ict(&self, paddr: u64, ts: u64) -> u64 {
        if !unsafe { ICT } {
            return 0u64;
        }

        let mut l = 0u64;
        let mut a = paddr;

        for i in 0 ..= 4 {
            a = (1u64 << 48) | (((a >> 12) & 0x1fffffffffffu64) << 3);

            let r = SharedCacheAccessRequest {
                is_os: true,
                source: SharedCacheAccessSource::Device,
                block_id: a >> CACHE_LINE_SIZE.trailing_zeros(),
                access_type: CacheAccessType::DataRead
            };

            // let's assume that the effect of mlb is minimal
            match self.shared_cache.lookup_and_insert_on_miss(&r, ts, true) {
                SharedCacheLookupResult::Hit(_) => {
                    l |= 1u64 << i;
                    break;
                }
                _ => {}
            }
        }

        l
    }
}
