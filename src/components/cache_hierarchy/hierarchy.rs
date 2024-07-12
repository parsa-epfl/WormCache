use crate::components::cache_hierarchy::shared_cache::{
    SharedCache, SharedCacheLookupResult, VtsViolationResult,
};
use crate::parameter;
use crate::parameter::{ADJACENT_LINE_PREFETCHING, ENABLE_CACHE_LINE_HISTORY};

use crate::components::debug::statistics::{EventType, Statistics};

use crate::components::debug::cache_line_history::{CacheLineCoherenceHistory, CacheOperationType};

use super::directory::DirectorySet;
use super::{
    directory,
    private_cache::{self, PrivateCaches},
};

use hdrhistogram::serialization::Serializer;

use crate::components::mmu::AbstractMMU;
use hdrhistogram::Histogram;
use std::cell::UnsafeCell;
use std::fs::File;
use std::io::Write;
use std::ops::DerefMut;

#[cfg(test)]
mod debug_tests;
#[cfg(test)]
mod harvard_reverse_order_tests;
#[cfg(test)]
mod harvard_tests;
#[cfg(test)]
mod reverse_order_tests;
#[cfg(test)]
mod virtual_timestamp;

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
    with_statistics: bool,

    quantum_size: u64,
    vts_violation_distribution: Option<[UnsafeCell<Histogram<u64>>; parameter::CORE_COUNT]>,
}

#[derive(Debug, PartialEq, Clone)]
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
    Unknown, // This entry is emitted when a memory access arrives late but with a smaller timestamp than a previous write operation. It is unknown because its previous state is not clear.
}

// This function identify the memory instruction that can be influenced by the imperfect load generator.
// Their traffic will be recorded specially.
fn is_special_memory_access(pc: u64) -> bool {
    const SPECIAL_PC: [u64; 4] = [
        0xffff_8000_089f_a510,
        0xffff_8000_089f_a4f0,
        0xffff_8000_089f_a520,
        0xffff_8000_089f_a500,
    ];

    return SPECIAL_PC.contains(&pc);
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
    pub fn new(with_statistics: bool, quantum_size: u64) -> Self {
        Self {
            mmus: std::array::from_fn(|_| UnsafeCell::new(MMU::new())),
            private_caches: PCache::new(),
            directory: directory::Directory::new(),
            shared_cache: SCache::new(),
            with_statistics,
            quantum_size,
            vts_violation_distribution: if quantum_size > 1 {
                Some(std::array::from_fn(|_| {
                    UnsafeCell::new(Histogram::new_with_max(quantum_size, 3).unwrap())
                }))
            } else {
                None
            },
        }
    }

    pub fn access_memory_with_va(
        &self,
        core_id: u32,
        va: u64,
        ts: u64,
        is_store: bool,
        is_instruction: bool,
        v_ts: u64, // the timestamp of this instruction as if each instruction takes 1 ns.
        instruction_va_pc: u64,
    ) -> CacheHierarchyAccessResult {
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

        let is_os = (va >> 63) == 1;

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
                let res = self.access_memory_pblock_id(
                    core_id,
                    block_id,
                    ts,
                    v_ts,
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
                        ts,
                        v_ts,
                        prefetch_access_type,
                        is_os,
                        instruction_va_pc,
                    );
                };
                res
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
                        v_ts,
                        CacheAccessType::PageWalkRead,
                        false, // Page walk is not OS.
                        instruction_va_pc,
                    );
                }
                let block_id = paddr >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                let res = self.access_memory_pblock_id(
                    core_id,
                    block_id,
                    ts,
                    v_ts,
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
                        ts,
                        v_ts,
                        prefetch_access_type,
                        is_os,
                        instruction_va_pc,
                    );
                }

                if self.with_statistics {
                    Statistics::global_record(core_id, EventType::TLBMiss, is_os);
                    if is_instruction {
                        Statistics::global_record(
                            core_id,
                            EventType::TLBMissDueToInstruction,
                            is_os,
                        );
                    } else {
                        Statistics::global_record(core_id, EventType::TLBMissDueToData, is_os);
                    }
                }

                res
            }

            crate::components::mmu::MMUTranslationResult::MissNotCacheable(pa) => {
                let block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                let res = self.access_memory_pblock_id(
                    core_id,
                    block_id,
                    ts,
                    v_ts,
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
                        ts,
                        v_ts,
                        prefetch_access_type,
                        is_os,
                        instruction_va_pc,
                    );
                }

                res
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
        v_ts: u64, // the timestamp of this instruction as if each instruction takes 1 ns.
        instruction_va_pc: u64,
    ) -> CacheHierarchyAccessResult {
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

        let is_os = (va >> 63) == 1;

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
                let res = self.access_memory_pblock_id(
                    core_id,
                    block_id,
                    ts,
                    v_ts,
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
                        ts,
                        v_ts,
                        prefetch_access_type,
                        is_os,
                        instruction_va_pc,
                    );
                }

                res
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
                        v_ts,
                        CacheAccessType::PageWalkRead,
                        false,
                        instruction_va_pc,
                    );
                }
                // assert!(pa == reference_pa as u64);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                let res = self.access_memory_pblock_id(
                    core_id,
                    block_id,
                    ts,
                    v_ts,
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
                        ts,
                        v_ts,
                        prefetch_access_type,
                        is_os,
                        instruction_va_pc,
                    );
                }

                if self.with_statistics {
                    Statistics::global_record(core_id, EventType::TLBMiss, is_os);

                    if is_instruction {
                        Statistics::global_record(
                            core_id,
                            EventType::TLBMissDueToInstruction,
                            is_os,
                        );
                    } else {
                        Statistics::global_record(core_id, EventType::TLBMissDueToData, is_os);
                    }
                }

                res
            }
            crate::components::mmu::MMUTranslationResult::MissNotCacheable(_pa) => {
                // assert!(pa == reference_pa as u64);
                let block_id = reference_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                let res = self.access_memory_pblock_id(
                    core_id,
                    block_id,
                    ts,
                    v_ts,
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
                        ts,
                        v_ts,
                        prefetch_access_type,
                        is_os,
                        instruction_va_pc,
                    );
                }

                res
            }
        }
    }

    // This function is only for debugging.
    pub fn access_memory_pblock_id_with_the_same_ts_and_vts(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        access_type: CacheAccessType,
    ) -> CacheHierarchyAccessResult {
        self.access_memory_pblock_id(core_id, block_id, ts, ts, access_type, false, 0)
    }

    pub fn access_memory_pblock_id(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        access_type: CacheAccessType,
        is_os: bool,
        instruction_va_pc: u64,
    ) -> CacheHierarchyAccessResult {
        let is_prefetch = access_type == CacheAccessType::PrefetchRead
            || access_type == CacheAccessType::PrefetchWrite;

        let is_instruction = access_type == CacheAccessType::InstructionFetch;
        let is_store = access_type == CacheAccessType::DataWrite;
        let is_page_walk = access_type == CacheAccessType::PageWalkRead;

        let is_special_memory_instruction =
            is_special_memory_access(instruction_va_pc) && !is_instruction;

        if !is_prefetch && self.with_statistics {
            Statistics::global_record(core_id, EventType::MemoryAccess, is_os);
            if is_instruction {
                Statistics::global_record(core_id, EventType::InstructionAccess, is_os);
            } else {
                Statistics::global_record(core_id, EventType::DataAccess, is_os);
            }

            if is_special_memory_instruction {
                Statistics::global_record(
                    core_id,
                    EventType::SpecialMemoryInstructionAccess,
                    is_os,
                );
            }
        }

        // first, we need to check the private cache.
        let private_hit = self.private_caches.poke_and_update(
            core_id,
            block_id,
            ts,
            v_ts,
            is_instruction,
            is_store,
        );

        if private_hit == private_cache::PrivateCachePokeResult::Hit {
            // we don't have to anything. Just return.
            return CacheHierarchyAccessResult::HitInSelfPrivateCache;
        }

        let evicted_slot = match private_hit {
            private_cache::PrivateCachePokeResult::Hit => unreachable!(),
            private_cache::PrivateCachePokeResult::Miss(ref slot) => slot.clone(),
            private_cache::PrivateCachePokeResult::PermissionViolation(ref slot) => slot.clone(),
        };

        // Alright, we may need to get another directory entry of the eviction.
        // This entry may bot be used, because other entry in the same set can be evicted. But we need to get it ahead of time to avoid deadlock.
        let (mut miss_directory_set_guard, evict_directory) = {
            match evicted_slot {
                private_cache::EvictedSlot::Valid(_, potential_evicted_id) => {
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

        if sharers.count_ones() == 0
            && miss_directory_guard.recent_writer_vts > v_ts
            && self.with_statistics
        {
            // this cache line is evicted and previously is written. This is definitely a order violation.
            Statistics::global_record(core_id, EventType::PrivateCacheVTsOrderViolation, is_os);
        }

        if PRECISE_COHERENCE_RECONSTRUCTION
            && sharers.count_ones() == 0
            && miss_directory_guard.recent_writer_ts > ts
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

            // Well, this is not very accurate. The truth is that we don't know whether this is a miss or hit,
            // because the history has been cleaned up by an earlier writer.
            if !is_prefetch && self.with_statistics {
                Statistics::global_record(core_id, EventType::UnknownPrivateCacheMisses, is_os);
                // accordingly, we don't know whether this access would have cause a shared cache miss.
                Statistics::global_record(core_id, EventType::UnknownSharedCacheMisses, is_os);
            }

            return CacheHierarchyAccessResult::Unknown;
        }

        // if it is miss, we need to access the last level cache as well, and add it.
        if sharers.count_ones() == 0 {
            let shared_cache_result = if FILL_SCACHE_ON_FILLING_PCACHE {
                // here we take the ownership of the cache line from the shared cache to the private cache.
                // So abandon_dirty is true.
                // We also don't need to write through to the LLC, so the is_store is false.
                let lookup_result = self.shared_cache.lookup_and_insert_on_miss(
                    core_id,
                    block_id,
                    ts,
                    v_ts,
                    true,
                    false,
                    true,
                    access_type.clone(),
                    is_os,
                );

                match lookup_result.1 {
                    VtsViolationResult::Violataed(time_diff) => {
                        // This access must come from the same quantum.
                        self.handle_vts_violation(core_id, v_ts, time_diff, true, is_os)
                    }
                    VtsViolationResult::NotViolated => {}
                };

                match lookup_result.0 {
                    SharedCacheLookupResult::Hit(is_dirty) => Some(is_dirty),
                    SharedCacheLookupResult::Miss => None,
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
                let lookup_result = self.shared_cache.lookup(
                    core_id,
                    block_id,
                    ts,
                    v_ts,
                    true,
                    access_type.clone(),
                    is_os,
                );

                match lookup_result.1 {
                    VtsViolationResult::Violataed(time_diff) => {
                        self.handle_vts_violation(core_id, v_ts, time_diff, true, is_os)
                    }
                    VtsViolationResult::NotViolated => {}
                };

                match lookup_result.0 {
                    SharedCacheLookupResult::Hit(is_dirty) => Some(is_dirty),
                    SharedCacheLookupResult::Miss => None,
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

            if is_store {
                miss_directory_guard.recent_writer_ts = ts;
                miss_directory_guard.recent_writer_vts = v_ts;
            }

            let modified = is_store;

            let writable = if !parameter::ENABLE_EXCLUSIVE_CACHE_STATE {
                modified
            } else {
                !is_instruction && !is_page_walk
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
                v_ts,
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
                    v_ts,
                    evicted_line_is_modified,
                );
            }

            if !is_prefetch && self.with_statistics {
                // Here it is a miss in the private cache.
                Statistics::global_record(core_id, EventType::PrivateCacheMiss, is_os);

                if is_special_memory_instruction {
                    Statistics::global_record(
                        core_id,
                        EventType::SpecialMemoryInstructionPrivateCacheMiss,
                        is_os,
                    );
                }

                if is_instruction {
                    Statistics::global_record(core_id, EventType::PrivateICacheMiss, is_os);
                } else if is_page_walk {
                    Statistics::global_record(core_id, EventType::PrivateCacheMissDueToPTW, is_os);
                } else {
                    Statistics::global_record(core_id, EventType::PrivateDCacheMiss, is_os);
                }

                Statistics::global_record(core_id, EventType::SharedCacheAccess, is_os);
            }

            if shared_cache_result.is_some() {
                return CacheHierarchyAccessResult::HitInSharedCache;
            } else {
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

                if is_special_memory_instruction {
                    Statistics::global_record(
                        core_id,
                        EventType::SpecialMemoryInstructionSharedCacheMiss,
                        is_os,
                    );
                }

                return CacheHierarchyAccessResult::Miss;
            }
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

        // The check for the v_ts.
        for (replica_cache_id, set, index) in acquired_sets.iter() {
            if let Some(index) = index {
                let line = &set.lines[*index];

                if *replica_cache_id == p_cache_id {
                    continue;
                }

                if is_store {
                    if line.access_virtual_timestamp() > v_ts {
                        self.handle_vts_violation(
                            core_id,
                            v_ts,
                            (line.access_virtual_timestamp() - v_ts) as u32,
                            false,
                            is_os,
                        );
                    }
                } else if line.write_virtual_timestamp() > v_ts {
                    self.handle_vts_violation(
                        core_id,
                        v_ts,
                        (line.write_virtual_timestamp() - v_ts) as u32,
                        false,
                        is_os,
                    );
                }
            }
        }

        if PRECISE_COHERENCE_RECONSTRUCTION {
            // Check whether it has a write history before the eviction.
            if miss_directory_guard.recent_writer_ts > ts {
                // well, this is an order violation.
                // Now, release the lock of the private cache.
                drop(acquired_sets);

                if self.with_statistics {
                    // The truth is that we don't know whether this is a miss or hit, because a previous write operation has cleaned the history.
                    Statistics::global_record(core_id, EventType::UnknownPrivateCacheMisses, is_os);
                    // Accordingly, we don't know whether this access would have cause a shared cache miss.
                    Statistics::global_record(core_id, EventType::UnknownSharedCacheMisses, is_os);
                }

                return CacheHierarchyAccessResult::Unknown;
            }

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
                        if line.write_ts() > other_write_ts {
                            other_write_ts = line.write_ts();
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

            if other_has_written_with_large_ts {
                // Well, this cache line is already touched by another core with a later timestamp.
                // Only that core should be kept.

                // Update the directory.
                miss_directory_guard.update_lru_ts(ts);
                assert!(miss_directory_guard.recent_writer_ts <= other_write_ts);
                miss_directory_guard.recent_writer_ts = other_write_ts; // The writer timestamp can be updated as well.

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
                    Statistics::global_record(core_id, EventType::UnknownPrivateCacheMisses, is_os);
                    // Accordingly, we don't know whether this access would have cause a shared cache miss.
                    Statistics::global_record(core_id, EventType::UnknownSharedCacheMisses, is_os);
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
                        // This means you will only get the read permission, because there is a core with read permission and large timestamp.
                        assert!(entry.write_ts() <= ts);
                        assert!(*replica_cache_id != p_cache_id);

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
                // This means there is no sharer. The core will get modified permission.
                set_for_refill_lock.fill_with_potential_eviction_slot(
                    evicted_slot,
                    block_id,
                    ts,
                    v_ts,
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
                    v_ts,
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
            } else {
                // This memory access is definitely not the first one to this cache line.
                assert!(miss_directory_guard.insertion_ts <= ts);
            }

            // add self to the incoming sharer list.
            incoming_sharer.set(p_cache_id, true);

            // update the directory.
            miss_directory_guard.update_lru_ts(ts);
            miss_directory_guard.sharers = incoming_sharer;

            // We have a new write exposed to the directory.
            assert!(miss_directory_guard.recent_writer_ts <= ts);
            miss_directory_guard.recent_writer_ts = ts;

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
                        already_modified = true;
                    }
                }

                if *replica_cache_id == p_cache_id {
                    set_for_refill_lock = Some(set);
                }
            }

            // Then, we need to add self to the directory.
            miss_directory_guard.update_lru_ts(ts);
            miss_directory_guard.sharers.set(p_cache_id, true);

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
                v_ts,
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
                v_ts,
                is_modified,
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

    pub fn handle_eviction<const SET: usize>(
        &self,
        directory_set_guard: &mut impl DerefMut<Target = DirectorySet<SET>>,
        cache_id: usize,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        modified: (bool, u64),
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
        directory_entry.update_lru_ts(ts);
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
        if modified.0 {
            let evicted_cache_line_write_ts = modified.1;
            // keep the latest write timestamp.
            directory_entry.recent_writer_ts =
                if directory_entry.recent_writer_ts < evicted_cache_line_write_ts {
                    evicted_cache_line_write_ts
                } else {
                    directory_entry.recent_writer_ts
                };
        }

        // before releasing the lock of the directory, we need to check whether we need to place this lock to the shared cache.
        if directory_entry.sharers.count_ones() == 0 {
            // we need to place this block to the shared cache.
            if self.with_statistics {
                Statistics::global_record(
                    PCache::find_cache_info_by_cache_id(cache_id).0,
                    EventType::SharedCacheAccess,
                    false,
                );
            }

            let core_id = PCache::find_cache_info_by_cache_id(cache_id).0;

            if FILL_SCACLE_ON_PCACHE_EVICTION && !modified.0 {
                self.shared_cache
                    .insert(core_id, block_id, ts, v_ts, modified.0, true);
            }

            if FILL_SCACHE_ON_PCACHE_WRITEBACK && modified.0 {
                self.shared_cache
                    .insert(core_id, block_id, ts, v_ts, modified.0, true);
            }
        }
    }

    #[inline]
    pub fn handle_vts_violation(
        &self,
        core_id: u32,
        v_ts: u64,
        time_diff: u32,
        is_shared_cache: bool,
        is_os: bool,
    ) {

        if !self.with_statistics {
            return;
        }

        if let Some(hists) = self.vts_violation_distribution.as_ref() {
            // let quantum_number = (v_ts - 1) / self.quantum_size;
            // let violated_quantum_number = (v_ts + time_diff as u64 - 1) / self.quantum_size;
            // if quantum_number != violated_quantum_number {
            //     println!(
            //         "Quantum Number: {}, Violated Quantum Number: {}",
            //         quantum_number,
            //         violated_quantum_number,
            //     );
            //     println!("VTS: {}, Time Diff: {} VTS + Diff: {}", v_ts, time_diff, v_ts + time_diff as u64);
            // }

            // assert_eq!(quantum_number, violated_quantum_number);


            if is_shared_cache {
                Statistics::global_record(core_id, EventType::SharedCacheVTsOrderViolation, is_os);
            } else {
                Statistics::global_record(core_id, EventType::PrivateCacheVTsOrderViolation, is_os);
            }

            let hist = unsafe { &mut *hists[core_id as usize].get() };
            hist.record(time_diff as u64).unwrap();
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

    pub fn dump_diagnose_information(&self) {
        self.private_caches.print_debug_info();

        // self.shared_cache
        //     .dump_access_frequency("shared_cache_access_frequency.csv");

        // if let Some(hist) = self.vts_violation_distribution.as_ref() {
        //     // we need to dump the distribution.
        //     for core_id in 0..parameter::CORE_COUNT {
        //         let hist = unsafe { &mut *hist[core_id].get() };
        //         let mut serializer = hdrhistogram::serialization::V2Serializer::new();
        //         let mut buffer = Vec::new();
        //         serializer.serialize(hist, &mut buffer).unwrap();
        //         let mut file = File::create(format!("vts_violation_{}.hist", core_id)).unwrap();
        //         file.write_all(&buffer).unwrap();
        //     }
        // }
    }
}
