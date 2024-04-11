use crate::parameter::DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION;
use crate::{components::cache_hierarchy::directory::SharerList, parameter};

use crate::components::debug::statistics::{EventType, Statistics};

use crate::components::debug::cache_line_history::{CacheLineCoherenceHistory, CacheOperationType};

use super::directory::DirectorySet;
use super::{
    directory,
    private_cache::{self, PrivateCaches},
    shared_cache,
};

use gcd;

use crate::components::mmu::AbstractMMU;
use std::cell::UnsafeCell;
use std::sync::MutexGuard;

mod debug_tests;
mod harvard_reverse_order_tests;
mod harvard_tests;
mod reverse_order_tests;

const DIRECTORY_SET: usize = if parameter::USE_UNIFIED_CACHE {
    parameter::UNIFIED_PRI_CACHE_SET
} else {
    gcd::binary_usize(
        parameter::HARVARD_PRI_I_CACHE_SET,
        parameter::HARVARD_PRI_D_CACHE_SET,
    )
};

pub struct LockedMemoryHierarchy<MMU: AbstractMMU, PCache: PrivateCaches> {
    mmus: [UnsafeCell<MMU>; parameter::CORE_COUNT],

    private_caches: PCache,
    // In case the hardware has separate L1i and L1d, and there is no private L2, we can just add two groups of caches.
    // The logic to handle it is the same. It is equivalent that we have more cores with a single private cache.
    // There might be a way to optimize if the permission is shared. I need to think about it.
    directory: directory::Directory<{ DIRECTORY_SET }>,

    shared_cache: shared_cache::SharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
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

impl<MMU: AbstractMMU, PCache: PrivateCaches> LockedMemoryHierarchy<MMU, PCache> {
    pub fn new() -> Self {
        Self {
            mmus: std::array::from_fn(|_| UnsafeCell::new(MMU::new())),
            private_caches: PCache::new(),
            directory: directory::Directory::new(),
            shared_cache: shared_cache::SharedCache::new(),
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
                self.access_memory_pblock_id(core_id, block_id, ts, is_store, is_instruction);
            }
            crate::components::mmu::MMUTranslationResult::Miss(paddr, walk_trace) => {
                // replay the trace.
                for pa in walk_trace {
                    if pa == u64::MAX {
                        break;
                    }
                    let block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                    self.access_memory_pblock_id(core_id, block_id, ts, false, false);
                }
                let block_id = paddr >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, is_store, is_instruction);

                Statistics::global_record(core_id, EventType::TLBMiss);
            }
            crate::components::mmu::MMUTranslationResult::MissNotCacheable(pa) => {
                let block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, is_store, is_instruction);
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
        let translation = unsafe {
            self.mmus[core_id as usize]
                .get()
                .as_mut()
                .unwrap()
                .translate_and_refill(va, ts)
        };

        match translation {
            crate::components::mmu::MMUTranslationResult::Hit(pa) => {
                // assert!(pa == reference_pa);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, is_store, is_instruction);
            }
            crate::components::mmu::MMUTranslationResult::Miss(pa, walk_trace) => {
                // replay the trace.
                for trace_pa in walk_trace {
                    if trace_pa == u64::MAX {
                        break;
                    }
                    let block_id = trace_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                    self.access_memory_pblock_id(core_id, block_id, ts, false, false);
                }
                // assert!(pa == reference_pa as u64);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, is_store, is_instruction);

                Statistics::global_record(core_id, EventType::TLBMiss);
            }
            crate::components::mmu::MMUTranslationResult::MissNotCacheable(pa) => {
                // assert!(pa == reference_pa as u64);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, is_store, is_instruction);
            }
        }
    }

    pub fn access_memory_pblock_id(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_store: bool,
        is_instruction: bool,
    ) -> CacheHierarchyAccessResult {
        Statistics::global_record(core_id, EventType::MemoryAccess);

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

        Statistics::global_record(core_id, EventType::PrivateCacheMiss);

        // now, it is a miss. We need to check the directory.
        let mut directory_set_guard = self.directory.get_set(block_id);
        let directory_entry = directory_set_guard.get_or_create(block_id);

        let sharers = directory_entry.sharers;

        // Now, we communicate with the directory. Current miss is mapped to the following position in the directory entry share list.
        let p_cache_id = PCache::get_cache_id_by_cache_info(core_id, is_instruction);

        let record_op = if is_store {
            CacheOperationType::GetM
        } else {
            CacheOperationType::GetR
        };

        if !DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION
            && directory_entry.modify_ts_before_eviction > ts
        {
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
            // NOTE: currently shared cache access is disabled.
            let shared_cache_result = self.shared_cache.lookup(block_id);

            directory_entry.ts = ts;
            directory_entry.sharers.set(p_cache_id as usize, true);

            if is_store {
                directory_entry.modify_ts_before_eviction = ts;
            }

            let evicted = self.private_caches.refill_from_shared_cache(
                core_id,
                block_id,
                ts,
                is_instruction,
                is_store,
            );

            // Here we have a problem. The line is evicted from the cache, but there is no notification to the directory that the line is evicted.
            // In order to do so, we need to get the lock of evicted line.

            CacheLineCoherenceHistory::global_record_history(
                block_id,
                record_op,
                p_cache_id,
                ts,
                true,
                directory_entry.sharers,
                line!(),
            );

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(
                    &mut directory_set_guard,
                    p_cache_id,
                    evicted_line.block_id(),
                    ts,
                    evicted_line.is_modified(),
                );
            }

            Statistics::global_record(core_id, EventType::SharedCacheAccess);

            if shared_cache_result {
                return CacheHierarchyAccessResult::HitInSharedCache;
            } else {
                Statistics::global_record(core_id, EventType::SharedCacheMiss);
                return CacheHierarchyAccessResult::Miss;
            }
        }

        let mut acquire_list = sharers.clone();
        // this list should either
        // - Not contain the current core, so it is a miss, or
        // - Contain the current core, because the permission is wrong.
        assert!(
            acquire_list.get(p_cache_id as usize).unwrap() == false
                || private_hit == private_cache::PrivateCachePokeResult::PermissionViolation
        );

        // the core itself should be also part of the acquire_list.
        acquire_list.set(p_cache_id as usize, true);

        // Now, acquire the lock of all sets of the private cache, suggested by the directory.
        let mut acquired_sets = self
            .private_caches
            .get_set_guard_by_sharer_list(block_id, acquire_list);

        if !DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION {
            // Do we have another sharer that has a write permission with a larger timestamp?
            let mut other_has_write_permission_with_late_ts = false;
            let mut other_write_ts = 0;
            let mut other_sharer_id = 0;
            for (replica_cache_id, set) in acquired_sets.iter() {
                if let Some(line) = set.poke(block_id) {
                    if  line.write_ts() > ts {
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
                incoming_sharer.set(other_sharer_id as usize, true);
                directory_entry.ts = ts;
                directory_entry.sharers = incoming_sharer;

                for (replica_cache_id, set) in acquired_sets.iter_mut() {
                    if *replica_cache_id != other_sharer_id {
                        set.invalidate(block_id);
                    }
                }

                CacheLineCoherenceHistory::global_record_history(
                    block_id,
                    record_op,
                    p_cache_id,
                    ts,
                    false,
                    directory_entry.sharers,
                    line!(),
                );

                // Now, release the lock of the private cache.
                drop(acquired_sets);

                // We don't know whether this cache line should trigger a hit or miss. It definitely misses in the private cache.
                return CacheHierarchyAccessResult::MissInPrivateCache;
            }
        }

        let mut res = if private_hit != private_cache::PrivateCachePokeResult::PermissionViolation {
            CacheHierarchyAccessResult::HitInOtherPrivateCache
        } else {
            CacheHierarchyAccessResult::MissDueToPermission
        };

        let evicted = if is_store {
            let mut incoming_sharer = directory_entry.sharers.clone();
            let mut set_for_refill_lock = None;

            // Here actually we can do something to tell the difference between CleanExclusive and CleanShared.
            // When checking replica, we can see the number of replica. If it is 1 and its owner is the current core, then it is CleanExclusive.
            // We can just return HitInSelfPrivateCache if the coherence protocol is MESI and the timing information is needed.

            for (replica_cache_id, set) in acquired_sets.iter_mut() {
                if let Some(entry) = set.poke(block_id) {
                    if DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION || entry.access_ts() < ts {
                        // invalid the directory entry.
                        incoming_sharer.set(*replica_cache_id as usize, false);
                        // invalid the private cache entry.
                        set.invalidate(block_id);
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
                    incoming_sharer.set(*replica_cache_id as usize, false);
                }

                if *replica_cache_id == p_cache_id {
                    set_for_refill_lock = Some(set);
                }
            }

            let set_for_refill_lock = set_for_refill_lock.unwrap();

            let evicted = if incoming_sharer.count_ones() == 0 {
                // This means there is no sharer. The core will get modified permission.
                set_for_refill_lock.refill(block_id, ts, is_instruction, true)
            } else {
                // There are sharers. So unfortunately, you can only get shared permission.
                set_for_refill_lock.refill(block_id, ts, is_instruction, false)
            };

            // add self to the incoming sharer list.
            incoming_sharer.set(p_cache_id as usize, true);

            // update the directory.
            directory_entry.ts = ts;
            directory_entry.sharers = incoming_sharer;

            CacheLineCoherenceHistory::global_record_history(
                block_id,
                record_op,
                p_cache_id,
                ts,
                true,
                directory_entry.sharers,
                line!(),
            );

            drop(acquired_sets);

            evicted
        } else {
            // You need to find currently whether there are cores that have modified permission.
            let mut already_modified = false;
            let mut set_for_refill_lock = None;

            for (replica_cache_id, set) in acquired_sets.iter_mut() {
                if let Some(entry) = set.poke(block_id) {
                    if entry.is_modified() {
                        assert!(already_modified == false);

                        if entry.write_ts() > ts && !DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION
                        {
                            // OK, this read operation is also not ordered.
                            // There is nothing we need to do.

                            CacheLineCoherenceHistory::global_record_history(
                                block_id,
                                record_op,
                                p_cache_id,
                                ts,
                                false,
                                directory_entry.sharers,
                                line!(),
                            );

                            drop(acquired_sets);
                            return CacheHierarchyAccessResult::MissInPrivateCache;
                        }

                        // Alright, we find the modifier of this cache line.
                        set.request_sharer(block_id, ts);
                        already_modified = true;
                    }
                }

                if *replica_cache_id == p_cache_id {
                    set_for_refill_lock = Some(set);
                }
            }

            // Then, we need to add self to the directory.
            directory_entry.ts = ts;
            directory_entry.sharers.set(p_cache_id as usize, true);

            // We can insert the block to the private cache now.
            let set_for_refill_lock = set_for_refill_lock.unwrap();

            let evicted = set_for_refill_lock.refill(block_id, ts, is_instruction, false);

            CacheLineCoherenceHistory::global_record_history(
                block_id,
                record_op,
                p_cache_id,
                ts,
                true,
                directory_entry.sharers,
                line!(),
            );

            drop(acquired_sets);

            evicted
        };

        if let Some(evicted_line) = evicted {
            self.handle_eviction(
                &mut directory_set_guard,
                p_cache_id,
                evicted_line.block_id(),
                ts,
                evicted_line.is_modified(),
            );
        }

        return res;
    }

    pub fn handle_eviction(
        &self,
        directory_set_guard: &mut MutexGuard<'_, DirectorySet<DIRECTORY_SET>>,
        cache_id: usize,
        block_id: u64,
        ts: u64,
        modified: bool,
    ) {
        // first, we need to check the directory.
        let directory_set = directory_set_guard.get_or_create(block_id);

        // we cancel the element of this block in the directory.
        let sharer = directory_set.sharers;

        if sharer.get(cache_id).unwrap() == false {
            // Well, it is already invalid by other core.
            if parameter::ENABLE_CACHE_LINE_HISTORY {
                let his = CacheLineCoherenceHistory::global_get_block_history(block_id).unwrap();
                his.value().print_history();
                println!("Failed operation: {:?}, Cache ID: {}, Timestamp: {}, Refilled: {}, Share List: {:?}",
                    CacheOperationType::Drop, cache_id, ts, false, sharer.iter_ones().collect::<Vec<usize>>() );
            }
            assert!(false);
            return;
        }

        // we put the element back to the directory.
        directory_set.ts = ts;
        directory_set.sharers.set(cache_id, false);

        CacheLineCoherenceHistory::global_record_history(
            block_id,
            CacheOperationType::Drop,
            cache_id,
            ts,
            false,
            directory_set.sharers,
            line!(),
        );

        // Also update the writer timestamp before eviction.
        if modified {
            // keep the latest write timestamp.
            directory_set.modify_ts_before_eviction =
                if directory_set.modify_ts_before_eviction < ts {
                    ts
                } else {
                    directory_set.modify_ts_before_eviction
                };
        }

        // before releasing the lock of the directory, we need to check whether we need to place this lock to the shared cache.
        if directory_set.sharers.count_ones() == 0 {
            // we need to place this block to the shared cache.
            Statistics::global_record(
                PCache::find_cache_by_id(cache_id).0,
                EventType::SharedCacheAccess,
            );
            // NOTE: currently shared cache access is disabled.
            self.shared_cache.evict_to(block_id, ts);
            // We should also mark this one as deleted.
        }
    }

    pub fn dump_access_counter(&self) {
        // self.shared_cache.dump_access_counter();
    }


    pub fn dump_snapshot(&self, snapshot_folder: &str) {
        self.private_caches.dump_snapshot(snapshot_folder);
    }
}
