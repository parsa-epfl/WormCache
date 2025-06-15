use crate::components::cache_hierarchy::CacheBlockRequest;
use super::super::CCell;
use super::util;

#[derive(Debug, Clone, Copy)]
pub struct AccTableEntry<
    const N_BLK: usize,
> {
    pub tag: u64,
    pub pc: u64,
    pub offset: u64,
    pub access_pattern: [bool; N_BLK],
    pub read_pattern: [bool; N_BLK],
    pub ts: u64,
    pub valid: bool,
}

impl <
    const N_BLK: usize,
> AccTableEntry<N_BLK>
{
    pub fn new() -> Self {
        Self {
            tag: 0,
            pc: 0,
            offset: 0,
            access_pattern: [false; N_BLK],
            read_pattern: [false; N_BLK],
            ts: 0,
            valid: false,
        }
    }

    pub fn replace(&mut self, new_entry: &AccTableEntry<N_BLK>) {
        *self = *new_entry;
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Debug)]
pub struct AccTable<
    GAcc: CCell<AccTableEntry<N_BLK>> + std::fmt::Debug,
    const N_ACC: usize,
    const N_BLK: usize,
> {
    entries: Box<[GAcc; N_ACC]>,
}

impl<
    GAcc: CCell<AccTableEntry<N_BLK>> + std::fmt::Debug,
    const N_ACC: usize,
    const N_BLK: usize,
> AccTable<GAcc, N_ACC, N_BLK>
{
    pub fn new() -> Self {
        Self {
            entries: crate::util::init_heap_array(|_| GAcc::new(AccTableEntry::<N_BLK>::new())),
        }
    }

    fn poke(&self, request: &CacheBlockRequest) -> Option<usize> {
        let (base, _, _) = util::get_base_pc_offset(request, N_BLK);
        for (i, locked_entry) in self.entries.iter().enumerate() {
            let current_entry = locked_entry.inner();
            if current_entry.valid && current_entry.tag == base {
                return Some(i);
            }
            drop(current_entry);
        }
        None
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>) -> Option<AccTableEntry<N_BLK>> {
        let mut lru_idx = 0;
        let mut lru_ts = u64::MAX;
        for (i, locked_entry) in self.entries.iter().enumerate() {
            let mut current_entry = locked_entry.inner();
            if !current_entry.valid {
                assert!(entry.valid, "Cannot insert invalid entry into AccTable");
                current_entry.replace(entry);
                return None; // Entry was inserted, no eviction needed
            }
            if current_entry.ts < lru_ts {
                lru_ts = current_entry.ts;
                lru_idx = i;
            }
            drop(current_entry);
        }
        let mut dropped_entry = self.entries[lru_idx].inner();
        let dropped_entry_clone = dropped_entry.clone();
        dropped_entry.replace(entry);
        assert!(dropped_entry_clone.ts < dropped_entry.ts, "Cannot insert a block from past");
        Some(dropped_entry_clone)
    }

    pub fn poke_and_update(&self, request: &CacheBlockRequest, ts: u64) -> bool {   // return true if entry found and updated
        let (_, _, offset) = util::get_base_pc_offset(request, N_BLK);
        match self.poke(request) {
            Some(idx) => {
                let mut existing_entry = self.entries[idx].inner();
                if !existing_entry.access_pattern[offset as usize] {
                    existing_entry.access_pattern[offset as usize] = true;
                    existing_entry.read_pattern[offset as usize] = !request.is_store();
                }
                existing_entry.ts = ts;
                return true;
            }
            None => return false,
        }
    }

    // TODO: what if entry gets evicted due to frequent updates?
    pub fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        match self.poke(request) {
            Some(idx) => {
                let mut entry = self.entries[idx].inner();
                let evicted_entry = entry.clone();
                entry.reset();
                return Some(evicted_entry);
            }
            None => {
                return None; // No entry to evict
            }
        }
    }
}