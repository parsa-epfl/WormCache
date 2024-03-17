use crate::{
    components::memory_locker::directory::SharerList,
    parameter::{self, ENABLE_STATISTICS},
};

use crate::components::debug::statistics::{EventType, Statistics};

use super::{dashmap_directory, private_cache, shared_cache, statistics};

use crate::components::mmu::AbstractMMU;
use std::cell::UnsafeCell;

mod debug_tests;
mod reverse_order_tests;

pub struct LockedMemoryHierarchy<MMU: AbstractMMU> {
    mmus: [UnsafeCell<MMU>; parameter::CORE_COUNT],

    private_caches:
        [private_cache::PrivateCache<{ parameter::PRI_CACHE_SET }, { parameter::PRI_CACHE_ASSO }>;
            parameter::CORE_COUNT],
    // In case the hardware has separate L1i and L1d, and there is no private L2, we can just add two groups of caches.
    // The logic to handle it is the same. It is equivalent that we have more cores with a single private cache.
    // There might be a way to optimize if the permission is shared. I need to think about it.
    directory: dashmap_directory::Directory,

    shared_cache: shared_cache::SharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
}

#[derive(PartialEq, Eq, Debug)]
pub enum CacheHierarchyAccessResult {
    HitInSelfPrivateCache,
    HitInOtherPrivateCache,
    MissInPrivateCache, // This entry is emitted when we see order violation, because we don't know its state in the shared cache.
    HitInSharedCache,
    Miss,
}

impl<MMU: AbstractMMU> LockedMemoryHierarchy<MMU> {
    pub fn new() -> Self {
        Self {
            mmus: std::array::from_fn(|_| UnsafeCell::new(MMU::new())),
            private_caches: std::array::from_fn(|_| private_cache::PrivateCache::new()),
            directory: dashmap_directory::Directory::new(),
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

        let private_cache = &self.private_caches[core_id as usize];
        let mut private_set = private_cache.get_set(block_id).write().unwrap(); // Thread 6

        // first, we need to check the private cache.
        let private_hit = private_set.poke_and_update(block_id, ts, is_store, is_instruction);

        if private_hit == private_cache::PrivateCachePokeResult::Hit {
            // we don't have to anything. Just return.
            return CacheHierarchyAccessResult::HitInSelfPrivateCache;
        }

        Statistics::global_record(core_id, EventType::PrivateCacheMiss);

        // Now, we go to the directory. We release the lock of the private cache.
        drop(private_set);

        // now, it is a miss. We need to check the directory.
        let mut directory_entry = self.directory.get_or_create(block_id);
        let sharers = directory_entry.sharers;

        if directory_entry.modify_ts_before_eviction > ts {
            // This means that the current operation is not ordered. (even later than the first writer)
            // There is no need to continue, because this memory operation is whatever blocked by a writer before the eviction.
            return CacheHierarchyAccessResult::MissInPrivateCache;
        }

        // if it is miss, we need to access the last level cache as well, and add it.
        if sharers.count_ones() == 0 {
            // NOTE: currently shared cache access is disabled.
            let shared_cache_result = self.shared_cache.lookup(block_id);
            let mut incoming_sharer = SharerList::ZERO;
            incoming_sharer.set(core_id as usize, true);
            directory_entry.ts = ts;
            directory_entry.sharers = incoming_sharer;

            if is_store {
                directory_entry.modify_ts_before_eviction = ts;
            }

            let mut private_set = private_cache.get_set(block_id).write().unwrap();
            let evicted = private_set.refill(block_id, ts, is_instruction, is_store);

            drop(private_set);
            drop(directory_entry);

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(core_id, evicted_line.tag, ts, evicted_line.modified);
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
        // the core itself should be also part of the acquire_list.
        acquire_list.set(core_id as usize, true);

        // Now, acquire the lock of all sets of the private cache, suggested by the directory.
        let mut acquired_sets = Vec::with_capacity(sharers.count_ones() as usize);
        for i in sharers.iter_ones() {
            acquired_sets.push((
                i as u32,
                self.private_caches[i].get_set(block_id).write().unwrap(),
            ));
        }

        // Do we have another sharer that has a write permission with a larger timestamp?
        let mut other_has_write_permission_with_late_ts = false;
        let mut other_write_ts = 0;
        let mut other_core_id = 0;
        for (replica_core_id, set) in acquired_sets.iter() {
            let line = set.poke(block_id).unwrap();
            if line.modified && line.write_ts > ts {
                other_has_write_permission_with_late_ts = true;
                if line.write_ts > other_write_ts {
                    other_write_ts = line.write_ts;
                    other_core_id = *replica_core_id as u32;
                }
            }
        }

        if other_has_write_permission_with_late_ts {
            // Well, this cache line is already touched by another core with a later timestamp.
            // Only that core should be kept.

            // Update the directory.
            let mut incoming_sharer = SharerList::ZERO;
            incoming_sharer.set(other_core_id as usize, true);
            directory_entry.ts = ts;
            directory_entry.sharers = incoming_sharer;

            for (core_id, set) in acquired_sets.iter_mut() {
                if *core_id != other_core_id {
                    set.invalidate(block_id);
                }
            }

            // Now, release the lock.
            drop(acquired_sets);
            drop(directory_entry);

            // We don't know whether this cache line should trigger a hit or miss. It definitely misses in the private cache.
            return CacheHierarchyAccessResult::MissInPrivateCache;
        }

        let evicted = if is_store {
            let mut incoming_sharer = directory_entry.sharers.clone();
            let mut set_for_refill_lock = None;

            // Here actually we can do something to tell the difference between CleanExclusive and CleanShared.
            // When checking replica, we can see the number of replica. If it is 1 and its owner is the current core, then it is CleanExclusive.
            // We can just return HitInSelfPrivateCache if the coherence protocol is MESI and the timing information is needed.

            for (replica_core_id, set) in acquired_sets.iter_mut() {
                if let Some(entry) = set.poke(block_id) {
                    if entry.ts < ts {
                        // invalid the directory entry.
                        incoming_sharer.set(*replica_core_id as usize, false);
                        // invalid the private cache entry.
                        set.invalidate(block_id);
                    } else {
                        assert!(entry.write_ts <= ts);
                        assert!(*replica_core_id != core_id);
                    }
                } else {
                    // This is the only case that we can see a the private cache does not have this block.
                    assert!(*replica_core_id == core_id);
                    incoming_sharer.set(*replica_core_id as usize, false);
                }

                if *replica_core_id == core_id {
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

            // update the directory.
            directory_entry.ts = ts;
            directory_entry.sharers = incoming_sharer;

            drop(acquired_sets);
            drop(directory_entry);

            evicted
        } else {
            // You need to find currently whether there are cores that have modified permission.
            let mut already_modified = false;
            let mut set_for_refill_lock = None;

            for (replica_core_id, set) in acquired_sets.iter_mut() {
                if let Some(entry) = set.poke(block_id) {
                    if entry.modified {
                        assert!(already_modified == false);

                        if entry.write_ts > ts {
                            // OK, this read operation is also not ordered.
                            // There is nothing we need to do.

                            drop(acquired_sets);
                            drop(directory_entry);
                            return CacheHierarchyAccessResult::MissInPrivateCache;
                        }

                        // Alright, we find the modifier of this cache line.
                        set.request_sharer(block_id, ts);
                        already_modified = true;
                    }
                }

                if *replica_core_id == core_id {
                    set_for_refill_lock = Some(set);
                }
            }

            // Then, we need to add self to the directory.
            let mut incoming_sharer = directory_entry.sharers.clone();
            incoming_sharer.set(core_id as usize, true);
            directory_entry.ts = ts;
            directory_entry.sharers = incoming_sharer;

            // We can insert the block to the private cache now.
            let set_for_refill_lock = set_for_refill_lock.unwrap();

            let evicted = set_for_refill_lock.refill(block_id, ts, is_instruction, false);
            evicted
        };

        if let Some(evicted_line) = evicted {
            self.handle_eviction(core_id, evicted_line.tag, ts, evicted_line.modified);
        }

        return CacheHierarchyAccessResult::HitInOtherPrivateCache;
    }

    pub fn handle_eviction(&self, core_id: u32, block_id: u64, ts: u64, modified: bool) {
        // first, we need to check the directory.
        let mut directory_set = self.directory.get_or_create(block_id); // Thread 5

        // we cancel the element of this block in the directory.
        let sharer = directory_set.sharers;

        if sharer.get(core_id as usize).unwrap() == false {
            // Well, it is already invalid by other core.
            assert!(false);
            return;
        }

        let mut incoming_sharer = sharer.clone();
        incoming_sharer.set(core_id as usize, false);

        // we put the element back to the directory.
        directory_set.ts = ts;
        directory_set.sharers = incoming_sharer;

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
        if incoming_sharer.count_ones() == 0 {
            // we need to place this block to the shared cache.
            Statistics::global_record(core_id, EventType::SharedCacheAccess);
            // NOTE: currently shared cache access is disabled.
            self.shared_cache.write_back(block_id, ts);
            drop(directory_set);
            // We should also mark this one as deleted.
        } else {
            drop(directory_set);
        }
    }

    pub fn dump_access_counter(&self) {
        // self.shared_cache.dump_access_counter();
    }
}
