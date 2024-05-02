use spin::mutex::SpinMutex;
use spin::mutex::SpinMutexGuard;
use std::collections::HashMap;

use bitvec::prelude::*;
use bitvec::BitArr;
use serde::Serialize;

use crate::parameter;

const SHARED_LIST_LENGTH: usize = if parameter::USE_UNIFIED_CACHE {
    parameter::CORE_COUNT
} else {
    parameter::CORE_COUNT * 2
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
    pub index: usize,
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

    pub fn get_or_create(&mut self, block_id: u64) -> &mut DirectoryEntry {
        let internal_id = block_id >> Self::LOG2_SET;

        self.entries.entry(internal_id).or_insert(DirectoryEntry {
                    ts: 0,
                    sharers: SharerList::ZERO,
                    modify_ts_before_eviction: 0,
                });

        self.entries.get_mut(&internal_id).unwrap()
    }
}

// Probably the Directory should be infinitely sized.
pub struct Directory<const SET: usize> {
    entries: [SpinMutex<DirectorySet<SET>>; SET],
}

impl<const SET: usize> Default for Directory<SET> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SET: usize> Directory<SET> {
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|idx| SpinMutex::new(DirectorySet::new(idx))),
        }
    }

    pub fn get_set(&self, block_id: u64) -> SpinMutexGuard<'_, DirectorySet<SET>> {
        let set_id = (block_id as usize) % SET;
        self.entries[set_id].lock()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct SerializedDirectoryEntry {
    pub tag: u64,
    pub sharers: SharerList,
}

use serde::ser::{SerializeStruct, Serializer};

impl Serialize for SerializedDirectoryEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("SerializedDirectoryEntry", 2)?;
        state.serialize_field("ts", &self.tag)?;
        state.serialize_field("sharers", &self.sharers.as_raw_slice())?;
        state.end()
    }
}

impl<const SET: usize> Directory<SET> {
    pub fn dump_snapshot(&self, snapshot_folder: &str) {
        let file = std::fs::File::create(format!("{}/directory.json", snapshot_folder)).unwrap();

        let entries = self
            .entries
            .iter()
            .flat_map(|set| {
                let set = set.lock();
                set.entries
                    .iter()
                    .filter_map(|(tag, entry)| {
                        if entry.sharers.not_any() {
                            return None;
                        }
                        Some(SerializedDirectoryEntry {
                            tag: (*tag) * (SET as u64) + set.index as u64,
                            sharers: entry.sharers,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        serde_json::to_writer_pretty(&file, &entries).unwrap();
    }
}
