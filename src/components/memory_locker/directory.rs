use std::sync::RwLock;
use std::collections::HashMap;

use bitvec::prelude::*;
use bitvec::BitArr;

type CoreId = u8;
type SharerList = BitArr!(for 64, in u64, Lsb0);


// pub struct DirectoryEntry {
//     owner: Option<CoreId>,
//     sharers: SharerList,
// }

pub struct DirectoryEntry {
    ts: u64,
    sharers: SharerList,
}


// Probably the Directory should be infinitely sized.
pub struct Directory <
    const SET: usize,
> {
    entries: [RwLock<HashMap<u64, DirectoryEntry>>; SET], // map :: block_id -> DirectoryEntry
}

impl <
    const SET: usize,
> Directory <SET> {
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| RwLock::new(HashMap::new())),
        }
    }

    pub fn peek(&self, block_id: u64) -> SharerList {
        let set = block_id as usize % SET;
        let entries = self.entries[set].read().unwrap();

        if entries.contains_key(&block_id) {
            return entries[&block_id].sharers;
        } else {
            return SharerList::ZERO;
        }
    }

    pub fn replace_or_add(&self, block_id: u64, ts: u64, incoming_sharer: SharerList) -> SharerList {
        let set = block_id as usize % SET;
        let mut entries = self.entries[set].write().unwrap();

        if entries.contains_key(&block_id) {
            if incoming_sharer.count_ones() == 0 {
                // disable sharing means eviction.
                entries.remove(&block_id);
                return incoming_sharer;
            }
            let entry = entries.get_mut(&block_id).unwrap();
            let old_sharers = entry.sharers;
            entry.ts = ts;
            entry.sharers = incoming_sharer;
            return old_sharers;
        } else {
            if incoming_sharer.count_ones() == 0 {
                // disable sharing means eviction.
                return incoming_sharer;
            }
            entries.insert(block_id, DirectoryEntry {
                ts,
                sharers: incoming_sharer,
            });
            return SharerList::ZERO;
        }
    }
}