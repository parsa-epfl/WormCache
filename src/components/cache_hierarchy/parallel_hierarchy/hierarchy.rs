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

use zstd::{Decoder, Encoder};

use crate::parameter;

use crate::components::debug::statistics::{EventType, Statistics};

use crate::components::debug::cache_line_history::{CacheLineCoherenceHistory, CacheOperationType};

use super::super::common::{Directory, DirectorySet, PrivateCaches, SharedCache};

use crate::components::mmu::AbstractMMU;
use std::cell::UnsafeCell;
use std::ops::DerefMut;

#[cfg(test)]
mod debug_tests;
#[cfg(test)]
mod harvard_reverse_order_tests;
#[cfg(test)]
mod harvard_tests;
#[cfg(test)]
mod reverse_order_tests;

mod access_logic;

pub struct ParallelMemoryHierarchy<
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
    directory: Directory<DIRECTORY_SHARD_COUNT>,

    shared_cache: SCache,
    with_statistics: bool,
    directory_run_gc: bool,
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
    ParallelMemoryHierarchy<
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
    pub fn new(with_statistics: bool, _quantum_size: u64, directory_run_gc: bool) -> Self {
        Self {
            mmus: std::array::from_fn(|_| UnsafeCell::new(MMU::new())),
            private_caches: PCache::new(),
            directory: Directory::new(),
            shared_cache: SCache::new(),
            with_statistics,
            directory_run_gc,
        }
    }

    pub fn handle_eviction<const SET: usize>(
        &self,
        directory_set_guard: &mut impl DerefMut<Target = DirectorySet<SET>>,
        cache_id: usize,
        block_id: u64,
        ts: u64,
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
                    .insert(core_id, block_id, ts, modified.0, true);
            }

            if FILL_SCACHE_ON_PCACHE_WRITEBACK && modified.0 {
                self.shared_cache
                    .insert(core_id, block_id, ts, modified.0, true);
            }

            if self.directory_run_gc {
                // run GC here to clean this directory entry.
                directory_set_guard.erase(block_id);
            }
        }
    }

    pub fn dump_access_counter(&self) {
        // self.shared_cache.dump_access_counter();
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
}
