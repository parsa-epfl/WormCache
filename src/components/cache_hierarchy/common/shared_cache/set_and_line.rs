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
    SharedCacheLookupAndInsertResult, SharedCacheLookupResult,
    statistics::{SharedCacheSetStatistics, ZeroSharedCacheSetStatistics},
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
    const COHERENCE_FOLLOW_TS: bool,
    S: SharedCacheSetStatistics,
> {
    #[serde_as(as = "[_; WAY]")]
    pub blocks: [SharedCacheBlock; WAY],
    pub touched_count: usize,
    pub recent_evict_ts: u64, // if a cache access has a timestamp less than this one, its result might be unknown if there is a hit.

    pub access_count: u64,

    pub statistics: S,
    // #[serde(skip)]
    // pub modifying_history: Vec<(u64, u64, u32, bool, bool)>, // (block_id, ts, core_id, to_what, succeed), recorded on a cache line's modified state is updated.
}

impl<
    const WAY: usize,
    const SET: usize,
    const EXCLUSIVE: bool,
    const COHERENCE_FOLLOW_TS: bool,
    S: SharedCacheSetStatistics,
> Default for SharedCacheSet<WAY, SET, EXCLUSIVE, COHERENCE_FOLLOW_TS, S>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<
    const WAY: usize,
    const SET: usize,
    const EXCLUSIVE: bool,
    const COHERENCE_FOLLOW_TS: bool,
    S: SharedCacheSetStatistics,
> SharedCacheSet<WAY, SET, EXCLUSIVE, COHERENCE_FOLLOW_TS, S>
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

            // modifying_history: vec![],
            statistics: S::default(),
        }
    }

    pub fn from_without_statistics(
        other: SharedCacheSet<
            WAY,
            SET,
            EXCLUSIVE,
            COHERENCE_FOLLOW_TS,
            ZeroSharedCacheSetStatistics,
        >,
    ) -> Self {
        Self {
            blocks: other.blocks,
            touched_count: other.touched_count,
            recent_evict_ts: other.recent_evict_ts,
            access_count: other.access_count,
            // modifying_history: vec![],
            statistics: S::default(),
        }
    }

    pub fn without_statistics(
        &self,
    ) -> SharedCacheSet<WAY, SET, EXCLUSIVE, COHERENCE_FOLLOW_TS, ZeroSharedCacheSetStatistics>
    {
        SharedCacheSet {
            blocks: self.blocks.clone(),
            touched_count: self.touched_count,
            recent_evict_ts: self.recent_evict_ts,
            access_count: self.access_count,
            // modifying_history: vec![],
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

    pub fn invalidate(&mut self, block_id: u64, ts: u64, _core_id: u32) -> SharedCacheLookupResult {
        // if it is a hit, we remove this block from the cache
        if let Some(hit_block) = self.index_of(block_id) {
            if self.recent_evict_ts < ts {
                self.recent_evict_ts = ts;
            }

            let hit_block = &mut self.blocks[hit_block];

            return if hit_block.ts <= ts || !COHERENCE_FOLLOW_TS {
                hit_block.block_id_with_v = 0;
                hit_block.ts = 0;

                // self.modifying_history
                //     .push((block_id, ts, core_id, false, true));

                SharedCacheLookupResult::Hit(hit_block.modified)
            } else {
                // self.modifying_history
                //     .push((block_id, ts, core_id, false, false));
                SharedCacheLookupResult::Unknown(
                    hit_block.ts as u32 - ts as u32,
                    hit_block.modified,
                )
            };
        }

        // otherwise, it is a miss.
        if self.touched_count < WAY {
            SharedCacheLookupResult::ColdMiss
        } else {
            SharedCacheLookupResult::Miss
        }
    }

    #[inline]
    pub fn lookup(&mut self, r: &CacheBlockRequest, ts: u64) -> SharedCacheLookupResult {
        self.access_count += 1;

        let block_id = r.block_id;
        let core_id = r.core_id;
        let access_type = r.access_type.clone();
        let is_os = r.is_os();

        if let Some(hit_block) = self.index_of(block_id) {
            let hit_block = &mut self.blocks[hit_block];
            if hit_block.ts > ts && COHERENCE_FOLLOW_TS {
                // Reversed access order. We cannot do anything but return unknown.

                // self.modifying_history
                //     .push((block_id, ts, core_id, false, false));

                return SharedCacheLookupResult::Unknown(
                    hit_block.ts as u32 - ts as u32,
                    hit_block.modified,
                );
            } else {
                if r.is_store() || EXCLUSIVE {
                    // In any case, the invaildation should be recorded.
                    if self.recent_evict_ts < ts {
                        self.recent_evict_ts = ts;
                    }

                    // The cache line should be transferred to the accessor.
                    // If the cache line has larger ts, it should exists in the cache, thus should be invalid
                    // If the cache line has smaller ts, it should be invalid as well.
                    hit_block.block_id_with_v = 0;
                    hit_block.ts = 0;

                    // self.modifying_history
                    //     .push((block_id, ts, core_id, false, true));

                    return SharedCacheLookupResult::Hit(hit_block.modified);
                } else {
                    // the equal case is only about page walk, which enables touching multiple cache lines with the same timestamp.
                    hit_block.ts = ts;

                    hit_block.last_accessor = core_id;

                    // Accessed by a higher-level, meaning that the block is not dirty anymore.
                    let was_modified = hit_block.modified;
                    hit_block.modified = false;

                    // self.modifying_history
                    //     .push((block_id, ts, core_id, false, true));

                    self.statistics.record(access_type, is_os, true);
                    return SharedCacheLookupResult::Hit(was_modified);
                }
            }
        }

        self.statistics.record(access_type, is_os, false);

        if self.touched_count < WAY {
            SharedCacheLookupResult::ColdMiss
        } else {
            SharedCacheLookupResult::Miss
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
                if ts >= hit_block.ts {
                    hit_block.ts = ts;
                    if is_modified {
                        // self.modifying_history
                        //     .push((block_id, ts, core_id, is_modified, true));
                        // This cache line's permission should have been taken by the private cache.
                        // So it should not be modified in the shared cache.
                        if hit_block.modified {
                            // Open a file.
                            // let mut file = std::fs::File::create("cache_line_history.txt").unwrap();
                            // // Dump the modifying history of this cache line.
                            // for h in &self.modifying_history {
                            //     if h.0 == block_id {
                            //         // println!("Block id: {}, timestamp: {}, by core {}, to {}. Succeed: {}", h.0, h.1, h.2, h.3, h.4);
                            //         file.write_all(format!("Block id: {}, timestamp: {}, by core {}, to {}. Succeed: {}\n", h.0, h.1, h.2, h.3, h.4).as_bytes()).unwrap();
                            //     }
                            // }
                            // file.flush().unwrap();
                            panic!();
                        }
                        hit_block.modified = is_modified;
                    } else {
                        // self.modifying_history
                        //     .push((block_id, ts, core_id, is_modified, false));
                    }

                    hit_block.last_accessor = core_id;
                }
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

        // self.modifying_history
        //     .push((oldest_block.block_id_with_v >> 1, ts, core_id, false, true));

        // otherwise, we replace the oldest block.
        oldest_block.block_id_with_v = block_id_with_v;
        oldest_block.modified = is_modified;
        oldest_block.ts = ts;
        oldest_block.last_accessor = core_id;

        // update the eviction counter.
        if self.recent_evict_ts < ts {
            self.recent_evict_ts = ts;
        }

        // self.modifying_history
        //     .push((block_id, ts, core_id, is_modified, true));
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
            SharedCacheLookupResult::Hit(modified) => {
                SharedCacheLookupAndInsertResult::Hit(modified)
            }
            SharedCacheLookupResult::Miss => {
                // This function is only called when the private cache has a miss
                // Therefore, we cannot insert a modified block here, because the write permission should have been taken by the private cache.
                if !r.is_store() {
                    let just_warmed =
                        self.insert(block_id, core_id, ts, false, increase_touched_count);
                    assert!(!just_warmed);
                    SharedCacheLookupAndInsertResult::Inserted
                } else {
                    SharedCacheLookupAndInsertResult::Miss
                }
            }
            SharedCacheLookupResult::ColdMiss => {
                // This function is only called when the private cache has a miss
                // Therefore, we cannot insert a modified block here, because the write permission should have been taken by the private cache.
                if !r.is_store() {
                    let just_warmed =
                        self.insert(block_id, core_id, ts, false, increase_touched_count);
                    SharedCacheLookupAndInsertResult::InsertedAndCold(just_warmed)
                } else {
                    SharedCacheLookupAndInsertResult::Miss
                }
            }
            SharedCacheLookupResult::Unknown(diff, is_dirty) => {
                SharedCacheLookupAndInsertResult::Unknown(diff, is_dirty)
            }
        }
    }
}

#[test]
fn minimum_can_find_invalid() {
    use super::statistics::ZeroSharedCacheSetStatistics;

    let mut set = SharedCacheSet::<8, 1, false, true, ZeroSharedCacheSetStatistics>::new();
    let mut ts = 1;

    // push 8 elements inside.
    for i in 0..8 {
        assert_eq!(set.insert(i, 0, ts, false, true), i == 7);
        ts += 1;
    }

    // now, we invalid set 0.
    assert_eq!(
        set.invalidate(0, ts, 0),
        SharedCacheLookupResult::Hit(false)
    );
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

    let mut set = SharedCacheSet::<8, 1, false, true, ZeroSharedCacheSetStatistics>::new();

    let mut ts = 1;

    // do one lookup. It will return the cold.
    assert_eq!(
        set.lookup(
            &CacheBlockRequest {
                block_id: 10,
                core_id: 0,
                access_type: CacheAccessType::DataRead,
                is_os: false,
                pc: 0,
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
                is_os: false,
                pc: 0,
            },
            ts,
        ),
        SharedCacheLookupResult::Miss,
    );
}
