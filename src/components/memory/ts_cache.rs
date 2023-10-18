/**
 * This module defines the private LLC used for per-core LLC warmup.
 * It contains two information for each block: the timestamp, and the dirty bits
 */

#[derive(Clone)]
pub struct TimestampCacheMetaData {
    pub ts: usize,
    pub is_dirty: bool,
}

#[derive(Debug)]
pub struct TimestampCache<const A: usize, const S: usize> {
    // A: associativity, S: sets
    pub sets: Box<[super::TimestampCacheSet<A>; S]>,
    pub warmed_count: usize,
}

impl<const A: usize, const S: usize> TimestampCache<A, S> {
    pub const _SET_SHIFT_COUNT: usize = S.trailing_zeros() as usize;

    const fn _check_generics() -> bool {
        if (S & (S - 1)) != 0 {
            return false;
        }

        if A == 0 {
            return false;
        }

        return true;
    }

    #[allow(unused_variables)]
    const _CHECK_PARAM: () = assert!(Self::_check_generics());

    pub fn new() -> Self {
        return TimestampCache::<A, S> {
            // Man, I have to use unsafe here, because I cannot allocate large array in Box.
            sets: Vec::from_iter((0..S).map(|_| super::TimestampCacheSet::<A>::new()))
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            warmed_count: 0,
        };
    }

    pub fn record(
        &mut self,
        block_id: usize,
        is_instruction: bool,
        is_write: bool,
        ts: usize,
    ) -> super::CacheReturnResult {
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
        ts: usize,
    ) -> bool {
        let set_number = block_id & (S - 1);
        return self.sets[set_number].peek(block_id, ts, is_instruction, is_write);
    }
}
