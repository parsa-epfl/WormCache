use core::panic;

use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::components::debug::cache_line_history::CacheLineCoherenceHistory;

use super::{
    statistics::{SharedCacheSetStatistics, ZeroSharedCacheSetStatistics},
    SharedCacheLookupAndInsertResult, SharedCacheLookupResult, VtsViolationResult,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedCacheBlock {
    pub block_id_with_v: u64, // the last bit is the valid bit.
    pub ts: u64,
    pub modified: bool,
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
    pub recent_evict_vts: u64,

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
            }),

            touched_count: 0,
            recent_evict_ts: 0,
            recent_evict_vts: 0,

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
            recent_evict_vts: other.recent_evict_vts,
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
            recent_evict_vts: self.recent_evict_vts,
            access_count: self.access_count,
            statistics: ZeroSharedCacheSetStatistics {},
        }
    }

    #[inline]
    fn index_of(&self, block_id: u64) -> Option<usize> {
        let internal_block_id = block_id << 1 | 1;
        self.blocks
            .iter()
            .position(|p| p.block_id_with_v == internal_block_id)
    }

    fn peek(&mut self, block_id: u64, ts: u64, abandon_dirty: bool) -> Option<bool> {
        if let Some(hit_block) = self.index_of(block_id) {
            let hit_block = &mut self.blocks[hit_block];
            if ts >= hit_block.ts {
                // the equal case is only about page walk, which enables touching multiple cache lines with the same timestamp.
                hit_block.ts = ts;
            }

            if abandon_dirty {
                // Force a write back to the DRAM here.
                // We don't simulate this event now.
                hit_block.modified = false;
            }

            return Some(hit_block.modified);
        }

        // otherwise, it is a miss.
        None
    }

    pub fn invalidate(&mut self, block_id: u64, ts: u64) -> Option<bool> {
        // if it is a hit, we remove this block from the cache
        if let Some(hit_block) = self.index_of(block_id) {
            let hit_block = &mut self.blocks[hit_block];
            let res = Some(hit_block.modified);
            hit_block.block_id_with_v = 0;
            hit_block.ts = 0;

            if self.recent_evict_ts < ts {
                self.recent_evict_ts = ts;
            }

            return res;
        }

        // otherwise, it is a miss.
        None
    }

    #[inline]
    pub fn lookup(
        &mut self,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        abandon_dirty: bool,
        access_type: super::CacheAccessType,
        is_os: bool,
    ) -> (SharedCacheLookupResult, VtsViolationResult) {
        self.access_count += 1;

        match if EXCLUSIVE {
            self.invalidate(block_id, ts)
        } else {
            self.peek(block_id, ts, abandon_dirty)
        } {
            Some(is_dirty) => {
                self.statistics.record(access_type, is_os, true);
                (
                    SharedCacheLookupResult::Hit(is_dirty),
                    VtsViolationResult::NotViolated,
                )
            }
            None => {
                let cache_access_res = if ts < self.recent_evict_ts {
                    SharedCacheLookupResult::Unknown((self.recent_evict_ts - ts) as u32)
                } else {
                    self.statistics.record(access_type, is_os, false);
                    if self.touched_count < WAY {
                        SharedCacheLookupResult::ColdMiss
                    } else {
                        SharedCacheLookupResult::Miss
                    }
                };

                let violation = if v_ts < self.recent_evict_vts {
                    // check the evicted cache line.
                    VtsViolationResult::NotViolated
                } else {
                    VtsViolationResult::NotViolated
                };

                (cache_access_res, violation)
            }
        }
    }

    // return whether this cache set is just warmed.
    #[inline]
    pub fn insert(
        &mut self,
        block_id: u64,
        ts: u64,
        v_ts: u64,
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

        // update the eviction counter.
        if self.recent_evict_ts < ts {
            self.recent_evict_ts = ts;
        }

        if self.recent_evict_vts < v_ts {
            self.recent_evict_vts = v_ts;
        }

        // push the evicted line to the history.
        result
    }

    #[inline]
    pub fn lookup_and_insert(
        &mut self,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        abandon_dirty: bool,
        is_store: bool,
        increase_touched_count: bool,
        access_type: super::CacheAccessType,
        is_os: bool,
    ) -> (SharedCacheLookupAndInsertResult, VtsViolationResult) {
        // (is_hit, dirty/just_warmed)
        let result = self.lookup(block_id, ts, v_ts, abandon_dirty, access_type, is_os);

        let cache_access_result = match result.0 {
            SharedCacheLookupResult::Hit(is_dirty) => {
                SharedCacheLookupAndInsertResult::Hit(is_dirty)
            }
            SharedCacheLookupResult::Miss => {
                let just_warmed = self.insert(block_id, ts, v_ts, is_store, increase_touched_count);
                assert!(just_warmed == false);
                SharedCacheLookupAndInsertResult::Inserted
            }
            SharedCacheLookupResult::ColdMiss => {
                let just_warmed = self.insert(block_id, ts, v_ts, is_store, increase_touched_count);
                SharedCacheLookupAndInsertResult::InsertedAndCold(just_warmed)
            }
            SharedCacheLookupResult::Unknown(diff) => {
                SharedCacheLookupAndInsertResult::Unknown(diff)
            }
        };

        return (cache_access_result, result.1);
    }
}

#[test]
fn minimum_can_find_invalid() {
    use super::statistics::ZeroSharedCacheSetStatistics;

    let mut set = SharedCacheSet::<8, 1, false, ZeroSharedCacheSetStatistics>::new();
    let mut ts = 1;

    // push 8 elements inside.
    for i in 0..8 {
        assert_eq!(set.insert(i, ts, ts, false, true), i == 7);
        ts += 1;
    }

    // now, we invalid set 0.
    assert_eq!(set.invalidate(0, ts), Some(false));
    ts += 1;

    // Now if we refill, we will hit the first place.
    set.insert(9, ts, ts, false, true);

    // And the cache line 0 should be replaced.
    assert_eq!(
        set.blocks[0],
        SharedCacheBlock {
            block_id_with_v: 9 << 1 | 1,
            ts: ts,
            modified: false,
        }
    );

    ts += 1;

    // If we now insert another one, line[1] will be replaced.
    assert_eq!(set.insert(10, ts, ts, false, true), false);

    assert_eq!(
        set.blocks[1],
        SharedCacheBlock {
            block_id_with_v: 10 << 1 | 1,
            ts: ts,
            modified: false,
        }
    )
}

#[test]
fn cold_miss_exist() {
    use super::statistics::ZeroSharedCacheSetStatistics;

    let mut set = SharedCacheSet::<8, 1, false, ZeroSharedCacheSetStatistics>::new();

    let mut ts = 1;

    // do one lookup. It will return the cold.
    assert_eq!(
        set.lookup(10, ts, ts, false, super::CacheAccessType::DataRead, false)
            .0,
        SharedCacheLookupResult::ColdMiss,
    );

    ts += 1;

    // fill the cache.
    for i in 0..8 {
        assert_eq!(set.insert(i + 10, ts, ts, false, true), i == 7);
        ts += 1;
    }

    // do a lookup that triggers a miss. Now it should be a miss instead of a cold miss.
    assert_eq!(
        set.lookup(20, ts, ts, false, super::CacheAccessType::DataRead, false)
            .0,
        SharedCacheLookupResult::Miss,
    );
}
