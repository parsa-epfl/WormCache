use core::num::NonZeroUsize;
use lru::LruCache;

use super::CacheReturnResult;

pub const BLOCK_SIZE_LOG2: usize = 6;
pub const BLOCK_SIZE: usize = 1 << 6;

pub struct PrivateCache<const A: usize, const S: usize> { // A: associativity, S: sets
    sets: [LruCache<usize, bool>; S],
    warmed_count: usize,
}

impl <const A: usize, const S: usize> PrivateCache <A, S> {
    pub const SET_SHIFT_COUNT: usize = S.trailing_zeros() as usize;

    const fn check_generics(){
        if (S & (S-1)) != 0 {
            panic!("Set count should be aligned with power of 2.")
        }

        if A == 0 {
            panic!("Associativity should be always larger than 0.")
        }
    }

    pub fn new() -> Self {
        Self::check_generics();
        return PrivateCache {
            sets: std::array::from_fn(|_| {
                return LruCache::new(
                    NonZeroUsize::new(S).unwrap(),
                );
            }),
            warmed_count: 0,
        };
    }

    pub fn update(&mut self, block_id: usize, is_write: bool) -> CacheReturnResult {
        let set_index = block_id >> Self::SET_SHIFT_COUNT;
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
        let set_index: usize = block_id >> Self::SET_SHIFT_COUNT;
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

        return usage / (self.sets.len() * A) as f64;
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
