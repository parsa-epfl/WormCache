use std::collections::HashMap;

pub struct ReplicaPosition {
    v: u128, /* TODO: Replace this one with bitvec to save place */
}

pub struct ReplicaPositionIter {
    v: u128,
    current_idx: u8,
}

impl Iterator for ReplicaPositionIter {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx >= 128 {
            return None;
        }

        for i in self.current_idx..128 {
            let mask = 1u128 << i;
            if (self.v & mask) != 0 {
                self.current_idx = i + 1;
                return Some(i);
            }
        }

        return None;
    }
}

impl ReplicaPosition {
    pub fn maximum_core_count() -> usize {
        return 128;
    }

    pub fn rerplicas_iter(&self) -> ReplicaPositionIter {
        return ReplicaPositionIter {
            v: self.v,
            current_idx: 0,
        };
    }

    pub fn new_with_single_position(id: u8) -> Self {
        return ReplicaPosition { v: 1u128 << id };
    }

    pub fn set_position(&mut self, id: u8) {
        self.v = self.v | (1u128 << id);
    }

    pub fn unset_position(&mut self, id: u8) {
        self.v = self.v & (!(1u128 << id));
    }

    // this will be really useful when
    pub fn only_keep_one(&mut self, id: u8) {
        self.v = 1u128 << id;
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        return self.v == 0;
    }
}

mod replace_position {
    use super::ReplicaPosition;

    #[test]
    fn general_test() {
        let mut r = ReplicaPosition::new_with_single_position(0);
        assert!(r.v == 1);
        r.set_position(10);
        assert!(r.v == 1025);
        r.set_position(20);
        assert!(r.v == 1025 + (1u128 << 20));
        let iter = r.rerplicas_iter();
        assert_eq!(iter.collect::<Vec<u8>>(), vec![0, 10, 20]);

        r.set_position(50);

        r.unset_position(10);

        let iter = r.rerplicas_iter();
        assert_eq!(iter.collect::<Vec<u8>>(), vec![0, 20, 50]);

        r.only_keep_one(30);

        let iter = r.rerplicas_iter();
        assert_eq!(iter.collect::<Vec<u8>>(), vec![30]);
    }
}

pub struct Directory {
    content: HashMap<usize, ReplicaPosition>,
}

impl Directory {
    pub fn new() -> Self {
        return Directory {
            content: HashMap::new(),
        };
    }

    pub fn insert(&mut self, block_id: usize, core_id: u8) {
        //  well, hope the insertion is not very painful because it is protected by a lock.
        match self.content.get_mut(&block_id) {
            Some(k) => {
                k.set_position(core_id);
            }
            None => {
                self.content
                    .insert(block_id, ReplicaPosition::new_with_single_position(core_id));
            }
        };
    }

    pub fn remove(&mut self, block_id: usize, core_id: u8) {
        match self.content.get_mut(&block_id) {
            Some(b) => b.unset_position(core_id),
            None => unreachable!("Please make sure that the entry is set before removing."),
        }
        // this function does not remove the entry from the HashTable. 
    }

    pub fn peek(&self, block_id: usize) -> ReplicaPositionIter {
        return match self.content.get(&block_id) {
            Some(b) => b.rerplicas_iter(),
            None => ReplicaPositionIter {
                v: 0,
                current_idx: 128,
            },
        };
    }

    pub fn unique(&mut self, block_id: usize, core_id: u8) {
        match self.content.get_mut(&block_id) {
            Some(b) => b.only_keep_one(core_id),
            None => {}
        }
    }

    pub fn run_gc(&mut self) {
        // it is a lazy way to clean the garbage...
        
        let empties_key: Vec<_> = self.content.iter().filter(|x| {
            x.1.is_empty()
        }).map(|x| {
            *x.0
        }).collect();

        for rk in empties_key {
            self.content.remove(&rk);
        }
    }
}
