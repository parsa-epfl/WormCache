use dashmap::mapref::one::RefMut;
use dashmap::DashMap;

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

// Probably the Directory should be infinitely sized.
pub struct Directory {
    // entries: [RwLock<DirectorySet>; SET], // map :: block_id -> DirectoryEntry
    entries: DashMap<u64, DirectoryEntry>,
}

impl Directory {
    pub fn new() -> Self {
        Self {
            // entries: std::array::from_fn(|_| RwLock::new(DirectorySet::new())),
            entries: DashMap::with_shard_amount(SHARED_COUNT),
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
                        sharers: BitArray::ZERO,
                        modify_ts_before_eviction: 0, // zero is a good initialize value, because all timestamp must not be 0.
                    },
                );
                self.entries.get_mut(&block_id).unwrap()
            }
        }
    }

    pub fn mark_as_useless(&self, block_id: u64) {
        // self.entries.remove(&block_id);
    }
}
