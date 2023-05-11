pub mod single;
pub mod private_llc;

pub enum CacheReturnResult {
    Miss,
    Hit,
    MissWithEviction(usize),
    MissWithDirtyEviction(usize),
    MissWithWrongPermission // shared -> modified, exclusive.
}

pub const PRIVATE_CACHE_ASSOCIATIVITY: usize = 8;
pub const PRIVATE_CACHE_SET_NUMBER: usize = 4096;

pub const LLC_ASSOCIATIVITY: usize =  16;
pub const LLC_SET: usize = 1024 * 64; // 64MB