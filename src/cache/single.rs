use core::num::NonZeroUsize;
use lru::LruCache;

use super::CacheReturnResult;

pub const BLOCK_SIZE_LOG2: usize = 6;
pub const BLOCK_SIZE: usize = 1 << 6;

pub struct PrivateCache {
    sets: Vec<LruCache<usize, bool>>,
    associativity: usize,
    warmed_count: usize,
}

impl PrivateCache {
    pub fn new(set_count: usize, associativity: usize) -> Self {
        return PrivateCache {
            sets: (0..set_count)
                .map(|_| {
                    return LruCache::<usize, bool>::new(NonZeroUsize::new(associativity).unwrap());
                })
                .collect(),
            associativity,
            warmed_count: 0,
        };
    }

    pub fn update(&mut self, block_id: usize, is_write: bool) -> CacheReturnResult {
        let set_index = block_id % self.sets.len();
        let set = self.sets.get_mut(set_index).unwrap();
        let replaced = set.push(block_id, is_write);
        return match replaced {
            Some((evicted_block_id, is_dirty)) => {
                if evicted_block_id != block_id {
                    if is_dirty {
                        CacheReturnResult::MissWithDirtyEviction(evicted_block_id)
                    } else {
                        CacheReturnResult::MissWithEviction(evicted_block_id)
                    }
                } else {
                    CacheReturnResult::Hit
                }
            }
            None => CacheReturnResult::Miss,
        };
    }

    pub fn invalidate(&mut self, block_id: usize) -> Option<bool> {
        let set_index: usize = block_id % self.sets.len();
        let set = self.sets.get_mut(set_index).unwrap();
        let evicted = set.pop(&block_id);
        return evicted;
    }

    pub fn is_fully_warmed_up(&self) -> bool {
        return self.warmed_count == self.sets.len();
    }

    pub fn usage(&self) -> f64 {
        let usage = self
            .sets
            .iter()
            .map(|x| -> usize { x.len() })
            .sum::<usize>() as f64;

        return usage / (self.sets.len() * self.associativity) as f64;
    }

    pub fn serialize(&self) -> Vec<Vec<Option<usize>>> {
        return self
            .sets
            .iter()
            .map(|x| -> Vec<Option<usize>> {
                return x
                    .iter()
                    .map(|el| -> Option<usize> {
                        return match &el.1 {
                            true => Some(*el.0),
                            false => None,
                        };
                    })
                    .collect();
            })
            .collect();
    }
}

mod test {
    // use super::*;
    // #[test]
    // fn single_set_test() {
    //     let mut c = PrivateCache::new(1, 4);
    //     c.update(0, false);

    //     assert!(c.usage() == 0.25);

    //     c.update(1, false);
    //     c.update(2, false);
    //     c.update(3, false);

    //     assert!(c.is_fully_warmed_up());

    //     assert!(c.serialize() == vec![vec![Some(3), Some(2), Some(1), Some(0)]]);
    // }

    // #[test]
    // fn multiple_set_test() {
    //     let mut c = PrivateCache::new(4, 1);

    //     c.update(1);
    //     c.update(2);
    //     c.update(3);
    //     c.update(4);

    //     assert!(c.usage() == 0.25);

    //     c.update(65);
    //     c.update(129);
    //     c.update(195);

    //     assert!(c.is_fully_warmed_up());

    //     assert!(c.serialize() == vec![vec![Some(0)], vec![Some(1)], vec![Some(2)], vec![Some(3)]]);

    //     c.update(64 * 6 + 14);

    //     assert!(c.serialize() == vec![vec![Some(0)], vec![Some(1)], vec![Some(6)], vec![Some(3)]]);
    // }

    // #[test]
    // fn replacement_test() {
    //     let mut c = PrivateCache::new(2, 2);

    //     c.update(67);
    //     c.update(257);
    //     c.update(197);
    //     c.update(15);

    //     assert!(c.serialize() == vec![vec![Some(0), Some(4)], vec![Some(3), Some(1)]]);

    //     c.update(1027);
    //     assert!(c.serialize() == vec![vec![Some(16), Some(0)], vec![Some(3), Some(1)]]);

    //     c.update(18);
    //     assert!(c.serialize() == vec![vec![Some(0), Some(16)], vec![Some(3), Some(1)]]);
    // }

    // #[test]
    // fn eviction() {
    //     let mut c = PrivateCache::new(2, 2);

    //     c.update(67);
    //     c.update(257);
    //     c.update(197);
    //     c.update(15);

    //     assert!(c.serialize() == vec![vec![Some(0), Some(4)], vec![Some(3), Some(1)]]);

    //     c.invalidate(17);
    //     assert!(c.serialize() == vec![vec![Some(4), None], vec![Some(3), Some(1)]]);

    //     c.update(1029);
    //     assert!(c.serialize() == vec![vec![Some(16), Some(4)], vec![Some(3), Some(1)]]);

    //     c.invalidate(65);
    //     c.invalidate(194);
    //     assert!(c.serialize() == vec![vec![Some(16), Some(4)], vec![None, None]]);
    // }

    // use chrono::Local;

    // #[test]
    // fn time_insertion() {
    //     let mut ncache = PrivateCache::new(1024, 16);
    //     let t1 = Local::now();
    //     for t in 0..1000 * 1000 * 10 {
    //         ncache.update(t);
    //     }
    //     let t2 = Local::now();
    //     println!("{}", t2 - t1);
    // }
}
