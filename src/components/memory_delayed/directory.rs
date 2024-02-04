use std::sync::RwLock;
use std::collections::HashMap;

use bitvec::prelude::*;
use bitvec::BitArr;

use crate::parameter::CORE_COUNT;

pub type SharerList = BitArr!(for CORE_COUNT, in u64, Lsb0);


// pub struct DirectoryEntry {
//     owner: Option<CoreId>,
//     sharers: SharerList,
// }

pub struct DirectoryEntry {
    ts: u64,
    sharers: SharerList,
    sharers_ts: [u64; CORE_COUNT]
}

impl DirectoryEntry {
    fn is_exclusive(&self) -> bool {
        return self.sharers.count_ones() == 1;
    }

    fn is_valid(&self) -> bool {
        return self.sharers.count_ones() > 0;
    }

    fn is_shared(&self) -> bool {
        return self.sharers.count_ones() > 1;
    }
}

// pub struct DirectorySet {
//     pub entries: ,
// }


// impl DirectorySet {
//     pub fn new() -> Self {
//         Self {
//             entries: HashMap::new(),
//         }
//     }

    // pub fn peek(&self, block_id: u64) -> SharerList {
    //     if self.entries.contains_key(&block_id) {
    //         return self.entries[&block_id].sharers;
    //     } else {
    //         return SharerList::ZERO;
    //     }
    // }

    // pub fn write(&mut self, block_id: u64, ts: u64, incoming_sharer: SharerList) -> SharerList {
    //     if self.entries.contains_key(&block_id) {
    //         // if incoming_sharer is all zero, we just empty the element from set.
    //         if incoming_sharer.count_ones() == 0 {
    //             self.entries.remove(&block_id);
    //             return SharerList::ZERO;
    //         }
    //         let entry = self.entries.get_mut(&block_id).unwrap();
    //         let old_sharers = entry.sharers;
    //         entry.ts = ts;
    //         entry.sharers = incoming_sharer;
    //         return old_sharers;
    //     } else {
    //         if incoming_sharer.count_ones() == 0 {
    //             return SharerList::ZERO;
    //         }
    //         self.entries.insert(block_id, DirectoryEntry {
    //             ts,
    //             sharers: incoming_sharer,
    //         });
    //         return SharerList::ZERO;
    //     }
    // }

    // // return true if this core is added to the sharer list. It is possible that this block is taken by other core.
    // pub fn add_sharer(&mut self, block_id: u64, ts: u64, core_id: CoreId) -> bool {
    //     return false;
    // }

    // // return true if this core takes the ownership. After this, you need to clean the outdated sharers.
    // pub fn create_exclusiveness(&mut self, block_id: u64, ts: u64, core_id: CoreId) -> bool {
    //     return false;
    // }

    // // return true if this one core is the only sharer.
    // pub fn remove_sharer(&mut self, block_id: u64, ts: u64, core_id: CoreId) -> bool {
    //     return false;
    // }


// }

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
            entries: std::array::from_fn(|_| RwLock::new(HashMap::<u64, DirectoryEntry>::new())),
        }
    }

    pub fn get_set(&self, block_id: u64) -> &RwLock<HashMap<u64, DirectoryEntry>> {
        return &self.entries[(block_id as usize) % SET];
    }
}