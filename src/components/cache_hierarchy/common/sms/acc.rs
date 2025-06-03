use crate::components::cache_hierarchy::CacheBlockRequest;
use super::super::CCell;

#[derive(Debug, Clone)]
pub struct AccTableEntry<
    const N_BLK: usize,
> {
    pub tag: u64,
    pub pc: u64,
    pub offset: u64,
    pub pattern: [bool; N_BLK],
    pub ts: u64,
}

impl<
    const N_BLK: usize,
> AccTableEntry<N_BLK>
{
    pub fn new() -> Self {
        Self {
            tag: 0,
            pc: 0,
            offset: 0,
            pattern: [false; N_BLK],
            ts: 0,
        }
    }

    pub fn set_pattern_bit(&mut self, block_id: usize, ts: u64) {
        self.pattern[block_id] = true;
        self.ts = ts;
    }

    pub fn replace(&mut self, new_entry: &AccTableEntry<N_BLK>) {
        self.tag = new_entry.tag;
        self.pc = new_entry.pc;
        self.offset = new_entry.offset;
        self.pattern = new_entry.pattern;
        self.ts = new_entry.ts;
    }

    pub fn reset(&mut self) {
        self.tag = 0;
        self.pc = 0;
        self.offset = 0;
        self.pattern.fill(false);
        self.ts = 0;
    }
}

#[derive(Debug)]
pub struct AccTable<
    GAcc: CCell<AccTableEntry<N_BLK>> + std::fmt::Debug,
    const N_ACC: usize,
    const N_BLK: usize,
> {
    pub entries: Box<[GAcc; N_ACC]>,
}

impl<
    GAcc: CCell<AccTableEntry<N_BLK>> + std::fmt::Debug,
    const N_ACC: usize,
    const N_BLK: usize,
> AccTable<GAcc, N_ACC, N_BLK>
{
    pub fn new() -> Self {
        Self {
            entries: crate::util::init_heap_array(|_| GAcc::new(AccTableEntry::new())),
        }
    }

    fn poke(&self, request: &CacheBlockRequest) -> Option<usize> {
        let tag = request.block_id >> (N_BLK.trailing_zeros());
        self.entries.iter().position(|x| x.inner().tag == tag)
    }

    pub fn poke_and_update(&self, request: &CacheBlockRequest, ts: u64) -> bool {
        match self.poke(request) {
            Some(index) => {
                let block_id = request.block_id % N_BLK as u64;
                self.entries[index].inner().set_pattern_bit(block_id as usize, ts);
                return true;
            }
            None => return false,
        }
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>) -> Option<AccTableEntry<N_BLK>> {
        match self.entries.iter().enumerate().min_by_key(|&(_, e)| e.inner().ts) {
            Some((_, min_entry)) => {
                let mut unlocked_min_entry = min_entry.inner();
                let evicted_entry = match unlocked_min_entry.ts {
                    0 => None, // If the entry is empty, we can insert directly.
                    _ => Some(unlocked_min_entry.clone()), // Otherwise, we evict the entry.
                };
                unlocked_min_entry.replace(entry);
                return evicted_entry;
            }
            None => unreachable!(),
        }
    }

    pub fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        let tag = request.block_id >> (N_BLK.trailing_zeros());
        match self.entries.iter().position(|e| e.inner().tag == tag) {
            Some(index) => {
                let mut unlocked_entry = self.entries[index].inner();
                let evicted_entry = unlocked_entry.clone();
                unlocked_entry.reset();
                return Some(evicted_entry);
            }
            None => None,
        }
    }
}

// TODO: Write tests