use std::sync::Mutex;
use std::sync::MutexGuard;

use std::collections::btree_map::BTreeMap;

use crate::util;

use bitvec::prelude::*;
use bitvec::BitArr;

pub type SharerList = BitArr!(for crate::parameter::CORE_COUNT, in u64, Lsb0);

#[derive(Debug)]
pub struct DirectoryEntry {
    pub ts: u64,
    pub sharers: SharerList,
}

impl DirectoryEntry {
    pub fn new() -> Self {
        Self {
            ts: 0,
            sharers: SharerList::ZERO,
        }
    }
}

#[derive(Debug)]
#[repr(align(64))]
pub struct DirectorySet<const WAYS: usize> {
    // tags: Box<[u64; WAYS]>,
    // entries: Box<[DirectoryEntry; WAYS]>,
    entries: BTreeMap<u64, DirectoryEntry>,
}

impl<const WAYS: usize> DirectorySet<WAYS> {
    pub fn new() -> Self {
        Self {
            // tags: util::init_heap_array(|_| 0),
            // entries: util::init_heap_array(|_| DirectoryEntry::new()),
            entries: BTreeMap::new(),
        }
    }

    pub fn exists(&self, block_id: u64) -> bool {
        let internal_tag = block_id << 1 | 1;
        // self.tags
        //     .iter()
        //     .any(|entry| (*entry) == internal_tag)
        self.entries.contains_key(&internal_tag)
    }

    pub fn get_mut(&mut self, block_id: u64) -> &mut DirectoryEntry {
        let internal_tag = block_id << 1 | 1;

        let hit = self.entries.get_mut(&internal_tag);

        if let Some(index) = hit {
            index
        } else {
            panic!("No such entry found.")
        }
    }

    pub fn create(&mut self, block_id: u64) -> &mut DirectoryEntry {
        // let invalid = self.tags.iter().enumerate().find(|entry| (*entry.1) == 0);

        let internal_tag = block_id << 1 | 1;
        self.entries.insert(internal_tag, DirectoryEntry::new());
        self.entries.get_mut(&internal_tag).unwrap()

        // if let Some((index, _)) = invalid {
        //     self.tags[index] = block_id << 1 | 1;
        //     &mut self.entries[index]
        // } else {
        //     panic!("No invalid entry found.")
        // }
    }

    pub fn invalidate(&mut self, block_id: u64) {
        // let hit = self
        //     .tags
        //     .iter()
        //     .enumerate()
        //     .find(|entry| (*entry.1) == (block_id << 1 | 1));

        // if let Some((index, _)) = hit {
        //     self.tags[index] = 0;
        // }
        let internal_tag = block_id << 1 | 1;
        self.entries.remove(&internal_tag);
    }
}

#[derive(Debug)]
pub struct ReplicaDirectory<const SETS: usize, const WAYS: usize> {
    pub sets: Box<[Mutex<DirectorySet<WAYS>>; SETS]>,
}

impl<const SETS: usize, const WAYS: usize> ReplicaDirectory<SETS, WAYS> {
    pub fn new() -> Self {
        Self {
            sets: util::init_heap_array(|_| Mutex::new(DirectorySet::new())),
        }
    }

    pub fn get_set(&self, block_id: u64) -> MutexGuard<'_, DirectorySet<WAYS>> {
        let index = (block_id as usize) % SETS;
        let guard = self.sets[index].lock().unwrap();
        guard
    }
}
