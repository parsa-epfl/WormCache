use std::collections::HashMap;
use std::sync::RwLock;

use bitvec::prelude::*;
use bitvec::BitArr;

pub type SharerList = BitArr!(for crate::parameter::CORE_COUNT, in u64, Lsb0);

// pub struct DirectoryEntry {
//     owner: Option<CoreId>,
//     sharers: SharerList,
// }

pub struct DirectoryEntry {
    ts: u64,
    sharers: SharerList,
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

    pub fn peek(&self, block_id: u64) -> SharerList {
        if self.entries.contains_key(&block_id) {
            return self.entries[&block_id].sharers;
        } else {
            return SharerList::ZERO;
        }
    }

    pub fn write(&mut self, block_id: u64, ts: u64, incoming_sharer: SharerList) -> SharerList {
        if self.entries.contains_key(&block_id) {
            // if incoming_sharer is all zero, we just empty the element from set.
            if incoming_sharer.count_ones() == 0 {
                self.entries.remove(&block_id);
                return SharerList::ZERO;
            }
            let entry = self.entries.get_mut(&block_id).unwrap();
            let old_sharers = entry.sharers;
            entry.ts = ts;
            entry.sharers = incoming_sharer;
            return old_sharers;
        } else {
            if incoming_sharer.count_ones() == 0 {
                return SharerList::ZERO;
            }
            self.entries.insert(
                block_id,
                DirectoryEntry {
                    ts,
                    sharers: incoming_sharer,
                },
            );
            return SharerList::ZERO;
        }
    }
}

// Probably the Directory should be infinitely sized.
pub struct Directory<const SET: usize> {
    entries: [RwLock<DirectorySet>; SET], // map :: block_id -> DirectoryEntry
}

impl<const SET: usize> Directory<SET> {
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| RwLock::new(DirectorySet::new())),
        }
    }

    pub fn get_set(&self, block_id: u64) -> &RwLock<DirectorySet> {
        return &self.entries[(block_id as usize) % SET];
    }
}
