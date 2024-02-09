use crate::parameter;

use super::directory::SharerList;

use super::private_cache::PrivateCacheState;
use super::{directory, private_cache, shared_cache};

use crate::arch::AArch64;
use crate::components::mmu::AbstractMMU;
use crate::components::mmu::MemoryManagementUnit;
use std::cell::UnsafeCell;

pub struct LockedMemoryHierarchy {
    mmus: [UnsafeCell<
        MemoryManagementUnit<AArch64, { parameter::TLB_ASSO }, { parameter::TLB_SET }>,
    >; parameter::CORE_COUNT],

    private_caches:
        [private_cache::PrivateCache<{ parameter::PRI_CACHE_SET }, { parameter::PRI_CACHE_ASSO }>;
            parameter::CORE_COUNT],

    directory: directory::Directory,

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
            mmus: std::array::from_fn(|_| UnsafeCell::new(MemoryManagementUnit::new())),
            private_caches: std::array::from_fn(|_| private_cache::PrivateCache::new()),
            directory: directory::Directory::new(),
            shared_cache: shared_cache::ExclusiveSharedCache::new(),
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
            crate::components::mmu::MMUTranslationResult::Miss(pa, walk_trace) => {
                // replay the trace.
                for trace_pa in walk_trace {
                    if trace_pa == u64::MAX {
                        break;
                    }
                    let block_id = trace_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                    self.access_memory_pblock_id(core_id, block_id, ts, false, false);
                }
                let block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, is_store, is_instruction);
            }
            crate::components::mmu::MMUTranslationResult::MissNotCacheable(pa) => {
                let block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
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
        let private_cache = &self.private_caches[core_id as usize];
        let private_set = unsafe { private_cache.get_set(block_id).get().as_mut().unwrap() };

        // first, we need to check the private cache.
        let private_hit = private_set.poke_and_update(block_id, ts, is_store, is_instruction);

        if private_hit {
            // we don't have to anything. Just return.
            return CacheHierarchyAccessResult::HitInSelfPrivateCache;
        }

        // now, it is a miss. We need to check the directory.
        let mut directory_entry = self.directory.get_or_create(block_id);
        let sharers = directory_entry.sharers;

        // if the directory reports a miss, we need to access the last level cache as well, and add it.
        if sharers.count_ones() == 0 {
            // NOTE: currently we ignore the LLC. 
            // let shared_cache_result = self.shared_cache.lookup(block_id);
            let shared_cache_result = false;
            let mut incoming_sharer = SharerList::ZERO;
            incoming_sharer.set(core_id as usize, true);
            directory_entry.ts = ts;
            directory_entry.sharers = incoming_sharer;

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

            drop(directory_entry);

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
                if *sharers.get(i).unwrap() {
                    // invalidate the cache line.
                    let other_private_cache = &self.private_caches[i];
                    let other_private_set = unsafe {
                        other_private_cache
                            .get_set(block_id)
                            .get()
                            .as_mut()
                            .unwrap()
                    };
                    other_private_set.send_message(
                        block_id,
                        ts,
                        private_cache::MessageType::Invalidate,
                    );
                }
            }

            drop(directory_entry);

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        }

        if sharers.count_ones() == 1 {
            // OK, only one guy has the permission. We need to send a low upgrade permission later.
            let mut incoming_sharer = sharers.clone();
            incoming_sharer.set(core_id as usize, true);

            let owner = sharers.first_one().unwrap();
            // send upgrade permission to the owner.
            let other_private_cache = &self.private_caches[owner];
            let other_private_set = unsafe {
                other_private_cache
                    .get_set(block_id)
                    .get()
                    .as_mut()
                    .unwrap()
            };
            other_private_set.send_message(block_id, ts, private_cache::MessageType::CreateSharer);

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

            drop(directory_entry);

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        }

        // Now, we just need to add ourself to the sharer list. This block must be the shared one.
        let mut incoming_sharer = sharers.clone();
        incoming_sharer.set(core_id as usize, true);
        directory_entry.ts = ts;
        directory_entry.sharers = incoming_sharer;

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

        drop(directory_entry);

        // handle eviction now.
        if let Some(evicted_line) = evicted {
            self.handle_eviction(core_id, evicted_line.tag, ts);
        }

        return CacheHierarchyAccessResult::HitInOtherPrivateCache;
    }

    pub fn handle_eviction(&self, core_id: u32, block_id: u64, ts: u64) {
        // before calling this function, make sure we don't have any locks of the directory or the shared cache.

        // first, we need to check the directory.
        let mut directory_entry = self.directory.get_or_create(block_id);

        // we cancel the element of this block in the directory.
        let sharers = directory_entry.sharers;

        assert!(sharers.get(core_id as usize).unwrap());

        let mut incoming_sharer = sharers.clone();
        incoming_sharer.set(core_id as usize, false);

        // we put the element back to the directory.
        directory_entry.ts = ts;
        directory_entry.sharers = incoming_sharer;

        // before releasing the lock of the directory, we need to check whether we need to place this lock to the shared cache.
        if incoming_sharer.count_ones() == 0 {
            // we need to place this block to the shared cache.
            // NOTE: currently, we ignore the LLC.
            // self.shared_cache.allocate(block_id, ts);
            drop(directory_entry);
            self.directory.mark_as_useless(block_id);
        } else {
            drop(directory_entry);
        }
    }
}
