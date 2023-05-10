use lru::LruCache;
use core::num::NonZeroUsize;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use bitvec::vec::BitVec;

use super::CacheReturnResult;

pub const BLOCK_SIZE_LOG2: usize = 6;
pub const BLOCK_SIZE: usize = 1 << 6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlockState {
    Exclusive,
    Modified,
    Shared
}

impl BlockState {
    pub fn require_exclusive(&self) -> bool {
        return *self == Self::Exclusive || *self == Self::Modified;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CacheEntry {
    pub perm: BlockState
}

#[derive(Clone, Debug)]
pub struct DirectoryEntry {
    perm: BlockState,
    replicas: BitVec
}

#[derive(Debug)]
pub struct ParallelCache {
    sets: Vec<Mutex<LruCache<usize, BlockState>>>,
    associativity: AtomicUsize,
    warmed_count: AtomicUsize,
}


impl ParallelCache {
    pub fn new(set_count: usize, associativity: usize) -> Self {
        return ParallelCache {
            sets: (0..set_count)
                .map(|_| {
                    return Mutex::new(LruCache::new(NonZeroUsize::new(associativity).unwrap()));
                })
                .collect(),
            associativity: associativity.into(),
            warmed_count: 0.into(),
        };
    }

    pub fn update(&self, block_id: usize, perm: BlockState) -> CacheReturnResult {
        let set_index = block_id % self.sets.len();
        let mut set = self.sets[set_index].lock().unwrap();
        let replaced = set.push(block_id, perm);
        return match replaced {
            Some((evicted_block_id, block)) => {
                if evicted_block_id != block_id {
                    if block == BlockState::Modified {
                        CacheReturnResult::MissWithDirtyEviction(evicted_block_id)
                    } else {
                        CacheReturnResult::MissWithEviction(evicted_block_id)
                    }
                } else if block == BlockState::Shared && perm.require_exclusive() {
                    CacheReturnResult::MissWithWrongPermission
                } else {
                    CacheReturnResult::Hit
                }
            }
            None => CacheReturnResult::Miss,
        };
    }

    pub fn invalidate(&mut self, block_id: usize) -> Option<BlockState> {
        let set_index: usize = block_id % self.sets.len();
        let mut set = self.sets.get_mut(set_index).unwrap().lock().unwrap();
        return set.pop(&block_id);
    }

    pub fn is_fully_warmed_up(&self) -> bool {
        return self.warmed_count.load(Ordering::Relaxed) == self.sets.len();
    }

    pub fn usage(&self) -> f64 {
        let usage = self
            .sets
            .iter()
            .map(|x| -> usize { x.lock().unwrap().len() })
            .sum::<usize>() as f64;

        return usage / (self.sets.len() * self.associativity.load(Ordering::Relaxed)) as f64;
    }
}

