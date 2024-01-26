// This module defines the basic memory hierarchies using fine-grained locks.
// It contains the same logical memory hierarchy as the `memory`, but uses locks for shared communication.
// - TLB, which is private.
// - Private caches, with set locks.
// - Directory, with set locks.
// - Shared caches, with set locks.

pub mod directory;
mod private_cache;
pub mod shared_cache;

use crate::{
    components::memory_locker::{directory::SharerList, private_cache::PrivateCacheState},
    parameter,
};

struct LockedMemoryHierarchy {
    private_caches:
        [private_cache::PrivateCache<{ parameter::PRI_CACHE_SET }, { parameter::PRI_CACHE_ASSO }>;
            parameter::CORE_COUNT],

    directory: directory::Directory<
        { parameter::PRI_CACHE_SET * 8 }, // over-provisioned by 8x to reduce the conflict.
    >,

    shared_cache: shared_cache::ExclusiveSharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
    >,
}

pub enum CacheHierarchyAccessResult {
    HitInSelfPrivateCache,
    HitInOtherPrivateCache,
    HitInSharedCache,
    Miss,
}

impl LockedMemoryHierarchy {
    pub fn new() -> Self {
        Self {
            private_caches: std::array::from_fn(|_| private_cache::PrivateCache::new()),
            directory: directory::Directory::new(),
            shared_cache: shared_cache::ExclusiveSharedCache::new(),
        }
    }

    pub fn access_memory(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_store: bool,
        is_instruction: bool,
    ) -> CacheHierarchyAccessResult {
        let private_cache = &self.private_caches[core_id as usize];
        let mut private_set = private_cache.get_set(block_id).write().unwrap();

        // first, we need to check the private cache.
        let private_hit = private_set.pook_and_update(block_id, ts, is_store, is_instruction);

        if private_hit {
            // we don't have to anything. Just return.
            return CacheHierarchyAccessResult::HitInSelfPrivateCache;
        }

        // now, it is a miss. We need to check the directory.
        let mut directory_set = self.directory.get_set(block_id).write().unwrap();
        let directory_result = directory_set.peek(block_id);

        // if it is miss, we need to access the last level cache as well, and add it.
        if directory_result.count_ones() == 0 {
            let shared_cache_result = self.shared_cache.lookup(block_id);
            let mut incoming_sharer = SharerList::ZERO;
            incoming_sharer.set(core_id as usize, true);
            directory_set.write(block_id, ts, incoming_sharer);
            let evicted = private_set.refill(
                block_id,
                ts,
                is_instruction,
                if is_store {
                    PrivateCacheState::DirtyExclusive
                } else {
                    PrivateCacheState::CleanExclusive
                },
            );

            drop(directory_set);
            drop(private_set);

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(core_id, evicted_line.tag, ts);
            }

            if shared_cache_result {
                return CacheHierarchyAccessResult::HitInSharedCache;
            } else {
                return CacheHierarchyAccessResult::Miss;
            }
        }

        // OK, now this block is provided by another core. We need to check whether we can access it.
        if is_store {
            // We need to invalid other cores' cache line.
            // First, we insert the result to our own private cache.
            let evicted = private_set.refill(
                block_id,
                ts,
                is_instruction,
                PrivateCacheState::DirtyExclusive,
            );
            // Second, we go over the sharer list, and invalidate them.
            for i in 0..parameter::CORE_COUNT {
                if *directory_result.get(i).unwrap() {
                    if i == core_id as usize {
                        panic!("Invalidating myself???")
                    }
                    // invalidate the cache line.
                    let other_private_cache = &self.private_caches[i];
                    let mut other_private_set =
                        other_private_cache.get_set(block_id).write().unwrap();
                    other_private_set.invalidate(block_id);
                }
            }

            drop(directory_set);
            drop(private_set);

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        }

        if directory_result.count_ones() == 1 {
            // OK, only one guy has the permission. We need to send a low upgrade permission later.
            let mut incoming_sharer = directory_result.clone();
            incoming_sharer.set(core_id as usize, true);

            let owner = directory_result.first_one().unwrap();
            // send upgrade permission to the owner.
            let other_private_cache = &self.private_caches[owner];
            let mut other_private_set = other_private_cache.get_set(block_id).write().unwrap();
            other_private_set.request_sharer(block_id, ts);

            // then, we can consider how to refill the cache line.
            let evicted = private_set.refill(
                block_id,
                ts,
                is_instruction,
                if is_store {
                    PrivateCacheState::DirtyExclusive
                } else {
                    PrivateCacheState::CleanExclusive
                },
            );

            drop(directory_set);
            drop(private_set);

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        }

        // Now, we just need to add ourself to the sharer list. This block must be the shared one.
        let mut incoming_sharer = directory_result.clone();
        incoming_sharer.set(core_id as usize, true);
        directory_set.write(block_id, ts, incoming_sharer);

        // then, we can consider how to refill the cache line.
        let evicted = private_set.refill(
            block_id,
            ts,
            is_instruction,
            if is_store {
                PrivateCacheState::DirtyShared
            } else {
                PrivateCacheState::CleanShared
            },
        );

        drop(directory_set);
        drop(private_set);

        // handle eviction now.
        if let Some(evicted_line) = evicted {
            self.handle_eviction(core_id, evicted_line.tag, ts);
        }

        return CacheHierarchyAccessResult::HitInOtherPrivateCache;
    }

    pub fn handle_eviction(&self, core_id: u32, block_id: u64, ts: u64) {
        // before calling this function, make sure we don't have any locks of the directory or the shared cache.

        // first, we need to check the directory.
        let mut directory_set = self.directory.get_set(block_id).write().unwrap();

        // we cancel the element of this block in the directory. 
        let sharer = directory_set.peek(block_id);
        
        assert!(sharer.get(core_id as usize).unwrap());

        let mut incoming_sharer = sharer.clone();
        incoming_sharer.set(core_id as usize, false);

        // we put the element back to the directory.
        directory_set.write(block_id, ts, incoming_sharer);

        // before releasing the lock of the directory, we need to check whether we need to place this lock to the shared cache.
        if incoming_sharer.count_ones() == 0 {
            // we need to place this block to the shared cache.
            self.shared_cache.allocate(block_id, ts);
            drop(directory_set);
        } else {
            drop(directory_set);
        }
    }
}
