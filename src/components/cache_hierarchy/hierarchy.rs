use crate::components::cache_hierarchy::shared_cache::SharedCache;
use crate::parameter::{ADJACENT_LINE_PREFETCHING, ENABLE_CACHE_LINE_HISTORY};
use crate::{components::cache_hierarchy::directory::SharerList, parameter};

use crate::components::debug::statistics::{EventType, Statistics};

use crate::components::debug::cache_line_history::{CacheLineCoherenceHistory, CacheOperationType};

use super::directory::DirectorySet;
use super::{
    directory,
    private_cache::{self, PrivateCaches},
};

use crate::components::mmu::AbstractMMU;
use std::cell::UnsafeCell;
use std::ops::DerefMut;

#[cfg(test)]
mod debug_tests;
#[cfg(test)]
mod harvard_reverse_order_tests;
#[cfg(test)]
mod harvard_tests;
#[cfg(test)]
mod reverse_order_tests;

pub struct MemoryHierarchy<
    MMU: AbstractMMU,
    PCache: PrivateCaches,
    SCache: SharedCache,
    const PRECISE_COHERENCE_RECONSTRUCTION: bool,
    const FILL_SCACHE_ON_FILLING_PCACHE: bool,
    const FILL_SCACLE_ON_PCACHE_CLEAN_EVICTION: bool,
    const FILL_SCACHE_ON_PCACHE_DIRTY_EVICTION: bool,
    const DIRECTORY_SHARD_COUNT: usize,
> {
    mmus: [UnsafeCell<MMU>; parameter::CORE_COUNT],

    private_caches: PCache,
    directory: directory::Directory<DIRECTORY_SHARD_COUNT>,

    shared_cache: SCache,
}

#[derive(Debug, PartialEq)]
pub enum CacheAccessType {
    InstructionFetch,

    DataRead,
    DataWrite,

    PageWalkRead,

    PrefetchRead,
    PrefetchWrite,
}

#[derive(PartialEq, Eq, Debug)]
pub enum CacheHierarchyAccessResult {
    HitInSelfPrivateCache,
    MissDueToPermission,
    HitInOtherPrivateCache,
    MissInPrivateCache, // This entry is emitted when we see order violation, because we don't know its state in the shared cache.
    HitInSharedCache,
    Miss,
}

impl<
        MMU: AbstractMMU,
        PCache: PrivateCaches,
        SCache: SharedCache,
        const PRECISE_COHERENCE_RECONSTRUCTION: bool,
        const FILL_SCACHE_ON_FILLING_PCACHE: bool,
        const FILL_SCACLE_ON_PCACHE_EVICTION: bool,
        const FILL_SCACHE_ON_PCACHE_WRITEBACK: bool,
        const DIRECTORY_SHARD_COUNT: usize,
    > Default
    for MemoryHierarchy<
        MMU,
        PCache,
        SCache,
        PRECISE_COHERENCE_RECONSTRUCTION,
        FILL_SCACHE_ON_FILLING_PCACHE,
        FILL_SCACLE_ON_PCACHE_EVICTION,
        FILL_SCACHE_ON_PCACHE_WRITEBACK,
        DIRECTORY_SHARD_COUNT,
    >
{
    fn default() -> Self {
        Self::new()
    }
}

impl<
        MMU: AbstractMMU,
        PCache: PrivateCaches,
        SCache: SharedCache,
        const PRECISE_COHERENCE_RECONSTRUCTION: bool,
        const FILL_SCACHE_ON_FILLING_PCACHE: bool,
        const FILL_SCACLE_ON_PCACHE_EVICTION: bool,
        const FILL_SCACHE_ON_PCACHE_WRITEBACK: bool,
        const DIRECTORY_SHARD_COUNT: usize,
    >
    MemoryHierarchy<
        MMU,
        PCache,
        SCache,
        PRECISE_COHERENCE_RECONSTRUCTION,
        FILL_SCACHE_ON_FILLING_PCACHE,
        FILL_SCACLE_ON_PCACHE_EVICTION,
        FILL_SCACHE_ON_PCACHE_WRITEBACK,
        DIRECTORY_SHARD_COUNT,
    >
{
    pub fn new() -> Self {
        Self {
            mmus: std::array::from_fn(|_| UnsafeCell::new(MMU::new())),
            private_caches: PCache::new(),
            directory: directory::Directory::new(),
            shared_cache: SCache::new(),
        }
    }

    pub fn access_memory_with_va(
        &self,
        core_id: u32,
        va: u64,
        ts: u64,
        is_store: bool,
        is_instruction: bool,
    ) {
        assert!(
            !(is_instruction && is_store),
            "Instruction and store permission cannot be used at the same time."
        );

        let access_type = if is_instruction {
            CacheAccessType::InstructionFetch
        } else if is_store {
            CacheAccessType::DataWrite
        } else {
            CacheAccessType::DataRead
        };

        let prefetch_access_type = if is_instruction {
            CacheAccessType::PrefetchRead
        } else if is_store {
            CacheAccessType::PrefetchWrite
        } else {
            CacheAccessType::PrefetchRead
        };

        let translation = unsafe {
            self.mmus[core_id as usize]
                .get()
                .as_mut()
                .unwrap()
                .translate_and_refill(va, ts)
        };

        match translation {
            crate::components::mmu::MMUTranslationResult::Hit(pa) => {
                let block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, access_type);
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(core_id, block_id + 1, ts, prefetch_access_type);
                }
            }
            crate::components::mmu::MMUTranslationResult::Miss(paddr, walk_trace) => {
                // replay the trace.
                for pa in walk_trace {
                    if pa == u64::MAX {
                        break;
                    }
                    let pte_block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                    self.access_memory_pblock_id(
                        core_id,
                        pte_block_id,
                        ts,
                        CacheAccessType::PageWalkRead,
                    );
                }
                let block_id = paddr >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, access_type);
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(core_id, block_id + 1, ts, prefetch_access_type);
                }

                Statistics::global_record(core_id, EventType::TLBMiss);
            }
            crate::components::mmu::MMUTranslationResult::MissNotCacheable(pa) => {
                let block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, access_type);
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(core_id, block_id + 1, ts, prefetch_access_type);
                }
            }
        }
    }

    pub fn access_memory_with_va_and_pa(
        &mut self,
        core_id: u32,
        va: u64,
        reference_pa: u64,
        ts: u64,
        is_store: bool,
        is_instruction: bool,
    ) {
        assert!(
            !(is_instruction && is_store),
            "Instruction and store permission cannot be used at the same time."
        );

        let access_type = if is_instruction {
            CacheAccessType::InstructionFetch
        } else if is_store {
            CacheAccessType::DataWrite
        } else {
            CacheAccessType::DataRead
        };

        let prefetch_access_type = if is_instruction {
            CacheAccessType::PrefetchRead
        } else if is_store {
            CacheAccessType::PrefetchWrite
        } else {
            CacheAccessType::PrefetchRead
        };

        let translation = unsafe {
            self.mmus[core_id as usize]
                .get()
                .as_mut()
                .unwrap()
                .translate_and_refill(va, ts)
        };

        match translation {
            crate::components::mmu::MMUTranslationResult::Hit(_pa) => {
                // assert!(pa == reference_pa);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, access_type);
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(core_id, block_id + 1, ts, prefetch_access_type);
                }
            }
            crate::components::mmu::MMUTranslationResult::Miss(_pa, walk_trace) => {
                // replay the trace.
                for trace_pa in walk_trace {
                    if trace_pa == u64::MAX {
                        break;
                    }
                    let pte_block_id = trace_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                    self.access_memory_pblock_id(
                        core_id,
                        pte_block_id,
                        ts,
                        CacheAccessType::PageWalkRead,
                    );
                }
                // assert!(pa == reference_pa as u64);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, access_type);
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(core_id, block_id + 1, ts, prefetch_access_type);
                }

                Statistics::global_record(core_id, EventType::TLBMiss);
            }
            crate::components::mmu::MMUTranslationResult::MissNotCacheable(_pa) => {
                // assert!(pa == reference_pa as u64);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, access_type);
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(core_id, block_id + 1, ts, prefetch_access_type);
                }
            }
        }
    }

    pub fn access_memory_pblock_id(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        access_type: CacheAccessType,
    ) -> CacheHierarchyAccessResult {
        let is_prefetch = access_type == CacheAccessType::PrefetchRead
            || access_type == CacheAccessType::PrefetchWrite;

        let is_instruction = access_type == CacheAccessType::InstructionFetch;
        let is_store = access_type == CacheAccessType::DataWrite;
        let is_page_walk = access_type == CacheAccessType::PageWalkRead;

        if !is_prefetch {
            Statistics::global_record(core_id, EventType::MemoryAccess);
            if is_instruction {
                Statistics::global_record(core_id, EventType::InstructionAccess);
            } else {
                Statistics::global_record(core_id, EventType::DataAccess);
            }
        }

        // first, we need to check the private cache.
        let private_hit =
            self.private_caches
                .poke_and_update(core_id, block_id, ts, is_instruction, is_store);

        if private_hit == private_cache::PrivateCachePokeResult::Hit {
            // we don't have to anything. Just return.
            return CacheHierarchyAccessResult::HitInSelfPrivateCache;
        }

        // Alright, we may need to get another directory entry of the eviction.
        // This entry may bot be used, because other entry in the same set can be evicted. But we need to get it ahead of time to avoid deadlock.

        if !is_prefetch {
            Statistics::global_record(core_id, EventType::PrivateCacheMiss);

            if is_instruction {
                Statistics::global_record(core_id, EventType::PrivateICacheMiss);
            } else if is_page_walk {
                Statistics::global_record(core_id, EventType::PrivateCacheMissDueToPTW);
            } else {
                Statistics::global_record(core_id, EventType::PrivateDCacheMiss);
            }
        }

        let evicted_slot = match private_hit {
            private_cache::PrivateCachePokeResult::Hit => unreachable!(),
            private_cache::PrivateCachePokeResult::Miss(ref slot) => slot.clone(),
            private_cache::PrivateCachePokeResult::PermissionViolation(ref slot) => slot.clone(),
        };

        // get the locks for the fill and the evict, in a fixed order, if possible.
        let (mut miss_directory_set_guard, evict_directory) = {
            match evicted_slot {
                private_cache::EvictedSlot::Valid(_, potential_evicted_id) => {
                    let (m_guard, e_guard) = self
                        .directory
                        .fetch_two_entries(block_id, potential_evicted_id);
                    (m_guard, Some((potential_evicted_id, e_guard)))
                }
                _ => {
                    let directory_set_guard = self.directory.lock_set(block_id);
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

        if PRECISE_COHERENCE_RECONSTRUCTION && miss_directory_guard.modify_ts_before_eviction > ts {
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

            return CacheHierarchyAccessResult::MissInPrivateCache;
        }

        // if it is miss, we need to access the last level cache as well, and add it.
        if sharers.count_ones() == 0 {
            let shared_cache_result = if FILL_SCACHE_ON_FILLING_PCACHE {
                // here we take the ownership of the cache line from the shared cache to the private cache.
                // So abandon_dirty is true.
                // We also don't need to write through to the LLC, so the is_store is false.
                self.shared_cache
                    .lookup_and_insert_on_miss(core_id, block_id, ts, true, false, true)
            } else {
                self.shared_cache.lookup(core_id, block_id, ts, true)
            };

            miss_directory_guard.ts = ts;
            miss_directory_guard.sharers.set(p_cache_id, true);

            if is_store {
                miss_directory_guard.modify_ts_before_eviction = ts;
            }

            // the dirtiness of the cache line in the shared cache is passed to the private cache.
            let modified = shared_cache_result.unwrap_or(false) || is_store;

            let writable = if !parameter::ENABLE_EXCLUSIVE_CACHE_STATE {
                modified
            } else {
                !is_instruction
            };

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

            let mut set_to_fill =
                self.private_caches
                    .get_set_for_fill(core_id, block_id, is_instruction);

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
                );
            }

            if !is_prefetch {
                Statistics::global_record(core_id, EventType::SharedCacheAccess);
            }

            if shared_cache_result.is_some() {
                return CacheHierarchyAccessResult::HitInSharedCache;
            } else {
                if !is_prefetch {
                    if is_page_walk {
                        Statistics::global_record(core_id, EventType::SharedCacheMissDueToPTW);
                    } else {
                        Statistics::global_record(core_id, EventType::SharedCacheMiss);
                    }
                }
                return CacheHierarchyAccessResult::Miss;
            }
        }

        if is_prefetch {
            return CacheHierarchyAccessResult::Miss;
        }

        let mut acquire_list = sharers;
        // this list should either
        // - Not contain the current core, so it is a miss, or
        // - Contain the current core, because the permission is wrong.
        assert!(
            acquire_list.get(p_cache_id).unwrap() == false || private_hit.permission_violation()
        );

        // the core itself should be also part of the acquire_list.
        acquire_list.set(p_cache_id, true);

        // Now, acquire the lock of all sets of the private cache, suggested by the directory.
        let mut acquired_sets = self
            .private_caches
            .get_set_guard_by_sharer_list(block_id, acquire_list);

        if PRECISE_COHERENCE_RECONSTRUCTION {
            // Do we have another sharer that has a write permission with a larger timestamp?
            let mut other_has_write_permission_with_late_ts = false;
            let mut other_write_ts = 0;
            let mut other_sharer_id = 0;
            for (replica_cache_id, set, index) in acquired_sets.iter() {
                if let Some(index) = index {
                    let line = &set.lines[*index];
                    assert_eq!(line.block_id(), block_id);
                    if line.write_ts() > ts {
                        other_has_write_permission_with_late_ts = true;
                        if line.write_ts() > other_write_ts {
                            other_write_ts = line.write_ts();
                            other_sharer_id = *replica_cache_id;
                        }
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

            if other_has_write_permission_with_late_ts {
                // Well, this cache line is already touched by another core with a later timestamp.
                // Only that core should be kept.

                // Update the directory.
                let mut incoming_sharer = SharerList::ZERO;
                incoming_sharer.set(other_sharer_id, true);
                miss_directory_guard.ts = ts;
                miss_directory_guard.sharers = incoming_sharer;

                for (replica_cache_id, set, idx) in acquired_sets.iter_mut() {
                    if *replica_cache_id != other_sharer_id {
                        if idx.is_some() {
                            assert_eq!(set.lines[idx.unwrap()].block_id(), block_id);
                            set.invalidate(idx.unwrap());
                        } else {
                            assert_eq!(*replica_cache_id, p_cache_id);
                        }
                    }
                }

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

                // We don't know whether this cache line should trigger a hit or miss. It definitely misses in the private cache.
                return CacheHierarchyAccessResult::MissInPrivateCache;
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

            // Here actually we can do something to tell the difference between CleanExclusive and CleanShared.
            // When checking replica, we can see the number of replica. If it is 1 and its owner is the current core, then it is CleanExclusive.
            // We can just return HitInSelfPrivateCache if the coherence protocol is MESI and the timing information is needed.

            for (replica_cache_id, set, index) in acquired_sets.iter_mut() {
                if let Some(index) = index {
                    let entry = &set.lines[*index];
                    assert_eq!(entry.block_id(), block_id);
                    if !PRECISE_COHERENCE_RECONSTRUCTION || entry.access_ts() < ts {
                        // invalid the directory entry.
                        incoming_sharer.set(*replica_cache_id, false);
                        // invalid the private cache entry.
                        set.invalidate(*index);
                        if ENABLE_CACHE_LINE_HISTORY {
                            CacheLineCoherenceHistory::global_record_history(
                                block_id,
                                CacheOperationType::Invalidate(p_cache_id),
                                *replica_cache_id,
                                ts,
                                false,
                                incoming_sharer,
                                line!(),
                            )
                        }
                    } else {
                        assert!(entry.write_ts() <= ts);
                        assert!(*replica_cache_id != p_cache_id);

                        // This means you will only get the read permission, because there is a core with read permission and large timestamp.

                        // The result must be inaccurate because the access arrives OoO.
                        res = CacheHierarchyAccessResult::MissInPrivateCache;
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

            // add self to the incoming sharer list.
            incoming_sharer.set(p_cache_id, true);

            // update the directory.
            miss_directory_guard.ts = ts;
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
            // You need to find currently whether there are cores that have modified permission.
            let mut already_modified = false;
            let mut set_for_refill_lock = None;

            for (replica_cache_id, set, index) in acquired_sets.iter_mut() {
                if let Some(index) = index {
                    let entry = &set.lines[*index];
                    assert_eq!(entry.block_id(), block_id);
                    if entry.has_write_permission() {
                        // well, if you have write permission, you have to yield the write permission.
                        assert!(!already_modified);

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
                            return CacheHierarchyAccessResult::MissInPrivateCache;
                        }

                        // Alright, we find the modifier of this cache line.
                        set.request_sharer(*index, ts);
                        already_modified = true;
                    }
                }

                if *replica_cache_id == p_cache_id {
                    set_for_refill_lock = Some(set);
                }
            }

            // Then, we need to add self to the directory.
            miss_directory_guard.ts = ts;
            miss_directory_guard.sharers.set(p_cache_id, true);

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
            );
        }

        res
    }

    pub fn handle_eviction<const SET: usize>(
        &self,
        directory_set_guard: &mut impl DerefMut<Target = DirectorySet<SET>>,
        cache_id: usize,
        block_id: u64,
        ts: u64,
        modified: bool,
    ) {
        let directory_entry = directory_set_guard.get_or_create(block_id);

        // we cancel the element of this block in the directory.
        let sharer = directory_entry.sharers;

        if sharer.get(cache_id).unwrap() == false {
            // Well, it is already invalid by other core.
            if parameter::ENABLE_CACHE_LINE_HISTORY {
                let his = CacheLineCoherenceHistory::global_get_block_history(block_id).unwrap();
                his.value().print_history();
                println!("Failed operation: {:?}, Cache ID: {}, Timestamp: {}, Refilled: false, Share List: {:?}",
                    CacheOperationType::Drop, cache_id, ts, sharer.iter_ones().collect::<Vec<usize>>() );
            }
            panic!();
        }

        // we put the element back to the directory.
        directory_entry.ts = ts;
        directory_entry.sharers.set(cache_id, false);

        CacheLineCoherenceHistory::global_record_history(
            block_id,
            CacheOperationType::Drop,
            cache_id,
            ts,
            false,
            directory_entry.sharers,
            line!(),
        );

        // Also update the writer timestamp before eviction.
        if modified {
            // keep the latest write timestamp.
            directory_entry.modify_ts_before_eviction =
                if directory_entry.modify_ts_before_eviction < ts {
                    ts
                } else {
                    directory_entry.modify_ts_before_eviction
                };
        }

        // before releasing the lock of the directory, we need to check whether we need to place this lock to the shared cache.
        if directory_entry.sharers.count_ones() == 0 {
            // we need to place this block to the shared cache.
            Statistics::global_record(
                PCache::find_cache_info_by_cache_id(cache_id).0,
                EventType::SharedCacheAccess,
            );

            let core_id = PCache::find_cache_info_by_cache_id(cache_id).0;

            if FILL_SCACLE_ON_PCACHE_EVICTION && !modified {
                self.shared_cache
                    .insert(core_id, block_id, ts, modified, true);
            }

            if FILL_SCACHE_ON_PCACHE_WRITEBACK && modified {
                self.shared_cache
                    .insert(core_id, block_id, ts, modified, true);
            }
        }
    }

    pub fn dump_access_counter(&self) {
        // self.shared_cache.dump_access_counter();
    }

    pub fn dump_snapshot(&self, snapshot_folder: &str) {
        self.private_caches.dump_snapshot(snapshot_folder);
        self.directory.dump_snapshot(snapshot_folder);
        // self.shared_cache.dump_snapshot(snapshot_folder);
    }

    pub fn get_scache_warmed_set_count(&self) -> usize {
        self.shared_cache.warmed_sets_count()
    }

    pub fn get_scache_warmed_slots_count(&self) -> usize {
        self.shared_cache.warmed_slots_count()
    }

    pub fn information() -> String {
        format!(
            "Private Cache: {}\nShared Cache: {}\nPrecise Coherence Reconstruction: {} \n Fill Shared Cache on Filling Private Cache: {} \n Fill Shared Cache on Private Cache Clean Eviction: {} \n Fill Shared Cache on Private Cache Dirty Eviction: {}",
            PCache::information(),
            SCache::information(),
            PRECISE_COHERENCE_RECONSTRUCTION,
            FILL_SCACHE_ON_FILLING_PCACHE,
            FILL_SCACLE_ON_PCACHE_EVICTION,
            FILL_SCACHE_ON_PCACHE_WRITEBACK
        )
    }
}
