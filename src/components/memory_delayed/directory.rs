use std::sync::RwLock;
use std::sync::Mutex;
use std::collections::HashMap;

use bitvec::prelude::*;
use bitvec::BitArr;

use crate::parameter::CORE_COUNT;

use dashmap::DashMap;

pub type SharerList = BitArr!(for CORE_COUNT, in u64, Lsb0);

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

pub struct Directory <
    const SET: usize,
> {
    // entries: [RwLock<HashMap<u64, DirectoryEntry>>; SET], // map :: block_id -> DirectoryEntry
    entries: DashMap<u64, Mutex<DirectoryEntry>>
}

impl <
    const SET: usize,
> Directory <SET> {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    pub fn get(&self, block_id: u64) -> Option<Mutex<DirectoryEntry>> {
        unimplemented!();
    }
}