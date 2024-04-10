use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::MutexGuard;

use bitvec::prelude::*;
use bitvec::BitArr;

use crate::parameter;

const SHARED_LIST_LENGTH: usize = if parameter::USE_UNIFIED_CACHE {
    parameter::CORE_COUNT
} else {
    parameter::CORE_COUNT * 2
};

const SHARED_COUNT: usize = if parameter::USE_UNIFIED_CACHE {
    parameter::UNIFIED_PRI_CACHE_SET
} else {
    if parameter::HARVARD_PRI_I_CACHE_ASSO > parameter::HARVARD_PRI_D_CACHE_ASSO {
        parameter::HARVARD_PRI_I_CACHE_SET
    } else {
        parameter::HARVARD_PRI_D_CACHE_SET
    }
};

pub type SharerList = BitArr!(for SHARED_LIST_LENGTH, in u64, Lsb0);

// pub struct DirectoryEntry {
//     owner: Option<CoreId>,
//     sharers: SharerList,
// }

pub struct DirectoryEntry {
    pub ts: u64,
    pub sharers: SharerList,
    pub modify_ts_before_eviction: u64, // This field is to avoid the eviction causes the write history to be lost.
}

#[repr(align(64))]
pub struct DirectorySet<const SET: usize> {
    entries: HashMap<u64, DirectoryEntry>,
    index: usize,
}

impl<const SET: usize> DirectorySet<SET> {
    pub fn new(index: usize) -> Self {
        Self {
            entries: HashMap::new(),
            index,
        }
    }

    const LOG2_SET: usize = SET.trailing_zeros() as usize;

    pub fn exists(&self, block_id: u64) -> bool {
        let internal_id = block_id >> Self::LOG2_SET;

        // Internal id is more efficient than block_id, because it removes the same lower bits.
        self.entries.contains_key(&internal_id)
    }

    // pub fn create(&mut self, block_id: u64) -> &mut DirectoryEntry {
    //     let internal_id = block_id >> Self::LOG2_SET;

    //     self.entries.insert(
    //         internal_id,
    //         DirectoryEntry {
    //             ts: 0,
    //             sharers: SharerList::ZERO,
    //             modify_ts_before_eviction: 0,
    //         },
    //     );

    //     self.entries.get_mut(&internal_id).unwrap()
    // }

    // pub fn get_entry(&mut self, block_id: u64) -> &mut DirectoryEntry {
    //     let internal_id = block_id >> Self::LOG2_SET;

    //     self.entries.get_mut(&internal_id).unwrap()
    // }

    pub fn get_or_create(&mut self, block_id: u64) -> &mut DirectoryEntry {
        let internal_id = block_id >> Self::LOG2_SET;

        if !self.entries.contains_key(&internal_id) {
            self.entries.insert(
                internal_id,
                DirectoryEntry {
                    ts: 0,
                    sharers: SharerList::ZERO,
                    modify_ts_before_eviction: 0,
                },
            );
        }

        self.entries.get_mut(&internal_id).unwrap()
    }
}

// Probably the Directory should be infinitely sized.
pub struct Directory<const SET: usize> {
    entries: [Mutex<DirectorySet<SET>>; SET],
}

impl<const SET: usize> Directory<SET> {
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|idx| Mutex::new(DirectorySet::new(idx))),
        }
    }

    pub fn get_set(&self, block_id: u64) -> MutexGuard<'_, DirectorySet<SET>> {
        let set_id = (block_id as usize) % SET;
        self.entries[set_id].lock().unwrap()
    }
}
