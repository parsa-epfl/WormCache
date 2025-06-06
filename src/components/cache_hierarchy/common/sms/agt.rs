use spin::mutex::SpinMutex;
use crate::components::cache_hierarchy::CacheBlockRequest;
use super::super::CCell;
use super::acc::{AccTable, AccTableEntry};
use super::filter::{FilterTable, FilterTableEntry};

#[derive(Debug)]
pub struct AGTPerCore<
    GAcc: CCell<AccTableEntry<N_BLK>> + std::fmt::Debug,
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const N_ACC: usize,
    const N_FILTER: usize,
    const N_BLK: usize,
> {
    pub acc_table: AccTable<GAcc, N_ACC, N_BLK>,
    pub filter_table: FilterTable<GFilter, N_FILTER, N_BLK>,
}

impl<
    GAcc: CCell<AccTableEntry<N_BLK>> + std::fmt::Debug,
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const N_ACC: usize,
    const N_FILTER: usize,
    const N_BLK: usize,
> AGTPerCore<GAcc, GFilter, N_ACC, N_FILTER, N_BLK>
{
    pub fn new() -> Self {
        Self {
            acc_table: AccTable::new(),
            filter_table: FilterTable::new(),
        }
    }

    pub fn record(&self, request: &CacheBlockRequest, ts: u64) -> Option<AccTableEntry<N_BLK>> {
        match self.acc_table.poke_and_update(&request, ts) {
            true => {
                println!("[AGT] Entry updated for request {}", request.block_id);
                return None
            },
            false => {
                match self.filter_table.poke_and_update(&request, ts){
                    Some(entry) => {
                        println!("[AGT] Entry upgraded to acc table for request {}", request.block_id);
                        return self.acc_table.insert(&entry)
                    },
                    None => {
                        println!("[AGT] Entry inserted into filter table for request {}", request.block_id);
                        return None
                    }
                }
            }
        }
    }

    pub fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        self.filter_table.evict(request);
        self.acc_table.evict(request)
    }
}

#[derive(Debug)]
pub struct AGT<
    GAcc: CCell<AccTableEntry<N_BLK>> + std::fmt::Debug,
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const N_BLK: usize,
> {
    pub tables: Box<[AGTPerCore<GAcc, GFilter, N_ACC, N_FILTER, N_BLK>; CORE_COUNT]>,
}

impl<
    GAcc: CCell<AccTableEntry<N_BLK>> + std::fmt::Debug,
    GFilter: CCell<FilterTableEntry> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const N_BLK: usize,
> AGT<GAcc, GFilter, CORE_COUNT, N_ACC, N_FILTER, N_BLK>
{
    pub fn new() -> Self {
        Self {
            tables: crate::util::init_heap_array(|_| AGTPerCore::<GAcc, GFilter, N_ACC, N_FILTER, N_BLK>::new()),
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
> = AGT<SpinMutex<AccTableEntry<N_BLK>>, SpinMutex<FilterTableEntry>, CORE_COUNT, N_ACC, N_FILTER, N_BLK>;