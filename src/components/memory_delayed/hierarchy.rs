use std::sync::MutexGuard;

use crate::parameter::{self, ENABLE_STATISTICS};

// use super::dashmap_directory::{SharerList, Directory};
use super::replica_directory::{DirectorySet, ReplicaDirectory, SharerList};

use super::private_cache::PrivateCacheState;
use super::statistics;
use super::{private_cache, shared_cache};

use crate::arch::AArch64;
use crate::components::mmu::AbstractMMU;
use crate::components::mmu::MemoryManagementUnit;

pub struct DelayedMemoryHierarchy {
    mmus: [MemoryManagementUnit<AArch64, { parameter::TLB_ASSO }, { parameter::TLB_SET }>;
        parameter::CORE_COUNT],

    private_caches:
        [private_cache::PrivateCache<{ parameter::PRI_CACHE_SET }, { parameter::PRI_CACHE_ASSO }>;
            parameter::CORE_COUNT],

    directory: ReplicaDirectory<
        { parameter::PRI_CACHE_SET },
        { parameter::PRI_CACHE_ASSO * parameter::CORE_COUNT },
    >,

    shared_cache: shared_cache::ExclusiveSharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
    >,

    per_core_statistics: [statistics::PerCoreStatistics; parameter::CORE_COUNT],
}

pub enum CacheHierarchyAccessResult {
    HitInSelfPrivateCache,
    HitInOtherPrivateCache,
    HitInSharedCache,
    Miss,
}

impl DelayedMemoryHierarchy {
    pub fn new() -> Self {
        Self {
            mmus: std::array::from_fn(|_| MemoryManagementUnit::new()),
            private_caches: std::array::from_fn(|_| private_cache::PrivateCache::new()),
            directory: ReplicaDirectory::new(),
            shared_cache: shared_cache::ExclusiveSharedCache::new(),
            per_core_statistics: std::array::from_fn(|_| statistics::PerCoreStatistics::new()),
        }
    }

    pub fn access_memory_with_va(
        &mut self,
        core_id: u32,
        va: u64,
        ts: u64,
        is_store: bool,
        is_instruction: bool,
    ) {
        let translation = self.mmus[core_id as usize].translate_and_refill(va, ts);

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
        &mut self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_store: bool,
        is_instruction: bool,
    ) -> CacheHierarchyAccessResult {
        if ENABLE_STATISTICS {
            self.per_core_statistics[core_id as usize].total_mem += 1;
        }

        let private_caches = &mut self.private_caches;

        let private_cache = &mut private_caches[core_id as usize];
        let private_set = private_cache.get_set(block_id);

        // first, we need to check the private cache.
        let private_hit = private_set.poke_and_update(block_id, ts, is_store, is_instruction);

        if private_hit {
            // we don't have to anything. Just return.
            return CacheHierarchyAccessResult::HitInSelfPrivateCache;
        }

        if ENABLE_STATISTICS {
            self.per_core_statistics[core_id as usize].private_cache_miss += 1;
        }

        // now, it is a miss. We need to check the directory.
        // let mut directory_entry_guard = self.directory.get_or_create(block_id);
        let mut directory_set_guard = self.directory.get_set(block_id);
        // let directory_entry_guard = directory_set_guard.get_or_create(block_id);
        // let sharers = directory_entry_guard.sharers;

        // if the directory reports a miss, we need to access the last level cache as well, and add it.
        if !directory_set_guard.exists(block_id) {
            // NOTE: currently we ignore the LLC.
            // let shared_cache_result = self.shared_cache.lookup(block_id);

            let evicted = private_set.refill(
                core_id,
                block_id,
                ts,
                is_instruction,
                if is_store {
                    PrivateCacheState::DirtyExclusive
                } else {
                    PrivateCacheState::CleanExclusive
                },
            );

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(&mut directory_set_guard, core_id, evicted_line.tag, ts);
            }

            let directory_entry_guard = directory_set_guard.create(block_id);

            let shared_cache_result = false;
            let mut incoming_sharer = SharerList::ZERO;
            incoming_sharer.set(core_id as usize, true);
            directory_entry_guard.ts = ts;
            directory_entry_guard.sharers = incoming_sharer;

            if shared_cache_result {
                return CacheHierarchyAccessResult::HitInSharedCache;
            } else {
                return CacheHierarchyAccessResult::Miss;
            }
        }

        let directory_entry_guard = directory_set_guard.get_mut(block_id);
        let sharers = directory_entry_guard.sharers;

        // OK, now this block is provided by another core. We need to check whether we can access it.
        if is_store {
            // We need to invalid other cores' cache line.
            // First, we insert the result to our own private cache.
            let evicted = private_set.refill(
                core_id,
                block_id,
                ts,
                is_instruction,
                PrivateCacheState::DirtyExclusive,
            );
            // Second, we go over the sharer list, and invalidate them.
            for i in 0..parameter::CORE_COUNT {
                if *sharers.get(i).unwrap() && i != core_id as usize {
                    // invalidate the cache line.
                    let other_private_cache = &mut self.private_caches[i];
                    let other_private_set = other_private_cache.get_set(block_id);
                    other_private_set.send_message(
                        block_id,
                        ts,
                        private_cache::MessageType::Invalidate,
                    );
                }
            }

            let mut exclusive_sharer = SharerList::ZERO;
            exclusive_sharer.set(core_id as usize, true);
            directory_entry_guard.sharers = exclusive_sharer;

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(&mut directory_set_guard, core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        }

        if sharers.count_ones() == 1 {
            // First, we insert the result to our own private cache.
            let evicted = private_set.refill(
                core_id,
                block_id,
                ts,
                is_instruction,
                PrivateCacheState::CleanShared,
            );

            // Then, we handle the coherence message.
            let mut incoming_sharer = sharers.clone();
            incoming_sharer.set(core_id as usize, true);
            directory_entry_guard.sharers = incoming_sharer;

            let owner = sharers.first_one().unwrap();
            // send upgrade permission to the owner.
            self.private_caches[owner].send_message(
                block_id,
                ts,
                private_cache::MessageType::CreateSharer,
            );

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(&mut directory_set_guard, core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        }

        // Now, we just need to add ourself to the sharer list. This block must be the shared one.
        let mut incoming_sharer = sharers.clone();
        incoming_sharer.set(core_id as usize, true);
        directory_entry_guard.ts = ts;
        directory_entry_guard.sharers = incoming_sharer;

        // then, we can consider how to refill the cache line.
        let evicted = private_set.refill(
            core_id,
            block_id,
            ts,
            is_instruction,
            if is_store {
                PrivateCacheState::DirtyShared
            } else {
                PrivateCacheState::CleanShared
            },
        );

        // handle eviction now.
        if let Some(evicted_line) = evicted {
            self.handle_eviction(&mut directory_set_guard, core_id, evicted_line.tag, ts);
        }

        return CacheHierarchyAccessResult::HitInOtherPrivateCache;
    }

    pub fn handle_eviction(
        &self,
        directory_guard: &mut MutexGuard<
            '_,
            DirectorySet<{ parameter::PRI_CACHE_ASSO * parameter::CORE_COUNT }>,
        >,
        core_id: u32,
        block_id: u64,
        ts: u64,
    ) {
        // first, we need to check the directory.
        // let mut directory_entry_guard = self.directory.get_or_create(block_id);
        let directory_entry_guard = directory_guard.get_mut(block_id);

        // we cancel the element of this block in the directory.
        let sharers = directory_entry_guard.sharers;

        let mut incoming_sharer = sharers.clone();
        incoming_sharer.set(core_id as usize, false);

        // we put the element back to the directory.
        directory_entry_guard.ts = ts;
        directory_entry_guard.sharers = incoming_sharer;

        // before releasing the lock of the directory, we need to check whether we need to place this lock to the shared cache.
        if incoming_sharer.count_ones() == 0 {
            // we need to place this block to the shared cache.
            // NOTE: currently, we ignore the LLC.
            // self.shared_cache.allocate(block_id, ts);
            directory_guard.invalidate(block_id);
        }
    }

    pub fn get_statistics(&self, core_id: u32) -> String {
        return self.per_core_statistics[core_id as usize].being_printed(core_id);
    }
}
