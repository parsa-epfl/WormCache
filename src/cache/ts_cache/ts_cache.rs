/**
 * This module defines the private LLC used for per-core LLC warmup.
 * It contains two information for each block: the timestamp, and the dirty bits
 */
use crate::cache::CacheReturnResult;
use super::ts_set::TimestampCacheSet;

#[derive(Clone)]
pub struct TimestampCacheMetaData {
    pub ts: usize,
    pub is_dirty: bool,
}

pub struct TimestampCache<const A: usize, const S: usize> {
    // A: associativity, S: sets
    pub sets: Box<[TimestampCacheSet<A>; S]>,
    pub warmed_count: usize,
}

impl<const A: usize, const S: usize> TimestampCache<A, S> {
    pub const SET_SHIFT_COUNT: usize = S.trailing_zeros() as usize;

    const fn check_generics() -> bool {
        if (S & (S - 1)) != 0 {
            return false;
        }

        if A == 0 {
            return false;
        }

        return true;
    }

    const check_param: () = assert!(Self::check_generics());

    pub fn new() -> Self {
        Self::check_generics();
        return TimestampCache::<A, S> {
            sets: Box::new(std::array::from_fn(|_| TimestampCacheSet::new())),
            warmed_count: 0,
        };
    }

    pub fn record(
        &mut self,
        block_id: usize,
        is_instruction: bool,
        is_write: bool,
        ts: usize,
    ) -> CacheReturnResult {
        let set_number = block_id & (S - 1);
        let set = &mut self.sets[set_number];

        let old_element_count = set.warm_chunk_count();

        let res = set.access(block_id, ts, is_instruction, is_write);

        // Counter to know how much is warmed up
        if old_element_count == (A - 1) && set.warm_chunk_count() == A {
            self.warmed_count += 1;
        }

        return res;
    }

    pub fn peek(
        &mut self,
        block_id: usize,
        is_instruction: bool,
        is_write: bool,
        ts: usize
    ) -> bool {
        let set_number = block_id * (S - 1);
        return self.sets[set_number].peek(block_id, ts, is_instruction, is_write);
    }
}
