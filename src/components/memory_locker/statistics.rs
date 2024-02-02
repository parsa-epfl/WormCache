#[repr(align(64))]
pub struct PerCoreStatistics {
    pub total_instruction: u64,
    pub private_cache_miss: u64,
    pub shared_cache_access: u64
}

impl PerCoreStatistics {
    pub fn new() -> Self {
        return Self {
            total_instruction: 0,
            private_cache_miss: 0,
            shared_cache_access: 0
        };
    }

    pub fn being_printed(&self, core_id: u32) -> String {
        return format!(
            "Core {}: Total Memory Instruction: {}, Private Cache Miss: {}, Shared Cache Access: {}, Private Miss Rate: {:.2}%, Shared Access Rate: {:.2}%",
            core_id,
            self.total_instruction,
            self.private_cache_miss,
            self.shared_cache_access,
            (self.private_cache_miss as f64 / self.total_instruction as f64) * 100.0,
            (self.shared_cache_access as f64 / self.total_instruction as f64) * 100.0
        );
    }
}