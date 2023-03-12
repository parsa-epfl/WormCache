use lru::LruCache;
use core::num::NonZeroUsize;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use bitvec::vec::BitVec;

pub const BLOCK_SIZE_LOG2: usize = 6;
pub const BLOCK_SIZE: usize = 1 << 6;

#[derive(Clone, Copy, Debug)]
pub enum BlockState {
    Exclusive,
    Modified,
    Shared
}

#[derive(Clone, Copy, Debug)]
struct CacheEntry {
    perm: BlockState
}

#[derive(Clone, Debug)]
struct DirectoryEntry {
    perm: BlockState,
    replicas: BitVec
}

pub struct ParallelCache<E> {
    sets: Vec<Mutex<LruCache<usize, E>>>,
    associativity: AtomicUsize,
    warmed_count: AtomicUsize,
}


impl<E> ParallelCache<E> {
    pub fn new(set_count: usize, associativity: usize) -> Self {
        return ParallelCache {
            sets: (0..set_count)
                .map(|_| {
                    return Mutex::new(LruCache::<usize, E>::new(NonZeroUsize::new(associativity).unwrap()));
                })
                .collect(),
            associativity: associativity.into(),
            warmed_count: 0.into(),
        };
    }

    // pub fn update(&mut self, addr: usize) {
    //     let set_index = (addr >> BLOCK_SIZE_LOG2) % self.sets.len();
    //     let mut set = self.sets.get_mut(set_index).unwrap().lock().unwrap();
    //     let previous_length = set.len();
    //     set.put(addr >> BLOCK_SIZE_LOG2, true);
    //     if previous_length == (self.associativity.load(Ordering::Relaxed) - 1) && set.len() == self.associativity.load(Ordering::Relaxed) {
    //         // this one is warmed up.
    //         self.warmed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    //     }
    // }

    pub fn invalidate(&mut self, addr: usize) -> Option<E> {
        let block_id = addr >> BLOCK_SIZE_LOG2;
        let set_index: usize = block_id % self.sets.len();
        let mut set = self.sets.get_mut(set_index).unwrap().lock().unwrap();
        return set.pop(&addr);
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

    // pub fn serialize(&self) -> Vec<Vec<Option<usize>>> {
    //     return self.sets.iter().map(|x| -> Vec<Option<usize>> {
    //         return x.lock().unwrap().iter().map(|el| -> Option<usize> {
    //             return match &el.1 {
    //                 true => Some(*el.0),
    //                 false => None,
    //             }
    //         }).collect()
    //     }).collect();
    // }
}


impl ParallelCache<CacheEntry> {
    pub fn update(&mut self, addr: usize, perm: BlockState) -> Option<(usize, CacheEntry)> {
        let set_index = (addr >> BLOCK_SIZE_LOG2) % self.sets.len();
        let mut set = self.sets.get_mut(set_index).unwrap().lock().unwrap();
        let previous_length = set.len();
        let replaced = set.push(addr >> BLOCK_SIZE_LOG2, CacheEntry{ perm });
        let new_length = set.len();
        drop(new_length);
        if previous_length == (self.associativity.load(Ordering::Relaxed) - 1) && new_length == self.associativity.load(Ordering::Relaxed) {
            // this one is warmed up.
            self.warmed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        match replaced {
            Some((evicted_addr, entry)) => {
                if evicted_addr != addr {
                    return Some((evicted_addr, entry))
                } else {
                    return None;
                }
            },
            None => return None,
        }
    }
}

impl ParallelCache<DirectoryEntry> {
    pub fn update(&mut self, addr: usize, perm: BlockState, cpu_id: usize) -> Option<(usize, DirectoryEntry)> {
        let set_index = (addr >> BLOCK_SIZE_LOG2) % self.sets.len();
        let mut set = self.sets.get_mut(set_index).unwrap().lock().unwrap();
        // first, check whether this entry is in the Directory. 
        if let Some(e) = set.get_mut(&addr) {
            // well, it is a permission transfer.
            match (e.perm, perm) {
                (BlockState::Exclusive, BlockState::Exclusive) => {},
                (BlockState::Exclusive, BlockState::Modified) => {
                    e.perm = perm;
                },
                (BlockState::Exclusive, BlockState::Shared) => {
                    e.perm = perm;
                },
                (BlockState::Modified, BlockState::Exclusive) => {
                    // requires a write back, but it is not done here. 
                    // if we want to keep it atomic, we may need to acquire two locks, thus possible to have deadlock?
                    e.perm = perm
                },
                (BlockState::Modified, BlockState::Modified) => {},
                (BlockState::Modified, BlockState::Shared) => {
                    // well, this should trigger a write back as well.
                    e.perm = perm
                },
                (BlockState::Shared, BlockState::Exclusive) => {
                    // this requires an eviction to other platform
                },
                (BlockState::Shared, BlockState::Modified) => {
                    
                },
                (BlockState::Shared, BlockState::Shared) => {},
            }
            return Some((addr, e.clone()));
        } else {
            // insert the 
        }

        return None;
    }
}


mod test {

    use chrono::Local;

    use super::{ParallelCache, CacheEntry};

    #[test]
    fn time_operation() {
        let mut ncache = ParallelCache::<CacheEntry>::new(1024, 16);
        let t1 = Local::now();
        for t in 0..1000*1000*10 {
            ncache.update(t, super::BlockState::Exclusive);
        }
        let t2 = Local::now();
        println!("{}", t2 - t1);
    }
}