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

use core::panic;

use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::components::{
    cache_hierarchy::CacheBlockRequest, debug::cache_line_history::CacheLineCoherenceHistory,
};

use super::{
    statistics::{SharedCacheSetStatistics, ZeroSharedCacheSetStatistics},
    SharedCacheLookupAndInsertResult, SharedCacheLookupResult,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedCacheBlock {
    pub block_id_with_v: u64, // the last bit is the valid bit.
    pub ts: u64,
    pub modified: bool,
    pub last_accessor: u32, // the last accessor of this cache line.
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SharedCacheSet<
    const WAY: usize,
    const SET: usize,
    const EXCLUSIVE: bool,
    S: SharedCacheSetStatistics,
> {
    #[serde_as(as = "[_; WAY]")]
    pub blocks: [SharedCacheBlock; WAY],
    pub touched_count: usize,
    pub recent_evict_ts: u64, // if a cache access has a timestamp less than this one, its result might be unknown if there is a hit.

    pub access_count: u64,

    pub statistics: S,
}

impl<const WAY: usize, const SET: usize, const EXCLUSIVE: bool, S: SharedCacheSetStatistics> Default
    for SharedCacheSet<WAY, SET, EXCLUSIVE, S>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const WAY: usize, const SET: usize, const EXCLUSIVE: bool, S: SharedCacheSetStatistics>
    SharedCacheSet<WAY, SET, EXCLUSIVE, S>
{
    pub fn new() -> Self {
        Self {
            blocks: std::array::from_fn(|_| SharedCacheBlock {
                block_id_with_v: 0,
                ts: 0,
                modified: false,
                last_accessor: 0,
            }),

            touched_count: 0,
            recent_evict_ts: 0,

            access_count: 0,

            statistics: S::default(),
        }
    }

    pub fn from_without_statistics(
        other: SharedCacheSet<WAY, SET, EXCLUSIVE, ZeroSharedCacheSetStatistics>,
    ) -> Self {
        Self {
            blocks: other.blocks,
            touched_count: other.touched_count,
            recent_evict_ts: other.recent_evict_ts,
            access_count: other.access_count,
            statistics: S::default(),
        }
    }

    pub fn without_statistics(
        &self,
    ) -> SharedCacheSet<WAY, SET, EXCLUSIVE, ZeroSharedCacheSetStatistics> {
        SharedCacheSet {
            blocks: self.blocks.clone(),
            touched_count: self.touched_count,
            recent_evict_ts: self.recent_evict_ts,
            access_count: self.access_count,
            statistics: ZeroSharedCacheSetStatistics {},
        }
    }

    // Return whether the block is a hit.
    #[inline]
    pub fn index_of(&self, block_id: u64) -> Option<usize> {
        let internal_block_id = block_id << 1 | 1;
        self.blocks
            .iter()
            .position(|p| p.block_id_with_v == internal_block_id)
    }

    pub fn invalidate(&mut self, block_id: u64, ts: u64) -> bool {
        // if it is a hit, we remove this block from the cache
        if let Some(hit_block) = self.index_of(block_id) {
            let hit_block = &mut self.blocks[hit_block];
            let res = Some(hit_block.modified);
            hit_block.block_id_with_v = 0;
            hit_block.ts = 0;

            if self.recent_evict_ts < ts {
                self.recent_evict_ts = ts;
            }

            return true;
        }

        // otherwise, it is a miss.
        false
    }

    #[inline]
    pub fn lookup(&mut self, r: &CacheBlockRequest, ts: u64) -> SharedCacheLookupResult {
        self.access_count += 1;

        let block_id = r.block_id;
        let core_id = r.core_id;
        let access_type = r.access_type.clone();
        let is_os = r.is_os();

        match if EXCLUSIVE {
            self.invalidate(block_id, ts)
        } else {
            if let Some(hit_block) = self.index_of(block_id) {
                let hit_block = &mut self.blocks[hit_block];
                if r.is_store() {
                    // The cache line should be transferred to the accessor.
                    hit_block.block_id_with_v = 0;
                    hit_block.ts = 0;

                    if self.recent_evict_ts < ts {
                        self.recent_evict_ts = ts;
                    }
                } else {
                    if ts >= hit_block.ts {
                        // the equal case is only about page walk, which enables touching multiple cache lines with the same timestamp.
                        hit_block.ts = ts;
                    }

                    hit_block.last_accessor = core_id;

                    // Accessed by a higher-level, meaning that the block is not dirty anymore.
                    hit_block.modified = false;
                }
                true
            } else {
                false
            }
        } {
            true => {
                self.statistics.record(access_type, is_os, true);
                SharedCacheLookupResult::Hit
            }
            false => {
                if ts < self.recent_evict_ts {
                    SharedCacheLookupResult::Unknown((self.recent_evict_ts - ts) as u32)
                } else {
                    self.statistics.record(access_type, is_os, false);
                    if self.touched_count < WAY {
                        SharedCacheLookupResult::ColdMiss
                    } else {
                        SharedCacheLookupResult::Miss
                    }
                }
            }
        }
    }

    // return whether this cache set is just warmed.
    #[inline]
    pub fn insert(
        &mut self,
        block_id: u64,
        core_id: u32,
        ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    ) -> bool {
        assert!(ts != 0); // ts should not be 0. 0 is reserved for invalid blocks.

        if let Some(hit_block) = self.index_of(block_id) {
            let hit_block = &mut self.blocks[hit_block];
            if EXCLUSIVE {
                // this should not happen for exclusive caches
                CacheLineCoherenceHistory::global_get_block_history(block_id)
                    .unwrap()
                    .print_history();
                panic!("Error: the incoming block is already in the shared cache.");
            } else {
                // update the timestamp and the modified bit.
                if ts > hit_block.ts {
                    hit_block.ts = ts;
                }

                if is_modified {
                    // This cache line's permission should have been taken by the private cache.
                    // So it should not be modified in the shared cache.
                    assert!(!hit_block.modified);
                    hit_block.modified = is_modified;
                }

                hit_block.last_accessor = core_id;

                return false;
            }
        }

        let result = if increase_touched_count && self.touched_count < WAY {
            self.touched_count += 1;
            self.touched_count == WAY
        } else {
            false
        };

        let block_id_with_v = block_id << 1 | 1;

        // TODO: This part can be accelerated using SIMD instructions.
        let mut minimal_ts = u64::MAX;
        let mut minimal_index = 0;

        for (index, line) in self.blocks.iter_mut().enumerate() {
            if line.ts < minimal_ts {
                minimal_ts = line.ts;
                minimal_index = index;
            }
        }

        let oldest_block = &mut self.blocks[minimal_index];

        // if the oldest block even has larger timestamp than the incoming block, we should print a log and do nothing.
        if oldest_block.ts > ts {
            return result;
        }

        // otherwise, we replace the oldest block.
        oldest_block.block_id_with_v = block_id_with_v;
        oldest_block.modified = is_modified;
        oldest_block.ts = ts;
        oldest_block.last_accessor = core_id;

        // update the eviction counter.
        if self.recent_evict_ts < ts {
            self.recent_evict_ts = ts;
        }

        // push the evicted line to the history.
        result
    }

    #[inline]
    pub fn lookup_and_insert(
        &mut self,
        r: &CacheBlockRequest,
        ts: u64,
        increase_touched_count: bool,
    ) -> SharedCacheLookupAndInsertResult {
        // (is_hit, dirty/just_warmed)
        let result = self.lookup(r, ts);

        let block_id = r.block_id;
        let core_id = r.core_id;

        match result {
            SharedCacheLookupResult::Hit => SharedCacheLookupAndInsertResult::Hit,
            SharedCacheLookupResult::Miss => {
                // This function is only called when the private cache has a miss
                // Therefore, we cannot insert a modified block here, because the write permission should have been taken by the private cache.
                if !r.is_store() {
                    let just_warmed = self.insert(block_id, core_id, ts, false, increase_touched_count);
                    assert!(!just_warmed);    
                    SharedCacheLookupAndInsertResult::Inserted
                } else {
                    SharedCacheLookupAndInsertResult::Miss
                }
            }
            SharedCacheLookupResult::ColdMiss => {
                // This function is only called when the private cache has a miss
                // Therefore, we cannot insert a modified block here, because the write permission should have been taken by the private cache.

                let just_warmed = self.insert(block_id, core_id, ts, false, increase_touched_count);
                SharedCacheLookupAndInsertResult::InsertedAndCold(just_warmed)
            }
            SharedCacheLookupResult::Unknown(diff) => {
                SharedCacheLookupAndInsertResult::Unknown(diff)
            }
        }
    }
}

#[test]
fn minimum_can_find_invalid() {
    use super::statistics::ZeroSharedCacheSetStatistics;

    let mut set = SharedCacheSet::<8, 1, false, ZeroSharedCacheSetStatistics>::new();
    let mut ts = 1;

    // push 8 elements inside.
    for i in 0..8 {
        assert_eq!(set.insert(i, 0, ts, false, true), i == 7);
        ts += 1;
    }

    // now, we invalid set 0.
    assert_eq!(set.invalidate(0, ts), true);
    ts += 1;

    // Now if we refill, we will hit the first place.
    set.insert(9, 0, ts, false, true);

    // And the cache line 0 should be replaced.
    assert_eq!(
        set.blocks[0],
        SharedCacheBlock {
            block_id_with_v: 9 << 1 | 1,
            ts: ts,
            modified: false,
            last_accessor: 0
        }
    );

    ts += 1;

    // If we now insert another one, line[1] will be replaced.
    assert_eq!(set.insert(10, 0, ts, false, true), false);

    assert_eq!(
        set.blocks[1],
        SharedCacheBlock {
            block_id_with_v: 10 << 1 | 1,
            ts: ts,
            modified: false,
            last_accessor: 0
        }
    )
}

#[test]
fn cold_miss_exist() {
    use super::statistics::ZeroSharedCacheSetStatistics;
    use crate::components::cache_hierarchy::common::CacheAccessType;

    let mut set = SharedCacheSet::<8, 1, false, ZeroSharedCacheSetStatistics>::new();

    let mut ts = 1;

    // do one lookup. It will return the cold.
    assert_eq!(
        set.lookup(
            &CacheBlockRequest {
                block_id: 10,
                core_id: 0,
                access_type: CacheAccessType::DataRead,
                is_os: false
            },
            ts,
        ),
        SharedCacheLookupResult::ColdMiss,
    );

    ts += 1;

    // fill the cache.
    for i in 0..8 {
        assert_eq!(set.insert(i + 10, 0, ts, false, true), i == 7);
        ts += 1;
    }

    // do a lookup that triggers a miss. Now it should be a miss instead of a cold miss.
    assert_eq!(
        set.lookup(
            &CacheBlockRequest {
                block_id: 20,
                core_id: 0,
                access_type: CacheAccessType::DataRead,
                is_os: false
            },
            ts,
        ),
        SharedCacheLookupResult::Miss,
    );
}
