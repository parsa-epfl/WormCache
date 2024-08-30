// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this
//    list of conditions and the following disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.
//
// 3. Neither the name of the PARSA, EPFL
//    nor the names of its contributors may be used to endorse or promote
//    products derived from this software without specific prior written
//    permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::cell::UnsafeCell;

use zstd::{Decoder, Encoder};

use crate::{
    components::{
        cache_hierarchy::common::{
            CacheAccessType, CacheHierarchyAccessResult, SharedCache, SharedCacheLookupResult,
        },
        debug::statistics::{EventType, Statistics},
    },
    parameter::{self, ADJACENT_LINE_PREFETCHING},
};

use super::super::common::{statistics::ZeroSharedCacheSetStatistics, SerialSingleSharedCache};

pub struct SingleCacheHierarchy<MMU: crate::components::mmu::AbstractMMU> {
    pub shared_cache: SerialSingleSharedCache<
        ZeroSharedCacheSetStatistics,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,

    mmus: [UnsafeCell<MMU>; parameter::CORE_COUNT],
}

impl<MMU: crate::components::mmu::AbstractMMU> SingleCacheHierarchy<MMU> {
    pub fn new() -> Self {
        SingleCacheHierarchy {
            shared_cache: SerialSingleSharedCache::new(),
            mmus: std::array::from_fn(|_| UnsafeCell::new(MMU::new())),
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
                .translate_and_refill(va, ts, is_instruction)
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

                Statistics::global_record(core_id, EventType::TLBMiss, is_os);
                if is_instruction {
                    Statistics::global_record(core_id, EventType::TLBMissDueToInstruction, is_os);
                } else {
                    Statistics::global_record(core_id, EventType::TLBMissDueToData, is_os);
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
                .translate_and_refill(va, ts, is_instruction)
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

                Statistics::global_record(core_id, EventType::TLBMiss, is_os);

                if is_instruction {
                    Statistics::global_record(core_id, EventType::TLBMissDueToInstruction, is_os);
                } else {
                    Statistics::global_record(core_id, EventType::TLBMissDueToData, is_os);
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

    pub fn access_memory_pblock_id(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        access_type: CacheAccessType,
        is_os: bool,
        _instruction_va_pc: u64,
    ) -> CacheHierarchyAccessResult {
        let is_store = access_type == CacheAccessType::DataWrite;
        let is_ptw = access_type == CacheAccessType::PageWalkRead;
        let is_fetch = access_type == CacheAccessType::InstructionFetch;

        Statistics::global_record(core_id, EventType::DataAccess, is_os);
        Statistics::global_record(core_id, EventType::SharedCacheAccess, is_os);

        let res = self
            .shared_cache
            .lookup_and_insert_on_miss(
                core_id,
                block_id,
                ts,
                v_ts,
                true,
                is_store,
                true,
                access_type,
                is_os,
            )
            .0;

        Statistics::global_record(
            core_id,
            match res {
                SharedCacheLookupResult::Hit(_) => EventType::SharedCacheAccess,
                SharedCacheLookupResult::Miss => EventType::SharedCacheMiss,
                SharedCacheLookupResult::ColdMiss => EventType::SharedCacheColdMiss,
                SharedCacheLookupResult::Unknown(_) => EventType::UnknownSharedCacheMisses,
            },
            is_os,
        );

        if matches!(res, SharedCacheLookupResult::Miss) {
            if is_store {
                Statistics::global_record(core_id, EventType::SharedCacheMissDueToDataWrite, is_os);
            } else if is_fetch {
                Statistics::global_record(
                    core_id,
                    EventType::SharedCacheMissDueToInstructionFetch,
                    is_os,
                );
            } else if is_ptw {
                Statistics::global_record(core_id, EventType::SharedCacheMissDueToPTW, is_os);
            } else {
                Statistics::global_record(core_id, EventType::SharedCacheMissDueToDataRead, is_os);
            }
        }

        let cache_hierarchy_access_result = match res {
            SharedCacheLookupResult::Hit(_) => CacheHierarchyAccessResult::HitInSharedCache,
            SharedCacheLookupResult::Miss => CacheHierarchyAccessResult::Miss,
            SharedCacheLookupResult::ColdMiss => CacheHierarchyAccessResult::Miss,
            SharedCacheLookupResult::Unknown(_) => CacheHierarchyAccessResult::Unknown,
        };

        cache_hierarchy_access_result
    }

    fn serialize_mmus(&self, name: &str, numa_node_id: usize) {
        let file =
            std::fs::File::create(format!("{}/mmus-{}.json.zstd", name, numa_node_id)).unwrap();
        let mut file = Encoder::new(file, 0).unwrap();

        let multiple_mmus = self
            .mmus
            .iter()
            .map(|x| unsafe { (*x.get()).serialize() })
            .collect::<Vec<_>>();

        serde_json::to_writer(&mut file, &serde_json::Value::Array(multiple_mmus)).unwrap();

        file.finish().unwrap();
    }

    fn deserialize_mmus(&self, name: &str, numa_node_id: usize) {
        let file = std::fs::File::open(format!("{}/mmus-{}.json.zstd", name, numa_node_id));

        if file.is_err() {
            println!("Cannot load the MMU state. Error: {:?}", file.err());
            return;
        }

        let file = file.unwrap();

        let mut file = Decoder::new(file).unwrap();

        let multiple_mmus: serde_json::Value = serde_json::from_reader(&mut file).unwrap();

        match multiple_mmus {
            serde_json::Value::Array(mmus) => {
                for (i, mmu) in mmus.into_iter().enumerate() {
                    unsafe { (*self.mmus[i].get()).deserialize(mmu) };
                }
            }
            _ => panic!("Invalid format."),
        };
    }

    pub fn serialize(&self, name: &str, numa_node_id: usize) {
        println!("Serializing private caches.");
        self.shared_cache.serialize(name, numa_node_id);
        println!("Serialize MMUs");
        self.serialize_mmus(name, numa_node_id);
    }

    pub fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        println!("Deserializing private caches.");
        self.shared_cache.deserialize(name, numa_node_id);
        println!("Deserialize MMUs");
        self.deserialize_mmus(name, numa_node_id);
    }

    pub fn get_scache_warmed_set_count(&self) -> usize {
        self.shared_cache.warmed_sets_count()
    }

    pub fn get_scache_warmed_slots_count(&self) -> usize {
        self.shared_cache.warmed_slots_count()
    }
}

pub type PluginSingleCacheHierarchy = SingleCacheHierarchy<
    crate::components::mmu::MemoryManagementUnit<
        crate::arch::AArch64,
        { parameter::TLB_ASSO },
        { parameter::TLB_SET },
    >,
>;
