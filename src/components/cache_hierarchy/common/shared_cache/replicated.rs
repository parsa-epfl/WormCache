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

use crate::components::cache_hierarchy::CacheBlockRequest;

use super::{
    SharedCache, SharedCacheLookupAndInsertResult, SharedCacheLookupResult, SharedCacheSet,
    statistics::SharedCacheSetStatistics,
};

use std::cell::UnsafeCell;

impl<
    S: SharedCacheSetStatistics,
    const WAY: usize,
    const SET: usize,
    const EXCLUSIVE: bool,
    const PRECISE_TS: bool,
> SharedCacheSet<WAY, SET, EXCLUSIVE, PRECISE_TS, S>
{
    fn _fold(&self, other: &Self) -> Self {
        // take the two arrays, combine them, and sort them by the timestamp. Only keel the elements with highest timestamp.
        let mut imm: Vec<_> = self.blocks.iter().chain(other.blocks.iter()).collect();

        // keep the elements with the highest timestamp.
        imm.sort_by(|a, b| b.ts.cmp(&a.ts));

        // keep the first WAY elements.
        imm.truncate(WAY);

        // keep the first WAY elements.
        Self {
            blocks: std::array::from_fn(|i| (*imm[i]).clone()),
            touched_count: usize::min(self.touched_count + other.touched_count, WAY),
            recent_evict_ts: 0,

            access_count: self.access_count + other.access_count,

            // modifying_history: vec![],

            // clean the statistics
            statistics: Default::default(),
        }
    }
}

struct PrivateSharedCache<
    S: SharedCacheSetStatistics,
    const SET: usize,
    const WAY: usize,
    const EXCLUSIVE: bool,
    const PRECISE_TS: bool,
> {
    blocks: Box<[SharedCacheSet<WAY, SET, EXCLUSIVE, PRECISE_TS, S>; SET]>,
}

impl<
    S: SharedCacheSetStatistics,
    const SET: usize,
    const WAY: usize,
    const EXCLUSIVE: bool,
    const PRECISE_TS: bool,
> PrivateSharedCache<S, SET, WAY, EXCLUSIVE, PRECISE_TS>
{
    fn new() -> Self {
        Self {
            blocks: crate::util::init_heap_array(|_| SharedCacheSet::new()),
        }
    }

    fn invalidate(&mut self, block_id: u64, ts: u64, core_id: u32) -> SharedCacheLookupResult {
        let set_idx = (block_id % SET as u64) as usize;
        self.blocks[set_idx].invalidate(block_id, ts, core_id)
    }

    fn lookup(&mut self, r: &CacheBlockRequest, ts: u64) -> SharedCacheLookupResult {
        let set_idx = (r.block_id % SET as u64) as usize;
        self.blocks[set_idx].lookup(r, ts)
    }

    fn insert(
        &mut self,
        block_id: u64,
        core_id: u32,
        ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    ) {
        let set_idx = (block_id % SET as u64) as usize;
        self.blocks[set_idx].insert(block_id, core_id, ts, is_modified, increase_touched_count);
    }

    fn lookup_and_insert(
        &mut self,
        r: &CacheBlockRequest,
        ts: u64,
        increase_touched_count: bool,
    ) -> SharedCacheLookupResult {
        let set_idx = (r.block_id % SET as u64) as usize;
        let res = self.blocks[set_idx].lookup_and_insert(r, ts, increase_touched_count);
        match res {
            SharedCacheLookupAndInsertResult::Hit(modified) => {
                SharedCacheLookupResult::Hit(modified)
            }
            SharedCacheLookupAndInsertResult::Miss => SharedCacheLookupResult::Miss,
            SharedCacheLookupAndInsertResult::InsertedAndCold(_) => {
                SharedCacheLookupResult::ColdMiss
            }
            SharedCacheLookupAndInsertResult::Inserted => SharedCacheLookupResult::Miss,
            SharedCacheLookupAndInsertResult::Unknown(unknown, is_modified) => {
                SharedCacheLookupResult::Unknown(unknown, is_modified)
            }
        }
    }

    // fn dump_snapshot(&self, snapshot_name: &str) {
    //     let mut file =
    //         std::fs::File::create(format!("{}/shared_cache.json", snapshot_name)).unwrap();

    //     let log2_set = SET.trailing_zeros();

    //     let entries = self
    //         .blocks
    //         .iter()
    //         .map(|entry| {
    //             let mut sorted_lines: Vec<_> = entry.blocks.iter().collect();
    //             sorted_lines.sort_by(|a, b| a.ts.cmp(&b.ts));

    //             sorted_lines
    //                 .iter()
    //                 .filter_map(|block| {
    //                     if block.block_id_with_v & 1 == 0 {
    //                         return None;
    //                     }
    //                     Some(SerializedSharedCacheBlock {
    //                         tag: (block.block_id_with_v >> 1) >> log2_set,
    //                         dirty: block.modified,
    //                         writable: true,
    //                     })
    //                 })
    //                 .collect::<Vec<_>>()
    //         })
    //         .collect::<Vec<_>>();

    //     serde_json::to_writer(
    //         &mut file,
    //         &json!({
    //             "associativity": WAY,
    //             "tags": entries,
    //         }),
    //     )
    //     .unwrap();
    // }

    fn _fold(&self, other: &Self) -> Self {
        return Self {
            blocks: self
                .blocks
                .iter()
                .zip(other.blocks.iter())
                .map(|(a, b)| a._fold(b))
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        };
    }
}

pub struct ReplicatedSharedCache<
    S: SharedCacheSetStatistics,
    const CORE_COUNT: usize,
    const SET: usize,
    const WAY: usize,
    const EXCLUSIVE: bool,
    const COHERENCE_FOLLOW_TS: bool,
> {
    blocks:
        [UnsafeCell<PrivateSharedCache<S, SET, WAY, EXCLUSIVE, COHERENCE_FOLLOW_TS>>; CORE_COUNT],
}

impl<
    S: SharedCacheSetStatistics,
    const CORE_COUNT: usize,
    const SET: usize,
    const WAY: usize,
    const EXCLUSIVE: bool,
    const COHERENCE_FOLLOW_TS: bool,
> SharedCache for ReplicatedSharedCache<S, CORE_COUNT, SET, WAY, EXCLUSIVE, COHERENCE_FOLLOW_TS>
{
    fn new() -> Self {
        Self {
            blocks: std::array::from_fn(|_| UnsafeCell::new(PrivateSharedCache::new())),
        }
    }

    fn peek(&self, request: &CacheBlockRequest) -> bool {
        let pcache = unsafe { &mut *self.blocks[request.core_id as usize].get() };
        let set_idx = (request.block_id % SET as u64) as usize;
        pcache.blocks[set_idx].index_of(request.block_id).is_some()
    }

    fn invalidate(&self, core_id: u32, block_id: u64, ts: u64) -> SharedCacheLookupResult {
        let pcache = unsafe { &mut *self.blocks[core_id as usize].get() };
        pcache.invalidate(block_id, ts, core_id)
    }

    fn lookup(&self, request: &CacheBlockRequest, ts: u64) -> SharedCacheLookupResult {
        let core_id = request.core_id;

        let pcache = unsafe { &mut *self.blocks[core_id as usize].get() };
        pcache.lookup(request, ts)
    }

    fn insert(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    ) {
        let pcache = unsafe { &mut *self.blocks[core_id as usize].get() };
        pcache.insert(block_id, core_id, ts, is_modified, increase_touched_count);
    }

    fn lookup_and_insert_on_miss(
        &self,
        request: &CacheBlockRequest,
        ts: u64,
        increase_touched_count: bool,
    ) -> SharedCacheLookupResult {
        let pcache = unsafe { &mut *self.blocks[request.core_id as usize].get() };

        pcache.lookup_and_insert(request, ts, increase_touched_count)
    }

    fn warmed_sets_count(&self) -> usize {
        0
    }

    fn warmed_slots_count(&self) -> usize {
        0
    }

    fn information() -> String {
        format!(
            "ReplicatedSharedCache: SET={}, WAY={}, EXCLUSIVE={}",
            SET, WAY, EXCLUSIVE
        )
    }

    fn dump_access_frequency(&self, _: &str) {}

    fn serialize(&self, _: &str, _: usize) {
        unimplemented!()
    }

    fn deserialize(&mut self, _: &str, _: usize) {
        unimplemented!()
    }
}
