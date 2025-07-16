use crate::components::cache_hierarchy::CacheBlockRequest;
use super::super::CCell;
use super::acc::AccTableEntry;
use super::util;

#[derive(Debug, Clone, Copy)]
pub struct FilterTableEntry {
    pub tag: u64,
    pub pc: u64,
    pub offset: u64,
    pub is_read: bool,
    pub ts: u64,
    pub valid: bool,
}

impl FilterTableEntry {
    pub fn new() -> Self {
        Self {
            tag: 0,
            pc: 0,
            offset: 0,
            is_read: false,
            ts: 0,
            valid: false,
        }
    }

    pub fn replace(&mut self, new_entry: &FilterTableEntry) {
        *self = *new_entry;
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Debug)]
pub struct FilterTable<
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const N_FILTER: usize,  // Number of entries in the filter table
    const N_BLK: usize,     // Number of blocks per spatial region
> {
    entries: Box<[GFilter; N_FILTER]>,
}

impl<
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const N_FILTER: usize,
    const N_BLK: usize,
> FilterTable<GFilter, N_FILTER, N_BLK>
{
    pub fn new() -> Self {
        Self {
            entries: crate::util::init_heap_array(|_| GFilter::new(FilterTableEntry::new())),
        }
    }

    #[inline]
    fn get_base_pc_offset(&self, request: &CacheBlockRequest) -> (u64, u64, u64) {
        util::get_base_pc_offset::<N_BLK>(request)
    }

    fn poke(&self, request: &CacheBlockRequest) -> Option<usize> {
        let (base, _, _) = self.get_base_pc_offset(request);
        for (i, locked_entry) in self.entries.iter().enumerate() {
            let current_entry = locked_entry.inner();
            if current_entry.valid && current_entry.tag == base {
                return Some(i);
            }
            drop(current_entry);
        }
        None
    }

    fn insert(&self, entry: &FilterTableEntry) {
        let mut lru_idx = 0;
        let mut lru_ts = u64::MAX;
        for  (i, locked_entry) in self.entries.iter().enumerate() {
            let mut current_entry = locked_entry.inner();
            if !current_entry.valid {
                assert!(entry.valid, "Cannot insert invalid entry into FilterTable");
                current_entry.replace(entry);
                return;
            }
            if current_entry.ts < lru_ts {  // current_entry.valid is true implicitly
                lru_ts = current_entry.ts;
                lru_idx = i;
            }
            drop(current_entry);
        }
        // Replace the LRU entry with the new entry
        self.entries[lru_idx].inner().replace(entry);
    }

    pub fn poke_and_update(&self, request: &CacheBlockRequest, ts: u64) -> Option<AccTableEntry<N_BLK>> {
        let (base, pc, offset) = self.get_base_pc_offset(request);
        match self.poke(request) {
            Some(idx) => {  // entry found, check further
                let mut existing_entry = self.entries[idx].inner();
                if offset == existing_entry.offset {
                    existing_entry.pc = pc;
                    existing_entry.is_read = !request.is_store();
                    existing_entry.ts = ts;
                    return None;
                } else {    // must be promoted to acc entry, TODO: is there a more efficient way?
                    let mut acc_entry = AccTableEntry::<N_BLK>::new();
                    acc_entry.tag = existing_entry.tag;
                    acc_entry.pc = existing_entry.pc;
                    acc_entry.offset = existing_entry.offset;
                    acc_entry.access_pattern[existing_entry.offset as usize] = true;
                    acc_entry.access_pattern[offset as usize] = true;
                    acc_entry.read_pattern[existing_entry.offset as usize] = existing_entry.is_read;
                    acc_entry.read_pattern[offset as usize] = !request.is_store();
                    acc_entry.ts = ts;
                    acc_entry.valid = true;
                    existing_entry.reset(); // reset the filter entry
                    return Some(acc_entry);
                }
            }
            None => {       // new entry must be allocated
                let mut new_entry = FilterTableEntry::new();
                new_entry.tag = base;
                new_entry.pc = pc;
                new_entry.offset = offset;
                new_entry.is_read = !request.is_store();
                new_entry.ts = ts;
                new_entry.valid = true;

                // Insert the new entry into the filter table
                self.insert(&new_entry);
                return None;
            }
        }
    }

    pub fn evict(&self, request: &CacheBlockRequest) {
        match self.poke(request) {
            Some(idx) => self.entries[idx].inner().reset(),
            None => {}
        }
    }
}