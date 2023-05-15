/**
 * This module defines the private LLC used for per-core LLC warmup.
 * It contains two information for each block: the timestamp, and the dirty bits
 */
use core::num::NonZeroUsize;
use lru::LruCache;

pub struct TimestampCacheMetaData {
    pub ts: usize,
    pub is_dirty: bool,
}

pub struct TimestampCache<const A: usize, const S: usize> {
    // A: associativity, S: sets
    pub sets: Vec<LruCache<usize, TimestampCacheMetaData>>,
    pub warmed_count: usize,
}

impl<const A: usize, const S: usize> TimestampCache<A, S> {
    pub const SET_SHIFT_COUNT: usize = S.trailing_zeros() as usize;

    const fn check_generics() {
        if (S & (S - 1)) != 0 {
            panic!("Set count should be aligned with power of 2.")
        }

        if A == 0 {
            panic!("Associativity should be always larger than 0.")
        }
    }

    pub fn new() -> Self {
        Self::check_generics();
        return TimestampCache::<A, S> {
            sets: (0..S)
                .map(|_| return LruCache::new(NonZeroUsize::new(A).unwrap()))
                .collect(),
            warmed_count: 0,
        };
    }

    pub fn record(
        &mut self,
        block_id: usize,
        is_dirty: bool,
        ts: usize,
    ) -> super::CacheReturnResult {
        let set_number = block_id & (S - 1);
        let set = &mut self.sets[set_number];

        let old_element_count = set.len();

        let res = match set.push(block_id, TimestampCacheMetaData { ts, is_dirty }) {
            Some((evicted_block_id, evicted_metadata)) => {
                if evicted_block_id == block_id {
                    // propagate the dirty bits.
                    let new_is_dirty = evicted_metadata.is_dirty || is_dirty;
                    set.peek_mut(&block_id).unwrap().is_dirty = new_is_dirty;
                    return super::CacheReturnResult::Hit;
                } else {
                    // Well, we evict someone else
                    return match evicted_metadata.is_dirty {
                        true => super::CacheReturnResult::MissWithWriteBack(evicted_block_id),
                        false => super::CacheReturnResult::MissWithEviction(evicted_block_id),
                    };
                }
            }
            None => super::CacheReturnResult::Miss,
        };

        // Counter to know how much is warmed up
        if old_element_count == (A - 1) && set.len() == A {
            self.warmed_count += 1;
        }

        return res;
    }
}
