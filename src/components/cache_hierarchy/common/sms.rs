use spin::mutex::SpinMutex;

use crate::components::cache_hierarchy::CacheBlockRequest;
use super::CCell;


#[derive(Debug)]
pub struct AccTableEntry {
    tag: usize,
    pc: usize,
    offset: usize,
    lru_ts: usize,
    pattern: usize,
    valid: bool,
}

#[derive(Debug)]
pub struct FilterTableEntry {
    tag: usize,
    pc: usize,
    offset: usize,
    lru_ts: usize,
    valid: bool,
}

#[derive(Debug)]
pub struct PHTEntry {
    tag: (usize, usize),    // (pc, offset)
    lru_ts: usize,
    pattern: usize,
    valid: bool,
}

#[derive(Debug)]
struct AGTPerCore<
    GAcc: CCell<AccTableEntry> + std::fmt::Debug,   // We dont need to use CCell?
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const N_ACC: usize,
    const N_FILTER: usize,
    const OFF_BITW: usize,
> {
    acc_entries: Box<[GAcc; N_ACC]>, // Why do we need box here?
    filter_entries: Box<[GFilter; N_FILTER]>,
}

#[derive(Debug)]
struct PHTPerCore<
    GPht: CCell<PHTEntry> + std::fmt::Debug,
    const N_PHT: usize,
> {
    pht_entries: Box<[GPht; N_PHT]>,
}

impl<
    GPht: CCell<PHTEntry> + std::fmt::Debug,
    const N_PHT: usize,
> PHTPerCore<GPht, N_PHT> {
    pub fn new() -> Self {
        Self {
            pht_entries: crate::util::init_heap_array(
                |_| GPht::new(PHTEntry {
                    tag: (0, 0),
                    lru_ts: 0,
                    pattern: 0,
                    valid: false,
                }),
            ),
        }
    }

    pub fn lookup(&self, pc: usize, offset: usize, ts: usize) -> Option<usize> {
        let tag = (pc, offset);
        for entry in self.pht_entries.iter() {
            let mut lock_entry = entry.inner();
            if lock_entry.tag == tag && lock_entry.valid {
                lock_entry.lru_ts = ts;
                return Some(lock_entry.pattern);
            }
            drop(lock_entry);
        }
        None
    }

    pub fn insert(&mut self, entry: &AccTableEntry) {
        let tag = (entry.pc, entry.offset);
        let mut lru_ts =  usize::MAX;
        let mut lru_idx = 0;
        for (idx, pht_entry) in self.pht_entries.iter_mut().enumerate() {
            let mut lock_entry = pht_entry.inner();
            if !lock_entry.valid {
                *lock_entry = PHTEntry {
                    tag,
                    lru_ts: entry.lru_ts,
                    pattern: entry.pattern,
                    valid: true,
                };
                return;
            } else if lock_entry.tag == tag {
                // Update existing entry
                lock_entry.lru_ts = entry.lru_ts;
                lock_entry.pattern = entry.pattern;
                return;
            } else {
                // Find the least recently used entry
                if lock_entry.lru_ts < lru_ts {
                    lru_ts = lock_entry.lru_ts;
                    lru_idx = idx;
                }
            }
            drop(lock_entry);
        }
        *self.pht_entries[lru_idx].inner() = PHTEntry {
            tag: tag,
            lru_ts: entry.lru_ts,
            pattern: entry.pattern,
            valid: true,
        };
    }

}

impl<
    GAcc: CCell<AccTableEntry> + std::fmt::Debug,
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const N_ACC: usize,
    const N_FILTER: usize,
    const OFF_BITW: usize,
> AGTPerCore<GAcc, GFilter, N_ACC, N_FILTER, OFF_BITW>
{
    pub fn new() -> Self {
        Self {
            acc_entries: crate::util::init_heap_array(
                |_| GAcc::new(AccTableEntry {
                    tag: 0,
                    pc: 0,
                    offset: 0,
                    lru_ts: 0,
                    pattern: 0,
                    valid: false,
                }),
            ),
            filter_entries: crate::util::init_heap_array(
                |_| GFilter::new(FilterTableEntry {
                    tag: 0,
                    pc: 0,
                    offset: 0,
                    lru_ts: 0,
                    valid: false,
                }),
            ),
        }
    }

    pub fn lookup(&mut self, addr: usize, pc: usize, ts: usize) -> Option<AccTableEntry> {
        let tag = addr >> OFF_BITW;
        let offset = addr & ((1 << OFF_BITW) - 1);

        let acc_search = self.lookup_acc(tag, offset, ts);
        if acc_search == true {
            return None;
        }
        match self.lookup_filter(tag, pc, offset, ts) {
            Some(entry) => self.insert_acc(entry),
            None => None,
        }
    }

    pub fn evict(&mut self, addr: usize) -> Option<AccTableEntry> {
        let tag = addr >> OFF_BITW;

        match self.evict_acc(tag) {
            Some(entry) => Some(entry),
            None => {
                match self.evict_filter(tag) {
                    true => None,
                    false => None,  // TODO: check, should this happen?
                }
            }
        }
    }

    fn evict_acc(&mut self, tag: usize) -> Option<AccTableEntry> {
        for entry in self.acc_entries.iter_mut() {
            let mut lock_entry = entry.inner();
            if lock_entry.tag == tag && lock_entry.valid {
                let evicted_entry = AccTableEntry {
                    tag: lock_entry.tag,
                    pc: lock_entry.pc,
                    offset: lock_entry.offset,
                    lru_ts: lock_entry.lru_ts,
                    pattern: lock_entry.pattern,
                    valid: true,
                };
                *lock_entry = AccTableEntry {
                    tag: 0,
                    pc: 0,
                    offset: 0,
                    lru_ts: 0,
                    pattern: 0,
                    valid: false,
                };
                return Some(evicted_entry);
            }
            drop(lock_entry);
        }
        None
    }

    fn evict_filter(&mut self, tag: usize) -> bool {
        for entry in self.filter_entries.iter_mut() {
            let mut lock_entry = entry.inner();
            if lock_entry.tag == tag && lock_entry.valid {
                *lock_entry = FilterTableEntry {
                    tag: 0,
                    pc: 0,
                    offset: 0,
                    lru_ts: 0,
                    valid: false,
                };
                return true;
            }
            drop(lock_entry);
        }
        false
    }

    fn lookup_acc(&mut self, tag: usize, offset: usize, ts: usize) -> bool {
        for entry in self.acc_entries.iter_mut() {
            let mut lock_entry = entry.inner();
            if lock_entry.tag == tag && lock_entry.valid {
                lock_entry.pattern |= 1 << offset;
                lock_entry.lru_ts = ts;
                return true;
            }
            drop(lock_entry);
        }
        false
    }

    fn insert_acc(&mut self, entry: AccTableEntry) -> Option<AccTableEntry> {
        let mut lru_ts = usize::MAX;
        let mut lru_idx = 0;
        for (idx, acc_entry) in self.acc_entries.iter().enumerate() {
            let mut lock_entry = acc_entry.inner();
            if lock_entry.valid == false {
                *lock_entry = entry;
                return None;
            }
            if lock_entry.lru_ts < lru_ts {
                lru_ts = lock_entry.lru_ts;
                lru_idx = idx;
            }
        }
        let mut lock_entry = self.acc_entries[lru_idx].inner();
        let evicted_entry = 
            AccTableEntry {
                tag: lock_entry.tag,
                pc: lock_entry.pc,
                offset: lock_entry.offset,
                lru_ts: lock_entry.lru_ts,
                pattern: lock_entry.pattern,
                valid: true,
            };
        *lock_entry = entry;
        return Some(evicted_entry);
    }

    fn lookup_filter(&mut self, tag: usize, pc: usize, offset: usize, ts: usize) -> Option<AccTableEntry> {
        let mut lru_ts = usize::MAX;
        let mut lru_idx = 0;
        for (idx, entry) in self.filter_entries.iter_mut().enumerate() {
            let mut lock_entry = entry.inner();
            if lock_entry.valid == false {
                *lock_entry = FilterTableEntry {
                    tag: tag,
                    pc: pc,
                    offset: offset,
                    lru_ts: ts,
                    valid: true,
                };
                return None;
            }
            if lock_entry.tag == tag {
                if lock_entry.offset == offset {
                    lock_entry.lru_ts = ts;
                    return None;
                } else {
                    let pattern: usize = (1 << lock_entry.offset) | (1 << offset);
                    return Some(AccTableEntry {
                        tag: lock_entry.tag,
                        pc: lock_entry.pc,
                        offset: lock_entry.offset,
                        lru_ts: ts,
                        pattern: pattern,
                        valid: true,
                    });
                }
            } else {
                if lock_entry.lru_ts < lru_ts{
                    lru_ts = lock_entry.lru_ts;
                    lru_idx = idx;
                }
            }
        }
        *self.filter_entries[lru_idx].inner() = 
            FilterTableEntry {
                tag: tag,
                pc: pc,
                offset: offset,
                lru_ts: ts,
                valid: true,
            };
        None
    }
}

pub trait AGTTrait {
    fn new() -> Self;
    fn lookup(&mut self, request: &CacheBlockRequest, ts: usize) -> Option<AccTableEntry>;
    fn evict(&mut self, request: &CacheBlockRequest) -> Option<AccTableEntry>;
}

pub trait PHTTrait {
    fn new() -> Self;
    fn lookup(&self, request: &CacheBlockRequest, ts: usize) -> Option<Vec<usize>>;
    fn insert(&mut self, entry: &AccTableEntry, core_id: usize);
}

pub struct AGT<
    GAcc: CCell<AccTableEntry> + std::fmt::Debug,   // We dont need to use CCell?
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const OFF_BITW: usize,
> {
    tables: Box<[AGTPerCore<GAcc, GFilter, N_ACC, N_FILTER, OFF_BITW>; CORE_COUNT]>,
}

pub struct PHT<
    GPht: CCell<PHTEntry> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const N_PHT: usize,
    const OFF_BITW: usize,
> {
    tables: Box<[PHTPerCore<GPht, N_PHT>; CORE_COUNT]>,
}

impl<
    GPht: CCell<PHTEntry> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const N_PHT: usize,
    const OFF_BITW: usize,
> PHTTrait for PHT<GPht, CORE_COUNT, N_PHT, OFF_BITW>
{
    fn new() -> Self {
        Self {
            tables: crate::util::init_heap_array(|_| PHTPerCore::<GPht, N_PHT>::new()),
        }
    }

    fn lookup(&self, request: &CacheBlockRequest, ts: usize) -> Option<Vec<usize>> {
        let core_id = request.core_id as usize;
        assert!(core_id < CORE_COUNT, "Core ID out of bounds: {}", core_id);
        let pc = request.pc as usize;
        let adddr = request.block_id as usize;
        let tag = adddr >> OFF_BITW;
        let offset = adddr & ((1 << OFF_BITW) - 1);
        let pattern = match self.tables[core_id].lookup(pc, offset, ts) {
            Some(pattern) => pattern,
            None => return None,
        };
        let max = 1 << OFF_BITW - 1;
        let mut result = Vec::new();
        for bit in 0..max {
            if (pattern & (1 << bit)) != 0 {
                let addr = tag << OFF_BITW + bit;
                result.push(addr);
            }
        }
        Some(result)
    }

    fn insert(&mut self, entry: &AccTableEntry, core_id: usize) {
        self.tables[core_id].insert(entry);
    }
}

impl<
    GAcc: CCell<AccTableEntry> + std::fmt::Debug,
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const OFF_BITW: usize,
> AGTTrait for AGT<GAcc, GFilter, CORE_COUNT, N_ACC, N_FILTER, OFF_BITW>
{

    fn new() -> Self {
        Self {
            tables: crate::util::init_heap_array(|_| AGTPerCore::<GAcc, GFilter, N_ACC, N_FILTER, OFF_BITW>::new()),
        }
    }

    fn lookup(&mut self, request: &CacheBlockRequest, ts: usize) -> Option<AccTableEntry> {
        let core_id = request.core_id as usize;
        assert!(core_id < CORE_COUNT, "Core ID out of bounds: {}", core_id);
        let addr = request.block_id as usize;
        let pc = request.pc as usize;

        self.tables[core_id].lookup(addr, pc, ts)
    }

    fn evict(&mut self, request: &CacheBlockRequest) -> Option<AccTableEntry> {
        let core_id = request.core_id as usize;
        assert!(core_id < CORE_COUNT, "Core ID out of bounds: {}", core_id);
        let addr = request.block_id as usize;

        self.tables[core_id].evict(addr)
    }
}

pub type ParallelAGT<
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const OFF_BITW: usize,
> = AGT<SpinMutex<AccTableEntry>, SpinMutex<FilterTableEntry>, CORE_COUNT, N_ACC, N_FILTER, OFF_BITW>;

pub type ParallelPHT<
    const CORE_COUNT: usize,
    const N_PHT: usize,
    const OFF_BITW: usize,
> = PHT<SpinMutex<PHTEntry>, CORE_COUNT, N_PHT, OFF_BITW>;