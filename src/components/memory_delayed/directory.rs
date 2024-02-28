use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use bitvec::prelude::*;
use bitvec::BitArr;

pub type SharerList = BitArr!(for crate::parameter::CORE_COUNT, in u64, Lsb0);

// pub struct DirectoryEntry {
//     owner: Option<CoreId>,
//     sharers: SharerList,
// }

pub struct DirectoryEntry {
    pub ts: u64,
    pub sharers: SharerList,
    // pub history: Vec<(u32, bool, u64, bool)>, // (core_id, is_store, ts, l1miss) // This entry is for debugging.
}

pub struct DirectorySet {
    entries: HashMap<u64, DirectoryEntry>,
}

impl DirectorySet {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn get_or_create(&mut self, block_id: u64) -> &mut DirectoryEntry {
        self.entries.entry(block_id).or_insert(DirectoryEntry {
            ts: 0,
            sharers: SharerList::ZERO,
            // history: Vec::new(),
        })
    }

    pub fn remove(&mut self, block_id: &u64) {
        // self.entries.remove(block_id);
    }
}

// Probably the Directory should be infinitely sized.
pub struct Directory<const SETS: usize> {
    entries: [Mutex<DirectorySet>; SETS],
}

impl<const SETS: usize> Directory<SETS> {
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| Mutex::new(DirectorySet::new())),
        }
    }

    pub fn get_set(&self, block_id: u64) -> MutexGuard<'_, DirectorySet> {
        let index = (block_id as usize) % SETS;
        let guard = self.entries[index].lock().unwrap();
        guard
    }
}
