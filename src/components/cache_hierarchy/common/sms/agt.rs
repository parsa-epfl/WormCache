use super::acc::{AccTable, AccTableEntry};
use super::filter::FilterTable;
use crate::components::cache_hierarchy::CacheBlockRequest;

#[derive(Debug)]
struct AGTPerCore<const N_ACC: usize, const N_FILTER: usize, const N_BLK: usize> {
    acc_table: AccTable<N_ACC, N_BLK>,
    filter_table: FilterTable<N_FILTER, N_BLK>,
}

impl<const N_ACC: usize, const N_FILTER: usize, const N_BLK: usize>
    AGTPerCore<N_ACC, N_FILTER, N_BLK>
{
    fn new() -> Self {
        Self {
            acc_table: AccTable::new(),
            filter_table: FilterTable::new(),
        }
    }

    fn record(&self, request: &CacheBlockRequest, ts: u64) -> Option<AccTableEntry<N_BLK>> {
        if self.acc_table.poke_and_update(&request, ts) {
            None
        } else {
            match self.filter_table.poke_and_update(&request, ts) {
                Some(entry) => self.acc_table.insert(&entry),
                None => None,
            }
        }
    }

    fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        self.filter_table.evict(request);
        self.acc_table.evict(request)
    }
}

pub struct AGT<
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const N_BLK: usize,
> {
    tables: Box<[AGTPerCore<N_ACC, N_FILTER, N_BLK>; CORE_COUNT]>,
}

impl<const CORE_COUNT: usize, const N_ACC: usize, const N_FILTER: usize, const N_BLK: usize>
    AGT<CORE_COUNT, N_ACC, N_FILTER, N_BLK>
{
    pub fn new() -> Self {
        Self {
            tables: crate::util::init_heap_array(|_| AGTPerCore::<N_ACC, N_FILTER, N_BLK>::new()),
        }
    }

    pub fn record(&self, request: &CacheBlockRequest, ts: u64) -> Option<AccTableEntry<N_BLK>> {
        let core_id = request.core_id as usize;
        self.tables[core_id].record(request, ts)
    }

    pub fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        let core_id = request.core_id as usize;
        self.tables[core_id].evict(request)
    }
}

pub type ParallelAGT<
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const N_BLK: usize,
> = AGT<CORE_COUNT, N_ACC, N_FILTER, N_BLK>;
