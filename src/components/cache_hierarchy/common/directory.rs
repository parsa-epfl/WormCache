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

use std::fmt;

use rustc_hash::FxHashMap as HashMap;
use serde::Deserialize;
use serde_with::serde_as;
use spin::mutex::SpinMutex;
use spin::mutex::SpinMutexGuard;

use bitvec::BitArr;
use bitvec::prelude::*;
use serde::Serialize;
use zstd::{Decoder, Encoder};

use crate::parameter;
use crate::util;

const SHARED_LIST_LENGTH: usize = if parameter::USE_UNIFIED_CACHE {
    parameter::CORE_COUNT
} else {
    parameter::CORE_COUNT * 2
};

pub type SharerList = BitArr!(for SHARED_LIST_LENGTH, in u64, Lsb0);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirectoryEntry {
    pub lru_ts: u64,
    pub sharers: SharerList,
    pub in_shared_cache: bool,

    pub shared: bool,
}

impl DirectoryEntry {
    #[inline]
    pub fn update_lru_ts(&mut self, ts: u64) {
        if ts > self.lru_ts {
            self.lru_ts = ts;
        }
    }
}

pub trait DirectorySet {
    fn new(index: usize) -> Self;
    fn from(raw: HashMap<u64, DirectoryEntry>, index: usize) -> Self;
    const LOG2_SET: usize;
    fn get_or_create(
        &mut self,
        block_id: u64,
    ) -> (&mut DirectoryEntry, Option<(u64, DirectoryEntry)>);

    fn get(&mut self, block_ud: u64) -> Option<&mut DirectoryEntry>;

    fn erase(&mut self, block_id: u64);
    fn run_gc(&mut self);

    fn raw(&self) -> HashMap<u64, DirectoryEntry>;
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

    const LOG2_SET: usize = SET.trailing_zeros() as usize;

    #[inline]
    fn get_or_create(
        &mut self,
        block_id: u64,
    ) -> (&mut DirectoryEntry, Option<(u64, DirectoryEntry)>) {
        let internal_id = block_id >> Self::LOG2_SET;

        self.entries.entry(internal_id).or_insert(DirectoryEntry {
            lru_ts: 0,
            sharers: SharerList::ZERO,
            in_shared_cache: false,
            shared: false,
        });

        (self.entries.get_mut(&internal_id).unwrap(), None)
    }

    fn get(&mut self, block_ud: u64) -> Option<&mut DirectoryEntry> {
        let internal_id = block_ud >> Self::LOG2_SET;
        self.entries.get_mut(&internal_id)
    }

    fn erase(&mut self, block_id: u64) {
        let internal_id = block_id >> Self::LOG2_SET;
        self.entries.remove(&internal_id);
    }

    fn run_gc(&mut self) {
        // clean all entries that has zero sharers.
        self.entries.retain(|_, entry| entry.sharers.any());
    }

    fn raw(&self) -> HashMap<u64, DirectoryEntry> {
        self.entries.clone()
    }
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(align(64))]
pub struct FiniteDirectorySet<const SET: usize, const WAY: usize> {
    entries: HashMap<u64, DirectoryEntry>,
    pub index: usize,
}

impl<const SET: usize, const WAY: usize> DirectorySet for FiniteDirectorySet<SET, WAY> {
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

    const LOG2_SET: usize = SET.trailing_zeros() as usize;

    fn get_or_create(
        &mut self,
        block_id: u64,
    ) -> (&mut DirectoryEntry, Option<(u64, DirectoryEntry)>) {
        // search for the block in the set.
        let internal_id = block_id >> Self::LOG2_SET;

        // if the block is not found, create a new entry.
        if !self.entries.contains_key(&internal_id) {
            // if the set is full, evict the LRU block.
            let evicted = if self.entries.len() == WAY {
                let lru_block = *self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.lru_ts)
                    .unwrap()
                    .0;

                self.entries
                    .remove(&lru_block)
                    .map(|entry| (lru_block << (Self::LOG2_SET + SET), entry))
            } else {
                None
            };

            self.entries.insert(
                internal_id,
                DirectoryEntry {
                    lru_ts: 0,
                    sharers: SharerList::ZERO,
                    in_shared_cache: false,
                    shared: false,
                },
            );

            return (self.entries.get_mut(&internal_id).unwrap(), evicted);
        }

        (self.entries.get_mut(&internal_id).unwrap(), None)
    }

    fn get(&mut self, block_ud: u64) -> Option<&mut DirectoryEntry> {
        let internal_id = block_ud >> Self::LOG2_SET;
        self.entries.get_mut(&internal_id)
    }

    fn erase(&mut self, block_id: u64) {
        let internal_id = block_id >> Self::LOG2_SET;
        self.entries.remove(&internal_id);
    }

    fn run_gc(&mut self) {
        // clean all entries that has zero sharers.
        self.entries.retain(|_, entry| entry.sharers.any());
    }

    fn raw(&self) -> HashMap<u64, DirectoryEntry> {
        self.entries.clone()
    }
}

// Probably the Directory should be infinitely sized.
pub struct Directory<TSet: DirectorySet, const SET: usize> {
    entries: Box<[SpinMutex<TSet>; SET]>,
}

#[derive(Serialize, Deserialize)]
struct DirectorySerdeHelper<const SET: usize> {
    entries: Vec<HashMap<u64, DirectoryEntry>>,
}

impl<'a, TSet: DirectorySet + fmt::Debug + Serialize + Deserialize<'a> + Clone, const SET: usize>
    Default for Directory<TSet, SET>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, TSet: DirectorySet + fmt::Debug + Serialize + Deserialize<'a> + Clone, const SET: usize>
    Directory<TSet, SET>
{
    pub fn new() -> Self {
        Self {
            entries: util::init_heap_array(|idx| SpinMutex::new(TSet::new(idx))),
        }
    }

    pub fn fetch_one_entry(&self, block_id: u64) -> SpinMutexGuard<'_, TSet> {
        let set_id = (block_id as usize) % SET;
        self.entries[set_id].lock()
    }

    pub fn fetch_two_entries(
        &self,
        block_id_0: u64,
        block_id_1: u64,
    ) -> (SpinMutexGuard<'_, TSet>, Option<SpinMutexGuard<'_, TSet>>) {
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

    pub fn run_gc(&self) {
        // clean all entries that has zero sharers.
        for set in self.entries.iter() {
            let mut set = set.lock();
            set.run_gc();
        }
    }

    fn to_serialize_helper(&self) -> DirectorySerdeHelper<SET> {
        let entries: Vec<HashMap<u64, DirectoryEntry>> = self
            .entries
            .iter()
            .map(|set| set.lock().clone().raw())
            .collect::<Vec<_>>();

        DirectorySerdeHelper { entries }
    }

    fn from_serialize_helper(helper: DirectorySerdeHelper<SET>) -> Self {
        let entries = helper
            .entries
            .into_iter()
            .enumerate()
            .map(|(idx, set)| SpinMutex::new(TSet::from(set, idx)))
            .collect::<Vec<_>>();

        Self {
            entries: entries.try_into().unwrap(),
        }
    }

    pub fn serialize(&self, name: &str, numa_node_id: usize) {
        self.run_gc();
        let file = std::fs::File::create(format!("{}/directory-{}.json.zstd", name, numa_node_id))
            .unwrap();

        let mut file = Encoder::new(file, 0).unwrap();

        let helper = self.to_serialize_helper();
        serde_json::to_writer(&mut file, &helper).unwrap();

        file.finish().unwrap();
    }

    pub fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        let file = std::fs::File::open(format!("{}/directory-{}.json.zstd", name, numa_node_id));

        if file.is_err() {
            println!("Cannot load the directory state. Error: {:?}", file.err());
            return;
        }

        let file = file.unwrap();

        let file = Decoder::new(file).unwrap();

        let helper: DirectorySerdeHelper<SET> = serde_json::from_reader(file).unwrap();
        *self = Self::from_serialize_helper(helper);
    }
}
