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
use rustc_hash::FxHashMap as HashMap;

use zstd::{Decoder, Encoder};
use std::io::Write;


use crate::{
    components::{
        cache_hierarchy::common::{
            CacheAccessType, CacheHierarchyAccessResult,
        },
        debug::statistics::{EventType, Statistics},
    },
    parameter::{self, ADJACENT_LINE_PREFETCHING},
};

pub struct IdealCacheStat {
    u_instr: u64,
    u_data: u64,
    k_instr: u64,
    k_data: u64,
}

pub struct IdealCache<MMU: crate::components::mmu::AbstractMMU> {
    map: [UnsafeCell<HashMap<u64,IdealCacheStat>>; parameter::CORE_COUNT],
    mmus: [UnsafeCell<MMU>; parameter::CORE_COUNT],
}

impl<MMU: crate::components::mmu::AbstractMMU> IdealCache<MMU> {
    pub fn new() -> Self {
        IdealCache {
            map: std::array::from_fn(|_| UnsafeCell::new(HashMap::default())),
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
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
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
                        CacheAccessType::PageWalkRead,
                        false, // Page walk is not OS.
                        instruction_va_pc,
                    );
                }
                let block_id = paddr >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                let res = self.access_memory_pblock_id(
                    core_id,
                    block_id,
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
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
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
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
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
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
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
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
                    access_type,
                    is_os,
                    instruction_va_pc,
                );
                if ADJACENT_LINE_PREFETCHING {
                    self.access_memory_pblock_id(
                        core_id,
                        block_id + 1,
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
        access_type: CacheAccessType,
        is_os: bool,
        _instruction_va_pc: u64,
    ) -> CacheHierarchyAccessResult {
        let is_fetch = access_type == CacheAccessType::InstructionFetch;
        let core_idx = core_id as usize;

        let per_core_map = unsafe {
            &mut *self.map[core_idx].get()
        };

        let is_present = per_core_map.contains_key(&block_id);

        Statistics::global_record(core_id, EventType::DataAccess, is_os);
        Statistics::global_record(core_id, EventType::SharedCacheAccess, is_os);

        if is_present {
            if is_os {
                if is_fetch {
                    per_core_map.get_mut(&block_id).unwrap().k_instr += 1;
                } else {
                    per_core_map.get_mut(&block_id).unwrap().k_data += 1;
                }
            } else {
                if is_fetch {
                    per_core_map.get_mut(&block_id).unwrap().u_instr += 1;
                } else {
                    per_core_map.get_mut(&block_id).unwrap().u_data += 1;
                }
            }
        } else {
            per_core_map.insert(
                block_id,
                IdealCacheStat {
                    u_instr: if is_fetch && !is_os { 1 } else { 0 },
                    u_data: if !is_fetch && !is_os { 1 } else { 0 },
                    k_instr: if is_fetch && is_os { 1 } else { 0 },
                    k_data: if !is_fetch && is_os { 1 } else { 0 },
                },
            );
        }

        let cache_hierarchy_access_result = CacheHierarchyAccessResult::HitInSelfPrivateCache;

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
        println!("Serialize MMUs");
        self.serialize_mmus(name, numa_node_id);
    }

    pub fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        println!("Deserialize MMUs");
        self.deserialize_mmus(name, numa_node_id);
    }

    pub fn dump_map(&self) {
        // Iterate over the array of hash maps
        for (idx, map) in self.map.iter().enumerate() {
            let mut file = std::fs::File::create(format!("IdealCacheStats/map-{}.json.zstd", idx)).unwrap();
            // Iterate over the key-value pairs in each hash map
            for (key, value) in unsafe { &*map.get() }.iter() {
                writeln!(file, "{}, {}, {}, {}, {}", key, value.k_instr, value.k_data, value.u_instr, value.u_data).unwrap();
            }
        }
    }

}

pub type PluginIdealCache = IdealCache<
    crate::components::mmu::MemoryManagementUnit<
        crate::arch::AArch64,
        { parameter::TLB_ASSO },
        { parameter::TLB_SET },
    >,
>;
