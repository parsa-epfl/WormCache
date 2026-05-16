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

use rustc_hash::FxHashMap as HashMap;
use spin::mutex::{SpinMutex, SpinMutexGuard};
use zstd::{Decoder, Encoder};

use crate::util;

use super::{Directory, DirectoryEntry, DirectoryHelper, DirectorySet, SharerList};

#[derive(Debug, Clone)]
#[repr(align(64))]
pub struct InfiniteDirectorySet<const SET: usize> {
    entries: HashMap<u64, DirectoryEntry>,
    pub index: usize,
}

impl<const SET: usize> DirectorySet for InfiniteDirectorySet<SET> {
    fn new(index: usize) -> Self {
        Self {
            entries: HashMap::<u64, DirectoryEntry>::default(),
            index,
        }
    }

    fn from(raw: HashMap<u64, DirectoryEntry>, index: usize) -> Self {
        Self {
            entries: raw,
            index,
        }
    }

    #[inline]
    fn get_or_create(
        &mut self,
        block_id: u64,
    ) -> (&mut DirectoryEntry, Option<(u64, DirectoryEntry)>) {
        self.entries.entry(block_id).or_insert(DirectoryEntry {
            lru_ts: 0,
            sharers: SharerList::ZERO,
            in_shared_cache: false,
            shared: false,
        });

        (self.entries.get_mut(&block_id).unwrap(), None)
    }

    fn get(&mut self, block_id: u64) -> Option<&mut DirectoryEntry> {
        self.entries.get_mut(&block_id)
    }

    fn erase(&mut self, block_id: u64) {
        self.entries.remove(&block_id);
    }

    fn run_gc(&mut self) {
        // clean all entries that has zero sharers.
        self.entries.retain(|_, entry| entry.sharers.any());
    }

    fn raw(&self) -> HashMap<u64, DirectoryEntry> {
        self.entries.clone()
    }
}

pub struct InfiniteDirectory<const SET: usize> {
    entries: Box<[SpinMutex<InfiniteDirectorySet<SET>>; SET]>,
}

impl<const SET: usize> InfiniteDirectory<SET> {
    fn to_checkpoint_helper(&self) -> DirectoryHelper {
        let entries: Vec<HashMap<u64, DirectoryEntry>> = self
            .entries
            .iter()
            .map(|set| set.lock().clone().raw())
            .collect::<Vec<_>>();

        DirectoryHelper::from_sets(&entries)
    }

    fn from_checkpoint_helper(helper: DirectoryHelper) -> Self {
        let entries = helper
            .into_sets()
            .into_iter()
            .enumerate()
            .map(|(idx, set)| {
                SpinMutex::new(<InfiniteDirectorySet<SET> as super::DirectorySet>::from(
                    set, idx,
                ))
            })
            .collect::<Vec<_>>();

        Self {
            entries: entries.try_into().unwrap(),
        }
    }
}

impl<const SET: usize> Default for InfiniteDirectory<SET> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, const SET: usize> Directory for InfiniteDirectory<SET> {
    type TSet = InfiniteDirectorySet<SET>;

    fn new() -> Self {
        Self {
            entries: util::init_heap_array(|idx| SpinMutex::new(InfiniteDirectorySet::new(idx))),
        }
    }

    fn fetch_one_entry(&self, block_id: u64) -> SpinMutexGuard<'_, Self::TSet> {
        let set_id = (block_id as usize) % SET;
        self.entries[set_id].lock()
    }

    fn fetch_two_entries(
        &self,
        block_id_0: u64,
        block_id_1: u64,
    ) -> (
        SpinMutexGuard<'_, Self::TSet>,
        Option<SpinMutexGuard<'_, Self::TSet>>,
    ) {
        let index_0 = (block_id_0 as usize) % SET;
        let index_1 = (block_id_1 as usize) % SET;

        match index_0.cmp(&index_1) {
            std::cmp::Ordering::Equal => (self.fetch_one_entry(block_id_0), None),
            std::cmp::Ordering::Less => {
                let g0 = self.fetch_one_entry(block_id_0);
                let g1 = self.fetch_one_entry(block_id_1);
                (g0, Some(g1))
            }
            std::cmp::Ordering::Greater => {
                let g1 = self.fetch_one_entry(block_id_1);
                let g0 = self.fetch_one_entry(block_id_0);
                (g0, Some(g1))
            }
        }
    }

    fn run_gc(&self) {
        // clean all entries that has zero sharers.
        for set in self.entries.iter() {
            let mut set = set.lock();
            set.run_gc();
        }
    }

    fn serialize(&self, name: &str, numa_node_id: usize) {
        use crate::parameter::USE_RKYV_SERIALIZATION;

        self.run_gc();

        let helper = self.to_checkpoint_helper();

        if USE_RKYV_SERIALIZATION {
            let file =
                std::fs::File::create(format!("{}/directory-{}.rkyv.zstd", name, numa_node_id))
                    .unwrap();

            let mut encoder = Encoder::new(file, 0).unwrap();
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
            std::io::Write::write_all(&mut encoder, &bytes).unwrap();
            encoder.finish().unwrap();
        } else {
            let file =
                std::fs::File::create(format!("{}/directory-{}.json.zstd", name, numa_node_id))
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
                std::fs::File::open(format!("{}/directory-{}.rkyv.zstd", name, numa_node_id));

            if file.is_err() {
                println!(
                    "Cannot load the directory state (rkyv). Error: {:?}",
                    file.err()
                );
                return;
            }

            let file = file.unwrap();
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            let helper: DirectoryHelper =
                rkyv::from_bytes::<DirectoryHelper, rkyv::rancor::Error>(&bytes).unwrap();
            *self = Self::from_checkpoint_helper(helper);
        } else {
            let file =
                std::fs::File::open(format!("{}/directory-{}.json.zstd", name, numa_node_id));

            if file.is_err() {
                return;
            }

            let file = file.unwrap();

            let file = Decoder::new(file).unwrap();

            let helper: DirectoryHelper = serde_json::from_reader(file).unwrap();
            *self = Self::from_checkpoint_helper(helper);
        }
    }

    fn serialize_shard(&self, shard_id: usize, name: &str, numa_node_id: usize) {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, USE_RKYV_SERIALIZATION};

        let total_shards = CHECKPOINT_POOL_SIZE;
        let sets_per_shard = SET / total_shards;
        let begin = shard_id * sets_per_shard;
        let end = if shard_id + 1 == total_shards { SET } else { begin + sets_per_shard };

        for i in begin..end {
            self.entries[i].lock().run_gc();
        }

        let entries_in_shard: Vec<HashMap<u64, DirectoryEntry>> = self.entries[begin..end]
            .iter()
            .map(|set| set.lock().clone().raw())
            .collect();

        let helper = DirectoryHelper::from_sets(&entries_in_shard);

        if USE_RKYV_SERIALIZATION {
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
            crate::util::write_compressed(
                &format!("{}/directory-{}-shard-{}.rkyv.zstd", name, numa_node_id, shard_id),
                &bytes,
            );
        } else {
            let bytes = serde_json::to_vec(&helper).unwrap();
            crate::util::write_compressed(
                &format!("{}/directory-{}-shard-{}.json.zstd", name, numa_node_id, shard_id),
                &bytes,
            );
        }
    }

    fn deserialize_shard(&mut self, shard_id: usize, name: &str, numa_node_id: usize) -> bool {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, USE_RKYV_SERIALIZATION};

        let total_shards = CHECKPOINT_POOL_SIZE;
        let sets_per_shard = SET / total_shards;
        let begin = shard_id * sets_per_shard;
        let _end = if shard_id + 1 == total_shards { SET } else { begin + sets_per_shard };

        if USE_RKYV_SERIALIZATION {
            let file = std::fs::File::open(format!(
                "{}/directory-{}-shard-{}.rkyv.zstd",
                name, numa_node_id, shard_id
            ));

            if file.is_err() {
                return false;
            }

            let file = file.unwrap();
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            let helper: DirectoryHelper =
                rkyv::from_bytes::<DirectoryHelper, rkyv::rancor::Error>(&bytes).unwrap();

            let sets = helper.into_sets();
            for (i, set) in sets.into_iter().enumerate() {
                self.entries[begin + i] =
                    SpinMutex::new(<InfiniteDirectorySet<SET> as DirectorySet>::from(
                        set,
                        begin + i,
                    ));
            }
        } else {
            let file = std::fs::File::open(format!(
                "{}/directory-{}-shard-{}.json.zstd",
                name, numa_node_id, shard_id
            ));

            if file.is_err() {
                return false;
            }

            let file = file.unwrap();
            let decoder = Decoder::new(file).unwrap();

            let helper: DirectoryHelper = serde_json::from_reader(decoder).unwrap();

            let sets = helper.into_sets();
            for (i, set) in sets.into_iter().enumerate() {
                self.entries[begin + i] =
                    SpinMutex::new(<InfiniteDirectorySet<SET> as DirectorySet>::from(
                        set,
                        begin + i,
                    ));
            }
        }
        true
    }

    fn information() -> String {
        format!("Infinite Directory with {} shard(s)", SET)
    }
}
