use std::collections::HashMap;

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

pub struct DirectoryEntry {
    pub ts: u64,
    pub sharers: SharerList,
}

// Probably the Directory should be infinitely sized.
pub struct Directory {
    entries: HashMap<u64, DirectoryEntry>, // block_id -> DirectoryEntry
}

impl Directory {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn get_or_create(&mut self, block_id: u64) -> &mut DirectoryEntry {
        if !self.entries.contains_key(&block_id) {
            self.entries.insert(
                block_id,
                DirectoryEntry {
                    ts: 0,
                    sharers: SharerList::ZERO,
                },
            );
        }

        self.entries.get_mut(&block_id).unwrap()
    }
}
