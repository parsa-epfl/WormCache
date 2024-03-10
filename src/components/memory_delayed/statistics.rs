#[repr(align(64))]
pub struct PerCoreStatistics {
    pub total_mem: u64,
    pub private_cache_miss: u64,
    pub shared_cache_access: u64,
    pub tlb_access: u64,
    pub tlb_miss: u64,
}

impl PerCoreStatistics {
    pub fn new() -> Self {
        return Self {
            total_mem: 0,
            private_cache_miss: 0,
            shared_cache_access: 0,
            tlb_access: 0,
            tlb_miss: 0,
        };
    }

    pub fn being_printed(&self, core_id: u32) -> String {
        // core_id, total_mem, private_cache_miss, shared_cache_access, private_cache_miss_ratio, shared_cache_access_ratio,tlb_access,tlb_miss,tlb_miss_ratio
        return format!(
            "{},{},{},{},{:.2}%,{:.2}%,{},{},{:.2}%",
            core_id,
            self.total_mem,
            self.private_cache_miss,
            self.shared_cache_access,
            (self.private_cache_miss as f64 / self.total_mem as f64) * 100.0,
            (self.shared_cache_access as f64 / self.total_mem as f64) * 100.0,
            self.tlb_access,
            self.tlb_miss,
            (self.tlb_miss as f64 / self.tlb_access as f64) * 100.0,
        );
    }
}

