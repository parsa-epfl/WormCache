use core::num::NonZeroUsize;
use lru::LruCache;

pub const BLOCK_SIZE_LOG2: usize = 6;
pub const BLOCK_SIZE: usize = 1 << 6;

pub trait CacheMetaData {
    fn is_dirty(&self) -> bool;
    fn set_dirty(&mut self, dirty: bool);
}

pub enum SingleCacheResult<T: Copy + CacheMetaData> {
    Hit,
    Miss,
    MissAndEvicted(usize, T),
}

pub struct PrivateCache<const A: usize, const S: usize, T: Copy + CacheMetaData> {
    // A: associativity, S: sets
    sets: [LruCache<usize, T>; S],
    warmed_count: usize,
}

impl<const A: usize, const S: usize, T: Copy + CacheMetaData> PrivateCache<A, S, T> {
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
        return PrivateCache {
            sets: std::array::from_fn(|_| {
                return LruCache::new(NonZeroUsize::new(S).unwrap());
            }),
            warmed_count: 0,
        };
    }

    pub fn update(&mut self, block_id: usize, updated_metadata: T) -> SingleCacheResult<T> {
        let set_index = block_id & (BLOCK_SIZE - 1);
        let set = self.sets.get_mut(set_index).unwrap();
        let replaced = set.push(block_id, updated_metadata);
        return match replaced {
            Some((evicted_block_id, evicted_metadata)) => {
                if evicted_block_id != block_id {
                    SingleCacheResult::MissAndEvicted(evicted_block_id, evicted_metadata)
                } else {
                    if evicted_metadata.is_dirty() {
                        set.get_mut(&block_id).unwrap().set_dirty(true);
                    }
                    SingleCacheResult::Hit
                }
            }
            None => SingleCacheResult::Miss,
        };
    }

    pub fn invalidate(&mut self, block_id: usize) -> Option<T> {
        let set_index: usize = block_id & (BLOCK_SIZE - 1);
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

    // pub fn serialize(&self) -> Vec<Vec<Option<usize>>> {
    //     return self
    //         .sets
    //         .iter()
    //         .map(|x| -> Vec<Option<usize>> {
    //             return x
    //                 .iter()
    //                 .map(|el| -> Option<usize> {
    //                     return match &el.1 {
    //                         true => Some(*el.0),
    //                         false => None,
    //                     };
    //                 })
    //                 .collect();
    //         })
    //         .collect();
    // }
}
