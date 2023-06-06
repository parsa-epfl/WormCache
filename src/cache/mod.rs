pub mod single;
pub mod ts_cache;
pub use ts_cache::*;

pub enum CacheReturnResult {
    Miss,
    Hit,
    MissWithEviction(usize),
    MissWithWriteBack(usize),
}

pub const PRIVATE_CACHE_ASSOCIATIVITY: usize = 8;
pub const PRIVATE_CACHE_SET_NUMBER: usize = 4096;

pub const LLC_ASSOCIATIVITY: usize =  16;
pub const LLC_SET: usize = 1024 * 64; // 64MB