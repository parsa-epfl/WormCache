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

// This module defines the basic memory hierarchies using fine-grained locks.
// It contains the same logical memory hierarchy as the `memory`, but uses locks for shared communication.
// - TLB, which is private.
// - Private caches, with set locks.
// - Directory, with set locks.
// - Shared caches, with set locks.

pub mod common;
pub mod mmu;
mod parallel_hierarchy;
mod single_cache_hierarchy;

use common::CacheAccessType;
use common::CacheHierarchyAccessResult;
use mmu::MMUTranslationResult;
pub use parallel_hierarchy::ParallelCacheHierarchyPlugin;
pub use parallel_hierarchy::hierarchy;

pub use single_cache_hierarchy::SingleCacheHierarchyPlugin;

use crate::parameter;
use crate::parameter::ADJACENT_LINE_PREFETCHING;
use crate::parameter::SMS_PREFETCHING;

#[derive(Clone)]
pub struct MemoryAccessRequest {
    pub core_id: u32,
    pub va: u64,
    pub access_type: CacheAccessType,
    pub is_os: bool,
    pub pc: u64,
}

impl MemoryAccessRequest {
    pub fn is_instruction(&self) -> bool {
        self.access_type == CacheAccessType::InstructionFetch
    }

    pub fn is_store(&self) -> bool {
        self.access_type == CacheAccessType::DataWrite
            || self.access_type == CacheAccessType::PrefetchWrite
    }

    pub fn is_prefetch(&self) -> bool {
        self.access_type == CacheAccessType::PrefetchRead
            || self.access_type == CacheAccessType::PrefetchWrite
    }

    pub fn is_os(&self) -> bool {
        self.is_os
    }
}

#[derive(Clone)]
pub struct CacheBlockRequest {
    pub core_id: u32,
    pub block_id: u64,
    pub access_type: CacheAccessType,
    pub is_os: bool,
    pub pc: u64,
}

impl CacheBlockRequest {
    pub fn is_instruction(&self) -> bool {
        self.access_type == CacheAccessType::InstructionFetch
    }

    pub fn is_store(&self) -> bool {
        self.access_type == CacheAccessType::DataWrite
            || self.access_type == CacheAccessType::PrefetchWrite
    }

    pub fn is_prefetch(&self) -> bool {
        self.access_type == CacheAccessType::PrefetchRead
            || self.access_type == CacheAccessType::PrefetchWrite
    }

    pub fn is_os(&self) -> bool {
        self.is_os
    }

    pub fn is_page_walk(&self) -> bool {
        self.access_type == CacheAccessType::PageWalkRead
    }

    pub fn get_prefetch_type(&self) -> CacheAccessType {
        match self.access_type {
            CacheAccessType::DataRead => CacheAccessType::PrefetchRead,
            CacheAccessType::DataWrite => CacheAccessType::PrefetchWrite,
            _  => unreachable!(),
        }
    }

}

pub trait MemoryHierarchy {
    fn access_memory_pblock_id(
        &self,
        request: &CacheBlockRequest,
        ts: u64,
    ) -> (CacheHierarchyAccessResult, (usize, usize, usize));

    fn translate(&self, request: &MemoryAccessRequest, ts: u64) -> MMUTranslationResult;

    fn flush_mmu(&self, core_id: u32, info: mmu::MMUFlushMode);

    fn prefetch_blocks(&self, request: &CacheBlockRequest, ts: u64);

    fn record_access(&self, request: &CacheBlockRequest, ts: u64);

    fn evict_sms(&self, core_id: u32, block_id: u64);

    #[inline]
    fn access_memory_with_va_and_pa(
        &self,
        request: &MemoryAccessRequest,
        pa: Option<u64>,
        ts: u64,
    ) -> CacheHierarchyAccessResult {
        assert!(
            !(request.is_instruction() && request.is_store()),
            "Instruction and store permission cannot be used at the same time."
        );

        let translation = self.translate(request, ts);

        let translated_request = match translation {
            MMUTranslationResult::MissNotCacheable(paddr) | MMUTranslationResult::Hit(paddr, _) => {
                let block_id = paddr >> parameter::CACHE_LINE_SIZE.trailing_zeros();

                let block_id = if let Some(pa) = pa {
                    pa >> parameter::CACHE_LINE_SIZE.trailing_zeros()
                } else {
                    block_id
                };

                CacheBlockRequest {
                    core_id: request.core_id,
                    block_id,
                    access_type: request.access_type.clone(),
                    is_os: request.is_os,
                    pc: request.pc,
                }
            }
            MMUTranslationResult::Miss(paddr, walk_trace) => {
                // walk.
                for pa in walk_trace {
                    if pa == u64::MAX {
                        break;
                    }
                    let pte_block_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                    let request = CacheBlockRequest {
                        core_id: request.core_id,
                        block_id: pte_block_id,
                        access_type: CacheAccessType::PageWalkRead,
                        is_os: request.is_os,
                        pc: request.pc,
                    };
                    let (result, _) = self.access_memory_pblock_id(&request, ts);
                    let code: u8 = match result {
                        CacheHierarchyAccessResult::HitInSelfPrivateCache => 0,
                        CacheHierarchyAccessResult::HitInSharedCache => 1,
                        CacheHierarchyAccessResult::HitInOtherPrivateCache => 3,
                        CacheHierarchyAccessResult::Miss => 2,
                        CacheHierarchyAccessResult::MissDueToPermission => 4,
                        CacheHierarchyAccessResult::MissInPrivateCache => 5,
                        CacheHierarchyAccessResult::Unknown => 6,
                    };
                    let access_code: u8 = match request.access_type {
                        CacheAccessType::DataRead => 0,
                        CacheAccessType::DataWrite => 1,
                        CacheAccessType::InstructionFetch => 2,
                        CacheAccessType::PrefetchRead => 3,
                        CacheAccessType::PrefetchWrite => 4,
                        CacheAccessType::PageWalkRead => 5,
                    };
                    println!("{},{},{},{},{},{},{}", ts, request.core_id, request.block_id, access_code, request.is_os, request.pc, code);
                }

                let block_id = paddr >> parameter::CACHE_LINE_SIZE.trailing_zeros();

                let block_id = if let Some(pa) = pa {
                    pa >> parameter::CACHE_LINE_SIZE.trailing_zeros()
                } else {
                    block_id
                };

                CacheBlockRequest {
                    core_id: request.core_id,
                    block_id,
                    access_type: request.access_type.clone(),
                    is_os: request.is_os,
                    pc: request.pc,
                }
            }
        };

        let (result, _) = self.access_memory_pblock_id(&translated_request, ts);
        let code: u8 = match result {
            CacheHierarchyAccessResult::HitInSelfPrivateCache => 0,
            CacheHierarchyAccessResult::HitInSharedCache => 1,
            CacheHierarchyAccessResult::HitInOtherPrivateCache => 3,
            CacheHierarchyAccessResult::Miss => 2,
            CacheHierarchyAccessResult::MissDueToPermission => 4,
            CacheHierarchyAccessResult::MissInPrivateCache => 5,
            CacheHierarchyAccessResult::Unknown => 6,
        };
        let access_code: u8 = match translated_request.access_type {
            CacheAccessType::DataRead => 0,
            CacheAccessType::DataWrite => 1,
            CacheAccessType::InstructionFetch => 2,
            CacheAccessType::PrefetchRead => 3,
            CacheAccessType::PrefetchWrite => 4,
            CacheAccessType::PageWalkRead => 5,
        };
        println!("{},{},{},{},{},{},{}", ts, translated_request.core_id, translated_request.block_id, access_code, translated_request.is_os, translated_request.pc, code);
        if ADJACENT_LINE_PREFETCHING {
            let mut prefetch_request = translated_request.clone();
            prefetch_request.block_id += 1;
            prefetch_request.access_type = prefetch_request.get_prefetch_type();
            self.access_memory_pblock_id(&prefetch_request, ts);
        }
        if SMS_PREFETCHING && !translated_request.is_instruction() {
            self.prefetch_blocks(&translated_request, ts);
            self.record_access(&translated_request, ts);
        }
        result
    }

    #[inline]
    fn access_memory_with_va(
        &self,
        request: &MemoryAccessRequest,
        ts: u64,
    ) -> CacheHierarchyAccessResult {
        self.access_memory_with_va_and_pa(request, None, ts)
    }

    fn serialize(&self, name: &str, numa_node_id: usize);
    fn deserialize(&mut self, name: &str, numa_node_id: usize);
}
