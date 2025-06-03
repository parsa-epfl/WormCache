use spin::mutex::SpinMutex;
use crate::components::cache_hierarchy::CacheBlockRequest;
use super::super::CCell;
use super::acc::AccTableEntry;

#[derive(Debug)]
pub struct PHTEntry<
    const N_BLK: usize,
> {
    pub pc: u64,
    pub offset: u64,
    pub pattern: [bool; N_BLK],
    pub ts: u64,
}

impl<
    const N_BLK: usize,
> PHTEntry<N_BLK>
{
    pub fn new() -> Self {
        Self {
            pc: 0,
            offset: 0,
            pattern: [false; N_BLK],
            ts: 0,
        }
    }

    pub fn update(&mut self, ts: u64) {
        self.ts = ts;
    }

    pub fn replace(&mut self, new_entry: &AccTableEntry<N_BLK>) {
        self.pc = new_entry.pc;
        self.offset = new_entry.offset;
        self.pattern = new_entry.pattern;
        self.ts = new_entry.ts;
    }

    pub fn reset(&mut self) {
        self.pc = 0;
        self.offset = 0;
        self.pattern.fill(false);
        self.ts = 0;
    }
}

#[derive(Debug)]
pub struct PHTPerCore<
    G: CCell<PHTEntry<N_BLK>> + std::fmt::Debug,
    const N_PHT: usize,
    const N_BLK: usize,
> {
    pub entries: Box<[G; N_PHT]>,
}

impl<
    G: CCell<PHTEntry<N_BLK>> + std::fmt::Debug,
    const N_PHT: usize,
    const N_BLK: usize,
> PHTPerCore<G, N_PHT, N_BLK>
{
    pub fn new() -> Self {
        Self {
            entries: crate::util::init_heap_array(|_| G::new(PHTEntry::new())),
        }
    }

    pub fn lookup(&self, request: &CacheBlockRequest) -> Option<Vec<u64>> {
        let base = request.block_id >> (N_BLK.trailing_zeros());
        let pc = request.pc;
        let offset = request.block_id % N_BLK as u64;
        match self.entries.iter().position(|x| x.inner().pc == pc && x.inner().offset == offset) {
            Some(index) => {
                let entry = &self.entries[index].inner();
                let pattern = entry.pattern;
                let mut result = Vec::new();
                for (i, &bit) in pattern.iter().enumerate() {
                    if bit {
                        result.push(base + (i as u64));
                    }
                }
                Some(result)
            }
            None => None,
        }
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>) {
        match self.entries.iter().enumerate().min_by_key(|&(_, e)| e.inner().ts) {
            Some((index, _)) => {
                self.entries[index].inner().replace(&entry);
            }
            None => unreachable!(),
        }
    }
}

pub struct PHT<
    G: CCell<PHTEntry<N_BLK>> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const N_PHT: usize,
    const N_BLK: usize,
> {
    pub tables: Box<[PHTPerCore<G, N_PHT, N_BLK>; CORE_COUNT]>,
}

impl<
    G: CCell<PHTEntry<N_BLK>> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const N_PHT: usize,
    const N_BLK: usize,
> PHT<G, CORE_COUNT, N_PHT, N_BLK>
{
    pub fn new() -> Self {
        Self {
            tables: crate::util::init_heap_array(|_| PHTPerCore::<G, N_PHT, N_BLK>::new()),
        }
    }

    pub fn lookup(&self, request: &CacheBlockRequest) -> Option<Vec<u64>> {
        let core_id = request.core_id as usize;
        self.tables[core_id].lookup(request)
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>, core_id: usize) {
        self.tables[core_id].insert(entry);
    }
}

pub type ParallelPHT<
    const CORE_COUNT: usize,
    const N_PHT: usize,
    const N_BLK: usize,
> = PHT<SpinMutex<PHTEntry<N_BLK>>, CORE_COUNT, N_PHT, N_BLK>;