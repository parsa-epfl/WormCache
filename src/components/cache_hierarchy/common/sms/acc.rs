use spin::mutex::SpinMutex;

use crate::components::cache_hierarchy::CacheBlockRequest;
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
    pub const fn new() -> Self {
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
    const N_ACC: usize,
    const N_BLK: usize,
> {
    entries: SpinMutex<[AccTableEntry<N_BLK>; N_ACC]>,
}

impl<
    const N_ACC: usize,
    const N_BLK: usize,
> AccTable<N_ACC, N_BLK>
{
    pub fn new() -> Self {
        Self {
            entries: SpinMutex::new([const { AccTableEntry::<N_BLK>::new() }; N_ACC]),
        }
    }

    #[inline]
    fn get_base_pc_offset(&self, request: &CacheBlockRequest) -> (u64, u64, u64) {
        util::get_base_pc_offset::<N_BLK>(request)
    }

    fn poke_index(&self, entries: &[AccTableEntry<N_BLK>; N_ACC], base: u64) -> Option<usize> {
        for (i, entry) in entries.iter().enumerate() {
            if entry.valid && entry.tag == base {
                return Some(i);
            }
        }
        None
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>) -> Option<AccTableEntry<N_BLK>> {
        assert!(entry.valid, "Cannot insert invalid entry into AccTable");
        
        let mut entries = self.entries.lock();
        let mut lru_ts = u64::MAX;
        let mut lru_idx: Option<usize> = None;
        
        for (i, current_entry) in entries.iter().enumerate() {
            if !current_entry.valid {
                entries[i].replace(entry);
                return None; // Entry was inserted, no eviction needed
            }
            if current_entry.ts < lru_ts {
                lru_ts = current_entry.ts;
                lru_idx = Some(i);
            }
        }

        // All entries were valid, evict LRU
        let idx = lru_idx.expect("AccTable must contain at least one entry and all were valid");
        let dropped_entry_clone = entries[idx].clone();
        entries[idx].replace(entry);
        assert!(dropped_entry_clone.ts < entry.ts, "Cannot insert a block from past");
        Some(dropped_entry_clone)
    }

    pub fn poke_and_update(&self, request: &CacheBlockRequest, ts: u64) -> bool {
        let (base, _, offset) = self.get_base_pc_offset(request);
        let is_store = request.is_store();
        
        let mut entries = self.entries.lock();
        
        if let Some(idx) = self.poke_index(&entries, base) {
            if !entries[idx].access_pattern[offset as usize] {
                entries[idx].access_pattern[offset as usize] = true;
                entries[idx].read_pattern[offset as usize] = !is_store;
            }
            entries[idx].ts = ts;
            return true;
        }
        false
    }

    pub fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        let (base, _, _) = self.get_base_pc_offset(request);
        
        let mut entries = self.entries.lock();
        
        if let Some(idx) = self.poke_index(&entries, base) {
            let evicted_entry = entries[idx].clone();
            entries[idx].reset();
            return Some(evicted_entry);
        }
        None
    }
}