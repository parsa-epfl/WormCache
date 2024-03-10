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
    history: Vec<(CacheOperationType, u32, u64)>, // operation, core_id, timestamp
}

impl SingleCacheLineCoherenceHistory {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
        }
    }

    pub fn record(&mut self, operation: CacheOperationType, core_id: u32, timestamp: u64) {
        self.history.push((operation, core_id, timestamp));
    }

    pub fn print_history(&self) {
        for (operation, core_id, timestamp) in &self.history {
            println!(
                "Operation: {:?}, Core ID: {}, Timestamp: {}",
                operation, core_id, timestamp
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
                let (operation, core_id, timestamp) = &self.history[i];
                println!(
                    "Operation: {:?}, Core ID: {}, Timestamp: {}",
                    operation, core_id, timestamp
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

    fn record(&self, block_id: u64, operation: CacheOperationType, core_id: u32, timestamp: u64) {
        let mut history = self
            .history
            .entry(block_id)
            .or_insert(SingleCacheLineCoherenceHistory::new());
        history.record(operation, core_id, timestamp);
    }

    pub fn global_record_history(
        block_id: u64,
        operation: CacheOperationType,
        core_id: u32,
        timestamp: u64,
    ) {
        if !crate::parameter::ENABLE_CACHE_LINE_HISTORY {
            // I believe the compiler will optimize this function out.
            return;
        }
        unsafe {
            GLOBAL_HISTORY.record(block_id, operation, core_id, timestamp);
        }
    }

    pub fn global_get_block_history(
        block_id: u64,
    ) -> Option<Ref<'static, u64, SingleCacheLineCoherenceHistory>> {
        unsafe { GLOBAL_HISTORY.history.get(&block_id).map(|v| v) }
    }
}
