use spin::mutex::SpinMutex;

use crate::components::cache_hierarchy::CacheBlockRequest;
use super::util;

#[derive(Debug, Clone, Copy)]
struct AccTableTagEntry<
    const N_BLK: usize,
> {
    pub tag_with_v: u64,
}

impl <
    const N_BLK: usize,
> AccTableTagEntry<N_BLK> 
{
    pub const fn new() -> Self {
        Self {
            tag_with_v: 0,
        }
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        (self.tag_with_v & 0x1) != 0
    }

    #[inline]
    pub fn get_tag(&self) -> u64 {
        self.tag_with_v >> 1
    }

    #[inline]
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Debug, Clone, Copy)]
struct AccTableDataEntry<
    const N_BLK: usize,
> {
    pub pc: u64,
    pub offset: u64,
    pub access_pattern: [bool; N_BLK],
    pub read_pattern: [bool; N_BLK],
    pub ts: u64,
}

impl <
    const N_BLK: usize,
> AccTableDataEntry<N_BLK> 
{
    pub const fn new() -> Self {
        Self {
            pc: 0,
            offset: 0,
            access_pattern: [false; N_BLK],
            read_pattern: [false; N_BLK],
            ts: 0,
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

// To preserve outside APIs
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

    fn from_tag_data(tag_entry: &AccTableTagEntry<N_BLK>, data_entry: &AccTableDataEntry<N_BLK>) -> Self {
        Self {
            tag: tag_entry.get_tag(),
            pc: data_entry.pc,
            offset: data_entry.offset,
            access_pattern: data_entry.access_pattern,
            read_pattern: data_entry.read_pattern,
            ts: data_entry.ts,
            valid: tag_entry.is_valid(),
        }
    }

    fn split_tag_data(&self) -> (AccTableTagEntry<N_BLK>, AccTableDataEntry<N_BLK>) {
        let tag_entry = AccTableTagEntry {
            tag_with_v: (self.tag << 1) | if self.valid { 1 } else { 0 },
        };
        let data_entry = AccTableDataEntry {
            pc: self.pc,
            offset: self.offset,
            access_pattern: self.access_pattern,
            read_pattern: self.read_pattern,
            ts: self.ts,
        };
        (tag_entry, data_entry)
    }
}

#[derive(Debug)]
pub struct AccTable<
    const N_ACC: usize,
    const N_BLK: usize,
> {
    tag_entries: SpinMutex<[AccTableTagEntry<N_BLK>; N_ACC]>,
    data_entries: SpinMutex<[AccTableDataEntry<N_BLK>; N_ACC]>,
}

impl<
    const N_ACC: usize,
    const N_BLK: usize,
> AccTable<N_ACC, N_BLK>
{
    pub const fn new() -> Self {
        Self {
            tag_entries: SpinMutex::new([AccTableTagEntry::new(); N_ACC]),
            data_entries: SpinMutex::new([AccTableDataEntry::new(); N_ACC]),
        }
    }

    #[inline]
    fn get_base_pc_offset(&self, request: &CacheBlockRequest) -> (u64, u64, u64) {
        util::get_base_pc_offset::<N_BLK>(request)
    }

    fn poke_index(&self, tag_entries: &[AccTableTagEntry<N_BLK>], base: u64) -> Option<usize> {
        let base_tag_with_v = (base << 1) | 0x1;
        for (i, entry) in tag_entries.iter().enumerate() {
            if entry.tag_with_v == base_tag_with_v {
                return Some(i);
            }
        }
        None
    }

    fn get_free_index(&self, tag_entries: &[AccTableTagEntry<N_BLK>]) -> Option<usize> {
        for (i, entry) in tag_entries.iter().enumerate() {
            if !entry.is_valid() {
                return Some(i);
            }
        }
        None
    }

    fn get_min_ts(&self, data_entries: &[AccTableDataEntry<N_BLK>]) -> usize {
        let mut min_idx = 0;
        let mut min_ts = u64::MAX;
        for (i, entry) in data_entries.iter().enumerate() {
            if entry.ts < min_ts {
                min_ts = entry.ts;
                min_idx = i;
            }
        }
        min_idx
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>) -> Option<AccTableEntry<N_BLK>> {
        assert!(entry.valid, "Cannot insert invalid entry into AccTable");
        let (tag_entry, data_entry) = entry.split_tag_data();

        let mut tag_entries = self.tag_entries.lock();
        let mut data_entries = self.data_entries.lock();

        if let Some(idx) = self.get_free_index(&(*tag_entries)) {
            tag_entries[idx] = tag_entry;
            data_entries[idx] = data_entry;
            return None;
        }
        let lru_idx = self.get_min_ts(&(*data_entries));
        let evicted_entry = AccTableEntry::from_tag_data(&tag_entries[lru_idx], &data_entries[lru_idx]);
        tag_entries[lru_idx] = tag_entry;
        data_entries[lru_idx] = data_entry;
        Some(evicted_entry)
    }

    pub fn poke_and_update(&self, request: &CacheBlockRequest, ts: u64) -> bool {
        let (base, _, offset) = self.get_base_pc_offset(request);
        let is_store = request.is_store();

        let tag_entries = self.tag_entries.lock();
        let mut data_entries = self.data_entries.lock();    // TODO: What if you cannot?

        if let Some(idx) = self.poke_index(&(*tag_entries), base) {
            if !data_entries[idx].access_pattern[offset as usize] {
                data_entries[idx].access_pattern[offset as usize] = true;
                data_entries[idx].read_pattern[offset as usize] = !is_store;
            }
            data_entries[idx].ts = ts;
            return true;
        }
        false
    }

    pub fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        let (base, _, _) = self.get_base_pc_offset(request);

        let mut tag_entries = self.tag_entries.lock();
        let mut data_entries = self.data_entries.lock();

        if let Some(idx) = self.poke_index(&(*tag_entries), base) {
            let evicted_entry = AccTableEntry::from_tag_data(&tag_entries[idx], &data_entries[idx]);
            tag_entries[idx].reset();
            data_entries[idx].reset();
            return Some(evicted_entry);
        }
        None
    }
}