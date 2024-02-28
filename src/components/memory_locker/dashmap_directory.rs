use dashmap::mapref::one::RefMut;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Mutex;

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
}

// Probably the Directory should be infinitely sized.
pub struct Directory {
    // entries: [RwLock<DirectorySet>; SET], // map :: block_id -> DirectoryEntry
    entries: DashMap<u64, DirectoryEntry>,
}

impl Directory {
    pub fn new() -> Self {
        Self {
            // entries: std::array::from_fn(|_| RwLock::new(DirectorySet::new())),
            entries: DashMap::with_shard_amount(crate::parameter::PRI_CACHE_SET),
        }
    }

    pub fn get_or_create(&self, block_id: u64) -> RefMut<'_, u64, DirectoryEntry> {
        match self.entries.get_mut(&block_id) {
            Some(entry) => entry,
            None => {
                self.entries.insert(
                    block_id,
                    DirectoryEntry {
                        ts: 0,
                        sharers: SharerList::ZERO,
                    },
                );
                self.entries.get_mut(&block_id).unwrap()
            }
        }
    }

    pub fn mark_as_useless(&self, block_id: u64) {
        // self.entries.remove(&block_id);
    }

    pub fn peek(&self, block_id: u64) -> SharerList {
        match self.entries.get(&block_id) {
            Some(entry) => entry.sharers,
            None => SharerList::ZERO,
        }
    }

    pub fn write(&self, block_id: u64, ts: u64, incoming_sharer: SharerList) -> SharerList {
        match self.entries.get_mut(&block_id) {
            Some(mut entry) => {
                let old_sharers = entry.sharers;
                entry.ts = ts;
                entry.sharers = incoming_sharer;
                return old_sharers;
            }
            None => {
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
}
