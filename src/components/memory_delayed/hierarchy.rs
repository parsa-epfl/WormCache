use std::cell::UnsafeCell;
use std::sync::MutexGuard;

use crate::components::NoMMU;
use crate::parameter::{self, ENABLE_STATISTICS};

// use super::dashmap_directory::{SharerList, Directory};
use super::replica_directory::{DirectorySet, ReplicaDirectory};

use super::cache_line_history::{CacheLineCoherenceHistory, CacheOperationType};
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

    shared_cache: shared_cache::SharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
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
    MissInPrivateCache, // This entry is emitted when we see order violation, because we don't know its state in the shared cache.
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
            shared_cache: shared_cache::SharedCache::new(),
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
            }
            crate::components::mmu::MMUTranslationResult::MissNotCacheable(pa) => {
                // assert!(pa == reference_pa as u64);
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

        let private_cache = &self.private_caches[core_id as usize];
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

        if is_store {
            CacheLineCoherenceHistory::global_record_history(
                block_id,
                CacheOperationType::GetM,
                core_id,
                ts,
            );
        } else {
            CacheLineCoherenceHistory::global_record_history(
                block_id,
                CacheOperationType::GetR,
                core_id,
                ts,
            );
        }

        // now, it is a miss. We need to check the directory.
        // let mut directory_entry_guard = self.directory.get_or_create(block_id);
        let mut directory_set_guard = self.directory.get_set(block_id);
        // let directory_entry_guard = directory_set_guard.get_or_create(block_id);
        // let sharers = directory_entry_guard.sharers;

        // if the directory reports a miss, we need to access the last level cache as well, and add it.
        let not_exist = !directory_set_guard.exists(block_id);
        let evicted = directory_set_guard.evicted(block_id);
        if not_exist || evicted {
            if evicted {
                // Well, here we have to be serious.
                let directory_entry_guard = directory_set_guard.get_mut(block_id);
                if is_store {
                    match directory_entry_guard.get_modify(core_id, ts, true) {
                        super::replica_directory::GetModifyResult::Rejected => {
                            // No need to continue.
                            return CacheHierarchyAccessResult::MissInPrivateCache;
                        }
                        _ => {}
                    }
                } else {
                    match directory_entry_guard.get_read(core_id, ts, block_id) {
                        super::replica_directory::GetReadResult::Rejected => {
                            // No need to continue as well.
                            return CacheHierarchyAccessResult::MissInPrivateCache;
                        }
                        _ => {}
                    }
                };
            } else {
                // We just add a new entry.
                directory_set_guard.create(core_id, block_id, ts, is_store);
            }

            // Now, we can get it from LLC.

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

            let get_m_result = directory_entry_guard.get_modify(core_id, ts, true);

            let evicted = match get_m_result {
                super::replica_directory::GetModifyResult::Successful(to_invalid) => {
                    for invalid_core in to_invalid {
                        unsafe {
                            (*self.private_caches[invalid_core as usize].get()).send_message(
                                block_id,
                                ts,
                                private_cache::MessageType::Invalidate,
                                core_id,
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
                }
                super::replica_directory::GetModifyResult::SuccessfulWithSharers(to_invalid) => {
                    for invalid_core in to_invalid {
                        unsafe {
                            (*self.private_caches[invalid_core as usize].get()).send_message(
                                block_id,
                                ts,
                                private_cache::MessageType::Invalidate,
                                core_id,
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
                }
                super::replica_directory::GetModifyResult::Rejected => {
                    return CacheHierarchyAccessResult::MissInPrivateCache;
                }
            };

            // handle eviction now.
            if let Some(evicted_line) = evicted {
                self.handle_eviction(&mut directory_set_guard, core_id, evicted_line.tag, ts);
            }

            return CacheHierarchyAccessResult::HitInOtherPrivateCache;
        } else {
            let get_r_result = directory_entry_guard.get_read(core_id, ts, block_id);
            let evicted = match get_r_result {
                super::replica_directory::GetReadResult::Exclusive => private_set.refill(
                    core_id,
                    block_id,
                    ts,
                    is_instruction,
                    PrivateCacheState::CleanExclusive,
                ),
                super::replica_directory::GetReadResult::Successful => private_set.refill(
                    core_id,
                    block_id,
                    ts,
                    is_instruction,
                    PrivateCacheState::CleanShared,
                ),
                super::replica_directory::GetReadResult::SuccessfulWithMessage(owner) => {
                    unsafe {
                        (*self.private_caches[owner as usize].get()).send_message(
                            block_id,
                            ts,
                            private_cache::MessageType::CreateSharer,
                            core_id,
                        );
                    }
                    private_set.refill(
                        core_id,
                        block_id,
                        ts,
                        is_instruction,
                        PrivateCacheState::CleanShared,
                    )
                }
                super::replica_directory::GetReadResult::Rejected => {
                    return CacheHierarchyAccessResult::MissInPrivateCache;
                }
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
        let directory_entry_guard = directory_guard.get_mut(block_id);

        CacheLineCoherenceHistory::global_record_history(
            block_id,
            CacheOperationType::Drop,
            core_id,
            ts,
        );

        // we cancel the element of this block in the directory.
        match directory_entry_guard.drop(core_id, block_id) {
            super::replica_directory::DropResult::NoSharer => {
                self.shared_cache.write_back(block_id, ts);
            }
            super::replica_directory::DropResult::NewExclusive(owner) => {
                // send a message to the owner to make it exclusive.
                unsafe {
                    (*self.private_caches[owner as usize].get()).send_message(
                        block_id,
                        ts,
                        private_cache::MessageType::MakeExclusive,
                        core_id,
                    );
                }
            }
            super::replica_directory::DropResult::MoreSharers => {}
        }
    }

    pub fn get_statistics(&self, core_id: u32) -> String {
        return self.per_core_statistics[core_id as usize].being_printed(core_id);
    }
}

#[cfg(test)]
mod reverse_order_tests;

#[cfg(test)]
mod debug_tests;
