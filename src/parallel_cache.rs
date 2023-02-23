use lru::LruCache;
use core::num::NonZeroUsize;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub const BLOCK_SIZE_LOG2: usize = 6;
pub const BLOCK_SIZE: usize = 1 << 6;

pub struct ParallelCache {
    sets: Vec<Mutex<LruCache<usize, bool>>>,
    associativity: AtomicUsize,
    warmed_count: AtomicUsize,
}


impl ParallelCache {
    pub fn new(set_count: usize, associativity: usize) -> Self {
        return ParallelCache {
            sets: (0..set_count)
                .map(|_| {
                    return Mutex::new(LruCache::<usize, bool>::new(NonZeroUsize::new(associativity).unwrap()));
                })
                .collect(),
            associativity: associativity.into(),
            warmed_count: 0.into(),
        };
    }

    pub fn update(&mut self, addr: usize) {
        let set_index = (addr >> BLOCK_SIZE_LOG2) % self.sets.len();
        let mut set = self.sets.get_mut(set_index).unwrap().lock().unwrap();
        let previous_length = set.len();
        set.put(addr >> BLOCK_SIZE_LOG2, true);
        if previous_length == (self.associativity.load(Ordering::Relaxed) - 1) && set.len() == self.associativity.load(Ordering::Relaxed) {
            // this one is warmed up.
            self.warmed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn invalidate(&mut self, addr: usize) {
        let block_id = addr >> BLOCK_SIZE_LOG2;
        let set_index: usize = block_id % self.sets.len();
        let mut set = self.sets.get_mut(set_index).unwrap().lock().unwrap();
        if set.contains(&block_id) {
            set.put(block_id, false);
            set.demote(&block_id);
        }
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

    pub fn serialize(&self) -> Vec<Vec<Option<usize>>> {
        return self.sets.iter().map(|x| -> Vec<Option<usize>> {
            return x.lock().unwrap().iter().map(|el| -> Option<usize> {
                return match &el.1 {
                    true => Some(*el.0),
                    false => None,
                }
            }).collect()
        }).collect();
    }
}
