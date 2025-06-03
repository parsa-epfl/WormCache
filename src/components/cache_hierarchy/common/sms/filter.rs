use crate::components::cache_hierarchy::CacheBlockRequest;
use super::super::CCell;
use super::acc::AccTableEntry;

#[derive(Debug)]
pub struct FilterTableEntry {
    pub tag: u64,
    pub pc: u64,
    pub offset: u64,
    pub ts: u64,
}

impl FilterTableEntry {
    pub fn new() -> Self {
        Self {
            tag: 0,
            pc: 0,
            offset: 0,
            ts: 0,
        }
    }

    pub fn update(&mut self, ts: u64) {
        self.ts = ts;
    }

    pub fn replace(&mut self, new_entry: &FilterTableEntry) {
        self.tag = new_entry.tag;
        self.pc = new_entry.pc;
        self.offset = new_entry.offset;
        self.ts = new_entry.ts;
    }

    pub fn reset(&mut self) {
        self.tag = 0;
        self.pc = 0;
        self.offset = 0;
        self.ts = 0;
    }
}

#[derive(Debug)]
pub struct FilterTable<
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const N_FILTER: usize,
    const N_BLK: usize,
> {
    pub entries: Box<[GFilter; N_FILTER]>,
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

    fn poke(&self, request: &CacheBlockRequest) -> Option<usize> {
        let tag = request.block_id >> (N_BLK.trailing_zeros());
        self.entries.iter().position(|x| x.inner().tag == tag)
    }

    fn insert(&self, entry: FilterTableEntry) {
        match self.entries.iter().enumerate().min_by_key(|&(_, e)| e.inner().ts) {
            Some((index, _)) => {
                self.entries[index].inner().replace(&entry);
            }
            None => unreachable!(),
        }
    }

    pub fn poke_and_update(&self, request: &CacheBlockRequest, ts: u64) -> Option<AccTableEntry<N_BLK>> {
        let tag = request.block_id >> (N_BLK.trailing_zeros());
        let pc = request.pc;
        let offset = request.block_id % N_BLK as u64;
        match self.poke(request) {
            Some(index) => {
                let mut unlocked_entry = self.entries[index].inner();
                if offset == unlocked_entry.offset {
                    unlocked_entry.update(ts);
                    return None;
                } else {
                    let mut entry = AccTableEntry::<N_BLK>::new();
                    entry.tag = tag;
                    entry.pc = unlocked_entry.pc;
                    entry.offset = unlocked_entry.offset;
                    entry.pattern[offset as usize] = true;
                    entry.pattern[unlocked_entry.offset as usize] = true;
                    entry.ts = ts;
                    return Some(entry);
                }
            }
            None => {
                let mut entry = FilterTableEntry::new();
                entry.tag = tag;
                entry.pc = pc;
                entry.offset = offset;
                entry.ts = ts;
                self.insert(entry);
                return None;
            }
        }
    }

    pub fn evict(&self, request: &CacheBlockRequest) {
        let tag = request.block_id >> (N_BLK.trailing_zeros());
        match self.entries.iter().position(|e| e.inner().tag == tag) {
            Some(index) => {
                self.entries[index].inner().reset();
            }
            None => {},
        }
    }
}