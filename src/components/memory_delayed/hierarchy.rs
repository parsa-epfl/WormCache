use std::cell::UnsafeCell;
use std::sync::MutexGuard;

use crate::components::NoMMU;
use crate::parameter::{self, ENABLE_STATISTICS};

// use super::dashmap_directory::{SharerList, Directory};
use super::replica_directory::{DirectorySet, ReplicaDirectory};

use super::private_cache::PrivateCacheState;
use super::statistics;
use super::{private_cache, shared_cache};

use crate::arch::AArch64;
use crate::components::mmu::AbstractMMU;
use crate::components::mmu::MemoryManagementUnit;

pub struct DelayedMemoryHierarchy<MMU: AbstractMMU> {
    mmus: [MMU; parameter::CORE_COUNT],

    private_caches: [UnsafeCell<
        private_cache::PrivateCache<{ parameter::PRI_CACHE_SET }, { parameter::PRI_CACHE_ASSO }>,
    >; parameter::CORE_COUNT],

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

pub type PluginDelayedMemoryHierarchy = DelayedMemoryHierarchy<
    MemoryManagementUnit<AArch64, { parameter::TLB_ASSO }, { parameter::TLB_SET }>,
>;

pub type TestingDelayedMemoryHierarchy = DelayedMemoryHierarchy<NoMMU>;

#[derive(Debug, PartialEq, Eq)]
pub enum CacheHierarchyAccessResult {
    HitInSelfPrivateCache,
    HitInOtherPrivateCache,
    HitInSharedCache,
    Miss,
}

impl<MMU: AbstractMMU> DelayedMemoryHierarchy<MMU> {
    pub fn new() -> Self {
        Self {
            mmus: std::array::from_fn(|_| MMU::new()),
            private_caches: std::array::from_fn(|_| {
                UnsafeCell::new(private_cache::PrivateCache::new())
            }),
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

    pub fn access_memory_with_va_and_pa(
        &mut self,
        core_id: u32,
        va: u64,
        reference_pa: u64,
        ts: u64,
        is_store: bool,
        is_instruction: bool,
    ) {
        let translation = self.mmus[core_id as usize].translate_and_refill(va, ts);

        match translation {
            crate::components::mmu::MMUTranslationResult::Hit(pa) => {
                assert!(pa == reference_pa);
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
                assert!(pa == reference_pa as u64);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_memory_pblock_id(core_id, block_id, ts, is_store, is_instruction);
            }
            crate::components::mmu::MMUTranslationResult::MissNotCacheable(pa) => {
                assert!(pa == reference_pa as u64);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
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

        let private_cache = &private_caches[core_id as usize];
        let private_set = unsafe { (*private_cache.get()).get_set(block_id) };

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
            let shared_cache_result = self.shared_cache.lookup(block_id);

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

            if is_store {
                directory_entry_guard.get_modify(core_id, ts);
            } else {
                directory_entry_guard.get_read(core_id, ts);
            }

            if shared_cache_result {
                return CacheHierarchyAccessResult::HitInSharedCache;
            } else {
                return CacheHierarchyAccessResult::Miss;
            }
        }

        let directory_entry_guard = directory_set_guard.get_mut(block_id);

        // OK, now this block is provided by another core. We need to check whether we can access it.
        if is_store {
            // We need to invalid other cores' cache line.

            let get_m_result = directory_entry_guard.get_modify(core_id, ts);

            let evicted = match get_m_result {
                super::replica_directory::GetModifyResult::Successful(to_invalid) => {
                    for invalid_core in to_invalid {
                        unsafe {
                            (*self.private_caches[invalid_core as usize].get()).send_message(
                                block_id,
                                ts,
                                private_cache::MessageType::Invalidate,
                            );
                        }
                    }

                    private_set.refill(
                        core_id,
                        block_id,
                        ts,
                        is_instruction,
                        PrivateCacheState::DirtyExclusive,
                    )
                },
                super::replica_directory::GetModifyResult::SuccessfulWithSharers(to_invalid) => {
                    for invalid_core in to_invalid {
                        unsafe {
                            (*self.private_caches[invalid_core as usize].get()).send_message(
                                block_id,
                                ts,
                                private_cache::MessageType::Invalidate,
                            );
                        }
                    }

                    private_set.refill(
                        core_id,
                        block_id,
                        ts,
                        is_instruction,
                        PrivateCacheState::DirtyShared,
                    )
                },
                super::replica_directory::GetModifyResult::Rejected => {
                    None
                },
            };

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(&mut directory_set_guard, core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        } else {
            let get_r_result = directory_entry_guard.get_read(core_id, ts);
            let evicted = match get_r_result {
                super::replica_directory::GetReadResult::Exclusive => {
                    private_set.refill(
                        core_id,
                        block_id,
                        ts,
                        is_instruction,
                        PrivateCacheState::CleanExclusive,
                    )
                },
                super::replica_directory::GetReadResult::Successful => {
                    private_set.refill(
                        core_id,
                        block_id,
                        ts,
                        is_instruction,
                        PrivateCacheState::CleanShared,
                    )
                },
                super::replica_directory::GetReadResult::SuccessfulWithMessage(owner) => {
                    unsafe {
                        (*self.private_caches[owner as usize].get()).send_message(
                            block_id,
                            ts,
                            private_cache::MessageType::CreateSharer,
                        );
                    }
                    private_set.refill(
                        core_id,
                        block_id,
                        ts,
                        is_instruction,
                        PrivateCacheState::CleanShared,
                    )
                },
                super::replica_directory::GetReadResult::Rejected => {
                    None
                },
            };

            if let Some(evicted_line) = evicted {
                self.handle_eviction(&mut directory_set_guard, core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        }
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
        if !directory_entry_guard.drop(core_id) {
            self.shared_cache.allocate(block_id, ts);
            directory_guard.invalidate(block_id);
        }
    }

    pub fn get_statistics(&self, core_id: u32) -> String {
        return self.per_core_statistics[core_id as usize].being_printed(core_id);
    }
}

#[cfg(test)]
mod tests;
