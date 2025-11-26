use rustc_hash::FxHashMap;
use spin::mutex::SpinMutex;

use crate::components::cache_hierarchy::CacheBlockRequest;
use super::util;

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
}

#[derive(Debug)]
pub struct AccTable<
    const N_ACC: usize,
    const N_BLK: usize,
> {
    entries: SpinMutex<FxHashMap<u64, AccTableDataEntry<N_BLK>>>,
}

impl<
    const N_ACC: usize,
    const N_BLK: usize,
> AccTable<N_ACC, N_BLK>
{
    pub fn new() -> Self {
        Self {
            entries: SpinMutex::new(FxHashMap::default()),
        }
    }

    #[inline]
    fn get_base_pc_offset(&self, request: &CacheBlockRequest) -> (u64, u64, u64) {
        util::get_base_pc_offset::<N_BLK>(request)
    }

    fn get_lru_key(entries: &FxHashMap<u64, AccTableDataEntry<N_BLK>>) -> Option<u64> {
        entries
            .iter()
            .min_by_key(|(_, data)| data.ts)
            .map(|(key, _)| *key)
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>) -> Option<AccTableEntry<N_BLK>> {
        assert!(entry.valid, "Cannot insert invalid entry into AccTable");
        let data_entry = AccTableDataEntry {
            pc: entry.pc,
            offset: entry.offset,
            access_pattern: entry.access_pattern,
            read_pattern: entry.read_pattern,
            ts: entry.ts,
        };

        let mut entries = self.entries.lock();

        // If we have space, insert directly
        if entries.len() < N_ACC {
            entries.insert(entry.tag, data_entry);
            return None;
        }

        // Need to evict LRU entry
        if let Some(lru_key) = Self::get_lru_key(&entries) {
            let evicted_data = entries.remove(&lru_key).unwrap();
            let evicted_entry = AccTableEntry {
                tag: lru_key,
                pc: evicted_data.pc,
                offset: evicted_data.offset,
                access_pattern: evicted_data.access_pattern,
                read_pattern: evicted_data.read_pattern,
                ts: evicted_data.ts,
                valid: true,
            };
            entries.insert(entry.tag, data_entry);
            return Some(evicted_entry);
        }

        None
    }

    pub fn poke_and_update(&self, request: &CacheBlockRequest, ts: u64) -> bool {
        let (base, _, offset) = self.get_base_pc_offset(request);
        let is_store = request.is_store();

        let mut entries = self.entries.lock();

        if let Some(data) = entries.get_mut(&base) {
            if !data.access_pattern[offset as usize] {
                data.access_pattern[offset as usize] = true;
                data.read_pattern[offset as usize] = !is_store;
            }
            data.ts = ts;
            return true;
        }
        false
    }

    pub fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        let (base, _, _) = self.get_base_pc_offset(request);

        let mut entries = self.entries.lock();

        if let Some(data) = entries.remove(&base) {
            return Some(AccTableEntry {
                tag: base,
                pc: data.pc,
                offset: data.offset,
                access_pattern: data.access_pattern,
                read_pattern: data.read_pattern,
                ts: data.ts,
                valid: true,
            });
        }
        None
    }
}