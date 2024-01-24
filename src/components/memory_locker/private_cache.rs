use std::sync::RwLock;

use super::directory::Directory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateCacheState {
    Invalid,
    CleanShared,
    DirtyShared,
    CleanExclusive,
    DirtyExclusive,
}

#[derive(Debug, Clone, Copy)]
pub struct PrivateCacheLine {
    pub state: PrivateCacheState,
    pub tag: usize,
    pub ts: usize,
    pub is_instruction: bool,
}

pub struct PrivateCache<const SET: usize, const WAY: usize> {
    cache: [RwLock<[PrivateCacheLine; WAY]>; SET],
}

impl<const SET: usize, const WAY: usize> PrivateCache<SET, WAY> {
    pub fn new() -> Self {
        Self {
            cache: std::array::from_fn(|_| {
                RwLock::new(
                    [PrivateCacheLine {
                        state: PrivateCacheState::Invalid,
                        tag: 0,
                        ts: 0,
                        is_instruction: false,
                    }; WAY],
                )
            }),
        }
    }

    // this function is called to check the permission of the cache line before accessing the directory.
    pub fn poke(&self, block_id: usize) -> PrivateCacheState {
        let set = block_id % SET;
        let cache_set = self.cache[set].read().unwrap();

        // find from the cache set with block id.
        let hit_element = cache_set.iter().find(|p| {
            return p.tag == block_id;
        });

        if let Some(hit_element) = hit_element {
            return hit_element.state;
        } else {
            return PrivateCacheState::Invalid;
        }
    }

    pub fn insert(&self, block_id: usize, ts: usize, state: PrivateCacheState) -> Option<PrivateCacheLine> {
        let set = block_id % SET;
        let mut cache_set = self.cache[set].write().unwrap();

        // find from the cache set with block id.
        let hit_element = cache_set.iter_mut().find(|p| {
            return p.tag == block_id;
        });

        if let Some(hit_element) = hit_element {
            hit_element.ts = ts;
            hit_element.state = state;
            return None;
        } else {
            // find the first invalid element.
            let invalid_element = cache_set.iter_mut().find(|p| {
                return p.state == PrivateCacheState::Invalid;
            });

            if let Some(invalid_element) = invalid_element {
                invalid_element.ts = ts;
                invalid_element.tag = block_id;
                invalid_element.state = state;
                return None;
            } else {
                // find the oldest element.
                let oldest_element = cache_set.iter_mut().min_by(|p, q| {
                    return p.ts.cmp(&q.ts);
                });

                match oldest_element {
                    Some(oldest_element) => {
                        let res = oldest_element.clone();
                        oldest_element.ts = ts;
                        oldest_element.tag = block_id;
                        oldest_element.state = state;
                        return Some(res);
                    }
                    None => {
                        unreachable!("PrivateCache::insert: no element in the cache set.");
                    }
                }
            }
        }
    }

    pub fn invalidate(&self, block_id: usize) -> Option<PrivateCacheLine> {
        let set = block_id % SET;
        let mut cache_set = self.cache[set].write().unwrap();

        // find from the cache set with block id.
        let hit_element = cache_set.iter_mut().find(|p| {
            return p.tag == block_id;
        });

        if let Some(hit_element) = hit_element {
            let res = hit_element.clone();
            hit_element.state = PrivateCacheState::Invalid;
            return Some(res);
        } else {
            return None;
        }
    }
}
