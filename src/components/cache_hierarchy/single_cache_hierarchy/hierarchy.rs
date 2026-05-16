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

use rayon::prelude::*;


use crate::{
    arch::AArch64,
    components::cache_hierarchy::{
        CacheBlockRequest, MemoryHierarchy,
        common::{
            CacheAccessType, CacheHierarchyAccessResult, SharedCache, SharedCacheAccessRequest,
            SharedCacheAccessSource, SharedCacheLookupResult,
        },
        mmu::{self, AbstractMMU, MMUTranslationResult},
    },
    debug::statistics::{EventType, Statistics},
    parameter,
};

use super::super::common::{ParallelLRUSharedCache, statistics::ZeroSharedCacheSetStatistics};

pub struct SingleCacheHierarchy<MMU: AbstractMMU> {
    pub shared_cache: ParallelLRUSharedCache<
        ZeroSharedCacheSetStatistics,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,

    mmus: [UnsafeCell<MMU>; parameter::CORE_COUNT],
}

unsafe impl<MMU: AbstractMMU> Sync for SingleCacheHierarchy<MMU> {}

impl<MMU: AbstractMMU> SingleCacheHierarchy<MMU> {
    pub fn new() -> Self {
        SingleCacheHierarchy {
            shared_cache: ParallelLRUSharedCache::new(),
            mmus: std::array::from_fn(|_| UnsafeCell::new(MMU::new())),
        }
    }

    fn serialize_mmus(&self, name: &str, numa_node_id: usize) {
        use crate::checkpoint::helpers::MMUsHelper;
        use crate::parameter::USE_RKYV_SERIALIZATION;

        let mmus_helper = MMUsHelper {
            mmus: self
                .mmus
                .iter()
                .map(|x| unsafe { (*x.get()).serialize() })
                .collect(),
        };

        if USE_RKYV_SERIALIZATION {
            let file =
                std::fs::File::create(format!("{}/mmus-{}.rkyv.zstd", name, numa_node_id)).unwrap();
            let mut encoder = Encoder::new(file, 0).unwrap();

            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&mmus_helper).unwrap();
            std::io::Write::write_all(&mut encoder, &bytes).unwrap();
            encoder.finish().unwrap();
        } else {
            let file =
                std::fs::File::create(format!("{}/mmus-{}.json.zstd", name, numa_node_id)).unwrap();
            let mut encoder = Encoder::new(file, 0).unwrap();

            serde_json::to_writer(&mut encoder, &mmus_helper).unwrap();
            encoder.finish().unwrap();
        }
    }

    fn deserialize_mmus(&self, name: &str, numa_node_id: usize) {
        // Try rkyv format first, fall back to JSON for backward compatibility
        let rkyv_path = format!("{}/mmus-{}.rkyv.zstd", name, numa_node_id);
        let json_path = format!("{}/mmus-{}.json.zstd", name, numa_node_id);

        if let Ok(file) = std::fs::File::open(&rkyv_path) {
            // Load rkyv format
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            let mmus_helper: crate::checkpoint::helpers::MMUsHelper = rkyv::from_bytes::<
                crate::checkpoint::helpers::MMUsHelper,
                rkyv::rancor::Error,
            >(&bytes)
            .unwrap();

            for (i, mmu_helper) in mmus_helper.mmus.into_iter().enumerate() {
                unsafe { (*self.mmus[i].get()).deserialize(mmu_helper) };
            }
        } else if let Ok(file) = std::fs::File::open(&json_path) {
            // Fall back to JSON format for backward compatibility
            let decoder = Decoder::new(file).unwrap();
            let mmus_helper: crate::checkpoint::helpers::MMUsHelper =
                serde_json::from_reader(decoder).unwrap();

            for (i, mmu_helper) in mmus_helper.mmus.into_iter().enumerate() {
                unsafe { (*self.mmus[i].get()).deserialize(mmu_helper) };
            }
        } else {
        }
    }

    fn serialize_mmus_worker(&self, worker_id: usize, name: &str, numa_node_id: usize) {
        use crate::checkpoint::helpers::MMUsHelper;
        use crate::parameter::{CHECKPOINT_POOL_SIZE, CORE_COUNT, USE_RKYV_SERIALIZATION};

        let cores_per_worker = CORE_COUNT / CHECKPOINT_POOL_SIZE;
        let begin = worker_id * cores_per_worker;
        let end = begin + cores_per_worker;

        let mmus_helper = MMUsHelper {
            mmus: self.mmus[begin..end]
                .iter()
                .map(|x| unsafe { (*x.get()).serialize() })
                .collect(),
        };

        if USE_RKYV_SERIALIZATION {
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&mmus_helper).unwrap();
            crate::util::write_compressed(
                &format!("{}/mmus-{}-worker-{}.rkyv.zstd", name, numa_node_id, worker_id),
                &bytes,
            );
        } else {
            let bytes = serde_json::to_vec(&mmus_helper).unwrap();
            crate::util::write_compressed(
                &format!("{}/mmus-{}-worker-{}.json.zstd", name, numa_node_id, worker_id),
                &bytes,
            );
        }
    }

    fn deserialize_mmus_worker(&self, worker_id: usize, name: &str, numa_node_id: usize) -> bool {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, CORE_COUNT};

        let cores_per_worker = CORE_COUNT / CHECKPOINT_POOL_SIZE;
        let begin = worker_id * cores_per_worker;

        let rkyv_path = format!(
            "{}/mmus-{}-worker-{}.rkyv.zstd",
            name, numa_node_id, worker_id
        );
        let json_path = format!(
            "{}/mmus-{}-worker-{}.json.zstd",
            name, numa_node_id, worker_id
        );

        if let Ok(file) = std::fs::File::open(&rkyv_path) {
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            let mmus_helper: crate::checkpoint::helpers::MMUsHelper = rkyv::from_bytes::<
                crate::checkpoint::helpers::MMUsHelper,
                rkyv::rancor::Error,
            >(&bytes)
            .unwrap();

            for (i, mmu_helper) in mmus_helper.mmus.into_iter().enumerate() {
                unsafe { (*self.mmus[begin + i].get()).deserialize(mmu_helper) };
            }
        } else if let Ok(file) = std::fs::File::open(&json_path) {
            let decoder = Decoder::new(file).unwrap();
            let mmus_helper: crate::checkpoint::helpers::MMUsHelper =
                serde_json::from_reader(decoder).unwrap();

            for (i, mmu_helper) in mmus_helper.mmus.into_iter().enumerate() {
                unsafe { (*self.mmus[begin + i].get()).deserialize(mmu_helper) };
            }
        } else {
            return false;
        }
        true
    }

    pub fn get_scache_warmed_set_count(&self) -> usize {
        self.shared_cache.warmed_sets_count()
    }

    pub fn get_scache_warmed_slots_count(&self) -> usize {
        self.shared_cache.warmed_slots_count()
    }
}

impl<MMU: AbstractMMU> MemoryHierarchy for SingleCacheHierarchy<MMU> {
    fn prefetch_blocks(&self, _request: &CacheBlockRequest, _ts: u64) {
        unimplemented!();
    }

    fn record_access(&self, _request: &CacheBlockRequest, _ts: u64) {
        unimplemented!();
    }

    fn evict_sms(&self, _core_id: u32, _block_id: u64) {
        unimplemented!();
    }

    fn access_memory_pblock_id(
        &self,
        request: &CacheBlockRequest,
        ts: u64,
    ) -> (CacheHierarchyAccessResult, (usize, usize, usize)) {
        let is_store = request.is_store();
        let is_ptw = request.is_page_walk();
        let is_fetch = request.is_instruction();
        let is_os = request.is_os();
        let core_id = request.core_id;
        let block_id = request.block_id;

        Statistics::global_record(core_id, EventType::DataAccess, is_os);
        Statistics::global_record(core_id, EventType::SharedCacheAccess, is_os);

        let res = self.shared_cache.lookup_and_insert_on_miss(
            &SharedCacheAccessRequest {
                source: SharedCacheAccessSource::Core(core_id),
                block_id,
                access_type: CacheAccessType::DataRead, // Read does not have impact on the tag array.
                is_os,
            },
            ts,
            true,
        );

        Statistics::global_record(
            core_id,
            match res {
                SharedCacheLookupResult::Hit(_) => EventType::SharedCacheAccess,
                SharedCacheLookupResult::Miss => EventType::SharedCacheMiss,
                SharedCacheLookupResult::ColdMiss => EventType::SharedCacheColdMiss,
                SharedCacheLookupResult::LookupLate(_, _) => {
                    EventType::SharedCacheAccessCausalityViolation
                }
                SharedCacheLookupResult::EvictedLate(_) => {
                    EventType::SharedCacheEvictionCausalityViolation
                }
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

        (
            match res {
                SharedCacheLookupResult::Hit(_) => CacheHierarchyAccessResult::HitInSharedCache,
                SharedCacheLookupResult::Miss => CacheHierarchyAccessResult::Miss,
                SharedCacheLookupResult::ColdMiss => CacheHierarchyAccessResult::Miss,
                SharedCacheLookupResult::LookupLate(_, _) => CacheHierarchyAccessResult::Unknown,
                SharedCacheLookupResult::EvictedLate(_) => CacheHierarchyAccessResult::Miss,
            },
            (0, 0, 0),
        )
    }

    fn translate(
        &self,
        r: &crate::components::cache_hierarchy::MemoryAccessRequest,
        ts: u64,
    ) -> MMUTranslationResult {
        unsafe {
            self.mmus[r.core_id as usize]
                .get()
                .as_mut()
                .unwrap()
                .translate_and_refill(r.core_id, r.va, ts, r.is_instruction())
        }
    }

    fn flush_mmu(&self, core_id: u32, info: crate::components::cache_hierarchy::mmu::MMUFlushMode) {
        unsafe {
            self.mmus[core_id as usize]
                .get()
                .as_mut()
                .unwrap()
                .flush(info);
        }
    }

    fn serialize(&self, name: &str, numa_node_id: usize) {
        println!("Serializing private caches.");
        self.shared_cache.serialize(name, numa_node_id);
        println!("Serialize MMUs");
        self.serialize_mmus(name, numa_node_id);
    }

    fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        println!("Deserializing private caches.");
        self.shared_cache.deserialize(name, numa_node_id);
        println!("Deserialize MMUs");
        self.deserialize_mmus(name, numa_node_id);
    }

    fn serialize_par(&self, name: &str, numa_node_id: usize) {
        use crate::CHECKPOINT_POOL;
        use crate::parameter::CHECKPOINT_POOL_SIZE;

        CHECKPOINT_POOL.get().unwrap().install(|| {
            (0..CHECKPOINT_POOL_SIZE).into_par_iter().for_each(|worker_id| {
                self.shared_cache.serialize_shard(worker_id, name, numa_node_id);
                self.serialize_mmus_worker(worker_id, name, numa_node_id);
            });
        });

        println!("Parallel checkpoint serialization complete.");
    }

    fn deserialize_par(&mut self, name: &str, numa_node_id: usize) {
        use crate::parameter::CHECKPOINT_POOL_SIZE;

        let mut shared_cache_loaded = true;
        let mut mmus_loaded = true;

        for worker_id in 0..CHECKPOINT_POOL_SIZE {
            shared_cache_loaded &= self.shared_cache.deserialize_shard(worker_id, name, numa_node_id);
            mmus_loaded &= self.deserialize_mmus_worker(worker_id, name, numa_node_id);
        }

        if shared_cache_loaded {
            println!("Loaded shared cache from checkpoint");
        }
        if mmus_loaded {
            println!("Loaded MMUs from checkpoint");
        }
    }

    fn access_from_device_with_pa(
        &self,
        _paddr: u64,
        _access_type: CacheAccessType,
        _ts: u64,
    ) -> CacheHierarchyAccessResult {
        todo!()
    }
}

type AArch64MMU = mmu::OrdinaryMMU<
    AArch64,
    { parameter::ITLB_ASSO },
    { parameter::ITLB_SET },
    { parameter::DTLB_ASSO },
    { parameter::DTLB_SET },
    { parameter::STLB_ENABLED },
    { parameter::STLB_ASSO },
    { parameter::STLB_SET },
    { parameter::NO_HUGE_PAGE },
>;

pub type PluginSingleCacheHierarchy = SingleCacheHierarchy<AArch64MMU>;
