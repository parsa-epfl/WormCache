use std::collections::HashSet;

pub struct TouchedCacheSet {
    set: HashSet<usize>, // The block ID is in the cache.
    capacity: usize,     // Associativity
    fully_touched: bool, // All blocks are touched.
}

impl TouchedCacheSet {
    pub fn new(capacity: usize) -> Self {
        Self {
            set: HashSet::new(),
            capacity,
            fully_touched: false,
        }
    }

    // Returns true if the set is fully touched by the current touch.
    pub fn touch(&mut self, block_id: usize) -> bool {
        if self.fully_touched {
            return false;
        }
        let previous_size = self.set.len();
        self.set.insert(block_id);
        if previous_size == (self.capacity - 1) && self.set.len() == self.capacity {
            self.fully_touched = true;
            return true;
        }
        return false;
    }
}

pub struct TouchedCache {
    sets: Vec<TouchedCacheSet>,
    fully_touched_sets: usize,
}

impl TouchedCache {
    pub fn new(sets: usize, associativity: usize) -> Self {
        Self {
            sets: Vec::from_iter((0..sets).map(|_| TouchedCacheSet::new(associativity))),
            fully_touched_sets: 0,
        }
    }

    fn touch(&mut self, set_id: usize, block_id: usize) -> bool {
        return self.sets[set_id].touch(block_id);
    }

    pub fn access(&mut self, pa: usize) -> bool {
        let block_id = pa >> (crate::parameter::CACHE_LINE_SIZE.trailing_zeros());
        let set_index = block_id & (self.sets.len() - 1);
        let res = self.touch(set_index, block_id);
        if res {
            self.fully_touched_sets += 1;
        }
        return res;
    }

    pub fn is_fully_touched(&self) -> bool {
        return self.fully_touched_sets == self.sets.len();
    }

    pub fn get_fully_touched_set_count(&self) -> usize {
        self.fully_touched_sets
    }

    pub fn reset(&mut self) {
        self.sets.iter_mut().for_each(|set| {
            set.fully_touched = false;
            set.set.clear();
        });
        self.fully_touched_sets = 0;
    }
}
