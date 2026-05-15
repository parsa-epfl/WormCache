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

use crate::checkpoint::helpers::UnifiedPrivateCacheHelper;
use crate::components::cache_hierarchy::CacheBlockRequest;

use super::{PrivateCache, PrivateCachePokeResult, PrivateCacheSet};
use spin::mutex::SpinMutex;
use std::collections::HashMap;
use std::ops::DerefMut;

use zstd::{Decoder, Encoder};

#[repr(align(64))]
#[derive(Debug)]
pub struct UnifiedPerCorePrivateCache<const SET: usize, const ASSO: usize> {
    cache: Box<[SpinMutex<PrivateCacheSet>; SET]>,
}

impl<const SET: usize, const ASSO: usize> UnifiedPerCorePrivateCache<SET, ASSO> {
    pub fn new() -> Self {
        Self {
            cache: crate::util::init_heap_array(|_| SpinMutex::new(PrivateCacheSet::new(ASSO))),
        }
    }

    pub fn get_set(&self, block_id: u64) -> &SpinMutex<PrivateCacheSet> {
        let set_id = block_id as usize % SET;
        &self.cache[set_id]
    }

    /// Convert to unified checkpoint helper (used for both JSON and rkyv).
    pub fn to_checkpoint_helper(&self) -> UnifiedPrivateCacheHelper {
        UnifiedPrivateCacheHelper {
            cache: self.cache.iter().map(|set| set.lock().clone()).collect(),
        }
    }

    /// Create from unified checkpoint helper.
    pub fn from_checkpoint_helper(helper: UnifiedPrivateCacheHelper) -> Self {
        let cache = helper
            .cache
            .into_iter()
            .map(|set| SpinMutex::new(set))
            .collect::<Vec<_>>();

        Self {
            cache: cache.try_into().unwrap(),
        }
    }
}

pub struct UnifiedPrivateCaches<const CORE_COUNT: usize, const SET: usize, const ASSO: usize> {
    caches: Box<[UnifiedPerCorePrivateCache<SET, ASSO>; CORE_COUNT]>,
}

impl<const CORE_COUNT: usize, const SET: usize, const ASSO: usize> PrivateCache
    for UnifiedPrivateCaches<CORE_COUNT, SET, ASSO>
{
    const DIRECTORY_SET: usize = SET;

    fn new() -> Self {
        Self {
            caches: crate::util::init_heap_array(|_| UnifiedPerCorePrivateCache::new()),
        }
    }

    #[inline]
    fn poke_and_update(&self, request: &CacheBlockRequest, ts: u64) -> PrivateCachePokeResult {
        let core_id = request.core_id as usize;
        let block_id = request.block_id;
        let is_store = request.is_store();
        let is_instruction = request.is_instruction();

        self.caches[core_id]
            .get_set(block_id)
            .lock()
            .poke_and_update(block_id, ts, is_store, is_instruction)
    }

    #[inline]
    fn get_set_guard_by_sharer_list(
        &self,
        block_id: u64,
        sharers: super::super::SharerList,
    ) -> Vec<(
        usize,
        impl DerefMut<Target = PrivateCacheSet>,
        Option<usize>,
    )> {
        let mut result = Vec::with_capacity(sharers.count_ones());

        for core_id in sharers.iter_ones() {
            let set = self.caches[core_id].get_set(block_id);
            let guard = set.lock();
            let index = guard.index_of(block_id);
            result.push((core_id, guard, index));
        }

        result
    }

    #[inline]
    fn in_which_cores(&self, block_id: u64) -> Vec<u32> {
        let mut result = Vec::with_capacity(CORE_COUNT);
        for core_id in 0..CORE_COUNT {
            let set = self.caches[core_id].get_set(block_id);
            let guard = set.lock();
            if guard.poke(block_id).is_some() {
                result.push(core_id as u32);
            }
        }
        result
    }

    #[inline]
    fn query_replica_state(&self, block_id: u64) -> HashMap<u32, bool> {
        let mut res = HashMap::new();

        // If the i cache has the block, it is considered as shared. Just double check the d cache and make sure it is shared if exists.

        for core_id in 0..CORE_COUNT {
            let is_d = self.caches[core_id].cache[block_id as usize % SET]
                .lock()
                .poke(block_id);

            if let Some(d_line) = is_d {
                assert!(d_line.block_id() == block_id);
                res.insert(core_id as u32, d_line.is_modified());
            }
        }

        res
    }

    #[inline]
    fn find_cache_info_by_cache_id(index: usize) -> (u32, bool) {
        let core_id = index as u32;
        let is_instruction = false;
        (core_id, is_instruction)
    }

    #[inline]
    fn get_cache_id_by_cache_info(core_id: u32, _: bool) -> usize {
        core_id as usize
    }

    fn information() -> String {
        format!(
            "Type: UnifiedPrivateCache, Core Count: {}, Set: {}, Associativity: {}",
            { CORE_COUNT },
            SET,
            ASSO
        )
    }

    #[inline]
    fn get_set_for_fill(
        &self,
        request: &CacheBlockRequest,
    ) -> impl DerefMut<Target = PrivateCacheSet> {
        let core_id = request.core_id as usize;
        let block_id = request.block_id;
        self.caches[core_id].get_set(block_id).lock()
    }

    #[inline]
    fn print_debug_info(&self) {
        // aggregate every set's statistics.
        let mut hit_index = 0;
        let mut hit_count = 0;

        for cache in self.caches.iter() {
            for set in cache.cache.iter() {
                let guard = set.lock();
                hit_count += guard.hit_time;
                hit_index += guard.hit_index_acc;
            }
        }

        use std::io::prelude::*;

        // dump this information to a log.
        let mut log_file = std::fs::File::create("cache_log.txt").unwrap();

        writeln!(
            log_file,
            "UnifiedPrivateCache: hit_time: {}, hit_index: {}, average: {}",
            hit_count,
            hit_index,
            hit_index as f64 / hit_count as f64
        )
        .unwrap();
    }

    fn serialize(&self, name: &str, numa_node_id: usize) {
        use crate::parameter::USE_RKYV_SERIALIZATION;

        let helper: Vec<_> = self
            .caches
            .iter()
            .map(|cache| cache.to_checkpoint_helper())
            .collect();

        if USE_RKYV_SERIALIZATION {
            let file =
                std::fs::File::create(format!("{}/{}-{}.rkyv.zstd", name, "unified", numa_node_id))
                    .unwrap();

            let mut encoder = Encoder::new(file, 0).unwrap();
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
            std::io::Write::write_all(&mut encoder, &bytes).unwrap();
            encoder.finish().unwrap();
        } else {
            let file =
                std::fs::File::create(format!("{}/{}-{}.json.zstd", name, "unified", numa_node_id))
                    .unwrap();

            let mut file = Encoder::new(file, 0).unwrap();

            serde_json::to_writer(&mut file, &helper).unwrap();

            file.finish().unwrap();
        }
    }

    fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        use crate::parameter::USE_RKYV_SERIALIZATION;

        if USE_RKYV_SERIALIZATION {
            let file =
                std::fs::File::open(format!("{}/{}-{}.rkyv.zstd", name, "unified", numa_node_id));

            if file.is_err() {
                println!(
                    "Cannot load the unified private cache (rkyv). Error: {:?}",
                    file.err()
                );
                return;
            }

            let file = file.unwrap();
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            let helper: Vec<UnifiedPrivateCacheHelper> =
                rkyv::from_bytes::<Vec<UnifiedPrivateCacheHelper>, rkyv::rancor::Error>(&bytes)
                    .unwrap();

            for (cache, helper) in self.caches.iter_mut().zip(helper.into_iter()) {
                *cache = UnifiedPerCorePrivateCache::from_checkpoint_helper(helper);
            }
        } else {
            let file =
                std::fs::File::open(format!("{}/{}-{}.json.zstd", name, "unified", numa_node_id));

            if file.is_err() {
                println!(
                    "Cannot load the unified private cache. Error: {:?}",
                    file.err()
                );
                return;
            }

            let file = file.unwrap();
            let file = Decoder::new(file).unwrap();

            let helper: Vec<UnifiedPrivateCacheHelper> = serde_json::from_reader(file).unwrap();

            for (cache, helper) in self.caches.iter_mut().zip(helper.into_iter()) {
                *cache = UnifiedPerCorePrivateCache::from_checkpoint_helper(helper);
            }
        }
    }

    fn serialize_worker(&self, worker_id: usize, name: &str, numa_node_id: usize) {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, CORE_COUNT, USE_RKYV_SERIALIZATION};

        let cores_per_worker = CORE_COUNT / CHECKPOINT_POOL_SIZE;
        let begin = worker_id * cores_per_worker;
        let end = begin + cores_per_worker;

        let helpers: Vec<_> = self.caches[begin..end]
            .iter()
            .map(|cache| cache.to_checkpoint_helper())
            .collect();

        if USE_RKYV_SERIALIZATION {
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helpers).unwrap();
            crate::util::write_compressed(
                &format!("{}/unified-{}-worker-{}.rkyv.zstd", name, numa_node_id, worker_id),
                &bytes,
            );
        } else {
            let bytes = serde_json::to_vec(&helpers).unwrap();
            crate::util::write_compressed(
                &format!("{}/unified-{}-worker-{}.json.zstd", name, numa_node_id, worker_id),
                &bytes,
            );
        }
    }

    fn deserialize_worker(&mut self, worker_id: usize, name: &str, numa_node_id: usize) {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, CORE_COUNT, USE_RKYV_SERIALIZATION};

        let cores_per_worker = CORE_COUNT / CHECKPOINT_POOL_SIZE;
        let begin = worker_id * cores_per_worker;
        let end = begin + cores_per_worker;

        if USE_RKYV_SERIALIZATION {
            let file = std::fs::File::open(format!(
                "{}/unified-{}-worker-{}.rkyv.zstd",
                name, numa_node_id, worker_id
            ));

            if file.is_err() {
                println!(
                    "Cannot load unified worker {} state (rkyv). Error: {:?}",
                    worker_id,
                    file.err()
                );
                return;
            }

            let file = file.unwrap();
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            let helper: Vec<UnifiedPrivateCacheHelper> =
                rkyv::from_bytes::<Vec<UnifiedPrivateCacheHelper>, rkyv::rancor::Error>(&bytes)
                    .unwrap();

            for (cache, helper) in self.caches[begin..end].iter_mut().zip(helper.into_iter()) {
                *cache = UnifiedPerCorePrivateCache::from_checkpoint_helper(helper);
            }
        } else {
            let file = std::fs::File::open(format!(
                "{}/unified-{}-worker-{}.json.zstd",
                name, numa_node_id, worker_id
            ));

            if file.is_err() {
                println!(
                    "Cannot load unified worker {} state. Error: {:?}",
                    worker_id,
                    file.err()
                );
                return;
            }

            let file = file.unwrap();
            let decoder = Decoder::new(file).unwrap();

            let helper: Vec<UnifiedPrivateCacheHelper> = serde_json::from_reader(decoder).unwrap();

            for (cache, helper) in self.caches[begin..end].iter_mut().zip(helper.into_iter()) {
                *cache = UnifiedPerCorePrivateCache::from_checkpoint_helper(helper);
            }
        }
    }
}

pub type ParallelUnifiedPrivateCache<const CORE_COUNT: usize, const SET: usize, const ASSO: usize> =
    UnifiedPrivateCaches<CORE_COUNT, SET, ASSO>;
