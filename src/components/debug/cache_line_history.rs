use dashmap::mapref::one::Ref;
use dashmap::DashMap;
use once_cell::sync::Lazy;

#[derive(Debug)]
pub enum CacheOperationType {
    GetM,
    GetR,
    Drop,
}

#[derive(Debug)]
pub struct SingleCacheLineCoherenceHistory {
    history: Vec<(CacheOperationType, usize, u64, bool)>, // operation, cache_id, timestamp
}

impl SingleCacheLineCoherenceHistory {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
        }
    }

    pub fn record(
        &mut self,
        operation: CacheOperationType,
        cache_id: usize,
        timestamp: u64,
        refilled: bool,
    ) {
        self.history
            .push((operation, cache_id, timestamp, refilled));
    }

    pub fn print_history(&self) {
        for (operation, core_id, timestamp, refilled) in &self.history {
            println!(
                "Operation: {:?}, Cache ID: {}, Timestamp: {}, Refilled: {}",
                operation, core_id, timestamp, refilled
            );
        }
    }

    pub fn print_last_n_history(&self, n: usize) {
        // for (operation, core_id, timestamp) in self.history.iter().rev().take(n) {
        //     println!(
        //         "Operation: {:?}, Core ID: {}, Timestamp: {}",
        //         operation, core_id, timestamp
        //     );
        // }

        if self.history.len() < n {
            self.print_history();
        } else {
            let middle = self.history.len() - n;

            for i in middle..self.history.len() {
                let (operation, core_id, timestamp, refilled) = &self.history[i];
                println!(
                    "Operation: {:?}, Cache ID: {}, Timestamp: {}, Refilled: {}",
                    operation, core_id, timestamp, refilled
                );
            }
        }
    }
}

pub struct CacheLineCoherenceHistory {
    history: DashMap<u64, SingleCacheLineCoherenceHistory>,
}

static mut GLOBAL_HISTORY: Lazy<CacheLineCoherenceHistory> =
    Lazy::new(CacheLineCoherenceHistory::new);

impl CacheLineCoherenceHistory {
    pub fn new() -> Self {
        Self {
            history: DashMap::new(),
        }
    }

    fn record(
        &self,
        block_id: u64,
        operation: CacheOperationType,
        cache_id: usize,
        timestamp: u64,
        refilled: bool,
    ) {
        let mut history = self
            .history
            .entry(block_id)
            .or_insert(SingleCacheLineCoherenceHistory::new());
        history.record(operation, cache_id, timestamp, refilled);
    }

    pub fn global_record_history(
        block_id: u64,
        operation: CacheOperationType,
        cache_id: usize,
        timestamp: u64,
        refilled: bool,
    ) {
        if !crate::parameter::ENABLE_CACHE_LINE_HISTORY {
            // I believe the compiler will optimize this function out.
            return;
        }
        unsafe {
            GLOBAL_HISTORY.record(block_id, operation, cache_id, timestamp, refilled);
        }
    }

    pub fn global_get_block_history(
        block_id: u64,
    ) -> Option<Ref<'static, u64, SingleCacheLineCoherenceHistory>> {
        unsafe { GLOBAL_HISTORY.history.get(&block_id).map(|v| v) }
    }
}
