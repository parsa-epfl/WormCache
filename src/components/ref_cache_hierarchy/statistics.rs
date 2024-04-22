use std::sync::atomic::{AtomicU64, Ordering};

#[repr(align(64))]
pub struct PerCoreStatistics {
    pub total_mem: AtomicU64,
    pub private_cache_miss: AtomicU64,
    pub shared_cache_access: AtomicU64,
    pub tlb_access: AtomicU64,
    pub tlb_miss: AtomicU64,
}

impl PerCoreStatistics {
    pub fn new() -> Self {
        return Self {
            total_mem: AtomicU64::new(0),
            private_cache_miss: AtomicU64::new(0),
            shared_cache_access: AtomicU64::new(0),
            tlb_access: AtomicU64::new(0),
            tlb_miss: AtomicU64::new(0),
        };
    }

    pub fn being_printed(&self, core_id: u32) -> String {
        // core_id, total_mem, private_cache_miss, shared_cache_access, private_cache_miss_ratio, shared_cache_access_ratio,tlb_access,tlb_miss,tlb_miss_ratio
        return format!(
            "{},{},{},{},{:.2}%,{:.2}%,{},{},{:.2}%",
            core_id,
            self.total_mem.load(Ordering::Relaxed),
            self.private_cache_miss.load(Ordering::Relaxed),
            self.shared_cache_access.load(Ordering::Relaxed),
            (self.private_cache_miss.load(Ordering::Relaxed) as f64
                / self.total_mem.load(Ordering::Relaxed) as f64)
                * 100.0,
            (self.shared_cache_access.load(Ordering::Relaxed) as f64
                / self.total_mem.load(Ordering::Relaxed) as f64)
                * 100.0,
            self.tlb_access.load(Ordering::Relaxed),
            self.tlb_miss.load(Ordering::Relaxed),
            (self.tlb_miss.load(Ordering::Relaxed) as f64
                / self.tlb_access.load(Ordering::Relaxed) as f64)
                * 100.0,
        );
    }
}
