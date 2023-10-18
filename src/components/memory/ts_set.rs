use core::panic;

// The return result of accessing a cache line.
pub enum CacheReturnResult {
    Miss,
    Hit,
    MissWithEviction(usize, bool), // (block_id, is_instruction)
    MissWithWriteBack(usize), // (block_id)
}


#[derive(PartialEq, Clone, Copy, Debug)]
pub enum TimestampCacheLineStatus {
    Invalid,
    Instruction,
    CleanData,
    CleanInstructionAndData,
    DirtyData,
}

impl TimestampCacheLineStatus {
    pub fn is_dirty(&self) -> bool {
        return *self == TimestampCacheLineStatus::DirtyData;
    }

    pub fn is_data(&self) -> bool {
        return *self == TimestampCacheLineStatus::CleanData || *self == TimestampCacheLineStatus::CleanInstructionAndData || *self == TimestampCacheLineStatus::DirtyData;
    }

    pub fn is_instruction(&self) -> bool {
        return *self == TimestampCacheLineStatus::Instruction || *self == TimestampCacheLineStatus::CleanInstructionAndData;
    }
}

#[derive(Clone, Copy)]
struct SetAccessResult<const A: usize>(usize);

impl<const A: usize> SetAccessResult<A> {
    pub fn new(index: usize) -> Self {
        return Self(index);
    }

    pub fn new_miss() -> Self {
        return Self(A);
    }

    pub fn is_hit(&self) -> bool {
        return self.0 != A;
    }

    pub fn is_miss(&self) -> bool {
        return self.0 == A;
    }
    
    pub fn index(&self) -> usize {
        return self.0;
    }
}

#[derive(Debug)]
pub struct TimestampCacheSet<const A: usize> {
    block_ids: [usize; A],
    ts: [usize; A], // timestamp contains order information, so no necessary for LRU bits.
    status: [TimestampCacheLineStatus; A],
    cold_element_pointer: usize, // from which element the instruction is invalid.
}

impl<const A: usize> TimestampCacheSet<A> {
    pub fn new() -> Self {
        return Self {
            block_ids: std::array::from_fn(|_| 0),
            ts: std::array::from_fn(|_| 0),
            status: std::array::from_fn(|_| TimestampCacheLineStatus::Invalid),
            cold_element_pointer: 0,
        };
    }

    fn lookup(&self, block_id: usize) -> SetAccessResult<A> {
        return match self
            .block_ids
            .iter()
            .zip(self.status.iter())
            .enumerate()
            .find(|(_, v)| {
                return (*(v.0) == block_id) && (*v.1 != TimestampCacheLineStatus::Invalid);
            }) {
            Some((idx, _)) => SetAccessResult::new(idx),
            None => SetAccessResult::new_miss(),
        };
    }

    fn add_new(
        &mut self,
        block_id: usize,
        ts: usize,
        is_instruction: bool,
        is_write: bool,
    ) -> Option<(usize, TimestampCacheLineStatus)> {
        // always triggers a replacement.
        // please make sure you check the result of lookup first before adding an element
        // otherwise it can cause redundancy.

        let potential_status = match (is_instruction, is_write) {
            (true, true) => panic!(),
            (true, false) => TimestampCacheLineStatus::Instruction,
            (false, true) => TimestampCacheLineStatus::DirtyData,
            (false, false) => TimestampCacheLineStatus::CleanData,
        };

        // First, we consider whether there is an empty element
        if self.cold_element_pointer < A {
            self.block_ids[self.cold_element_pointer] = block_id;
            self.ts[self.cold_element_pointer] = ts;
            self.status[self.cold_element_pointer] = potential_status;
            self.cold_element_pointer += 1;

            return None;
        }

        // Well, now we have to find a victim. We try to first find an invalid place.
        match self.status.iter().enumerate().find(|(_, status)| {
            return (**status) == TimestampCacheLineStatus::Invalid;
        }) {
            Some((idx, _)) => {
                self.block_ids[idx] = block_id;
                self.ts[idx] = ts;
                self.status[idx] = potential_status;
            }
            None => {}
        };

        // Finally, we then have to find an eviction.
        match self
            .ts
            .iter()
            .enumerate()
            .min_by_key(|(_, timestamp)| **timestamp)
        {
            Some((idx, _)) => {
                let res = Some((self.block_ids[idx], self.status[idx]));
                self.block_ids[idx] = block_id;
                self.ts[idx] = ts;
                self.status[idx] = potential_status;
                return res;
            }
            None => unreachable!(),
        }
    }

    pub fn peek(
        &mut self,
        block_id: usize,
        ts: usize,
        is_instruction: bool,
        is_write: bool,
    ) -> bool {
        // return whether this peek is hit. If it is, update its timestamp.
        let look_index = self.lookup(block_id);
        if look_index.is_miss() {
            return false;
        }
        self.ts[look_index.index()] = ts;
        match self.status[look_index.index()] {
            TimestampCacheLineStatus::Invalid => unreachable!(),
            TimestampCacheLineStatus::Instruction => {
                if !is_instruction {
                    self.status[look_index.index()] =
                        TimestampCacheLineStatus::CleanInstructionAndData;
                }
                // NX
                debug_assert!(!is_write);
            }
            TimestampCacheLineStatus::CleanData => {
                if is_instruction {
                    self.status[look_index.index()] =
                        TimestampCacheLineStatus::CleanInstructionAndData;
                    debug_assert!(!is_write);
                }

                if is_write {
                    self.status[look_index.index()] = TimestampCacheLineStatus::DirtyData;
                    debug_assert!(!is_instruction);
                }
            }
            TimestampCacheLineStatus::CleanInstructionAndData => {
                debug_assert!(!is_write);
            }
            TimestampCacheLineStatus::DirtyData => {
                debug_assert!(!is_instruction);
            }
        }
        return true;
    }

    pub fn access(
        &mut self,
        block_id: usize,
        ts: usize,
        is_instruction: bool,
        is_write: bool,
    ) -> CacheReturnResult {
        if self.peek(block_id, ts, is_instruction, is_write) {
            return CacheReturnResult::Hit;
        }
        return match self.add_new(block_id, ts, is_instruction, is_write) {
            Some((evicted_block_id, status)) => {
                if status == TimestampCacheLineStatus::DirtyData {
                    CacheReturnResult::MissWithWriteBack(evicted_block_id)
                } else {
                    CacheReturnResult::MissWithEviction(evicted_block_id, status.is_instruction())
                }
            }
            None => CacheReturnResult::Miss,
        };
    }

    pub fn invalid(&mut self) {
        todo!();
    }

    pub fn warm_chunk_count(&self) -> usize {
        return self.cold_element_pointer;
    }

    pub fn len(&self) -> usize {
        todo!();
    }
}

pub struct TimestampCacheSetIterator<'a, const A: usize> {
    base: &'a TimestampCacheSet<A>,
    current_idx: usize
}

impl<'a, const A: usize> Iterator for TimestampCacheSetIterator<'a, A> {
    type Item = (usize, usize, TimestampCacheLineStatus); /// (block_id, ts, status)

    /// Return value: (block_id, ts, status)
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx == A {
            return None;
        }

        if self.current_idx >= self.base.cold_element_pointer {
            return None;
        }

        for it in self.current_idx..A {
            if self.base.status[it] != TimestampCacheLineStatus::Invalid {
                // find it! 
                self.current_idx = it + 1;
                return Some((self.base.block_ids[it], self.base.ts[it], self.base.status[it]));
            }
        }
        return None;
    }
}

impl<const A: usize> TimestampCacheSet<A> {
    pub fn iter<'a>(&'a self) -> TimestampCacheSetIterator<'a, A> {
        return TimestampCacheSetIterator { base: self, current_idx: 0 };
    }
}

mod test {
    #[test]
    fn test_iterator(){
        let mut s = super::TimestampCacheSet::<4>::new();
        s.access(1024, 0, false, false);
        s.access(100, 1, false, false);
        s.access(23, 2, false, false);
        s.access(7, 7, false, false);
        s.access(73, 10, false, false);

        for x in s.iter() {
        }

    }
}