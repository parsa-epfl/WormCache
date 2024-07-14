use serde::Serialize;

// There are two possible operations for an exclusive shared cache
// 1. Empty to the cache, which means a write lock is required.
// 2. Read from the cache, depending on the result:
//    - Read is a hit: Read lock + write lock
//    - Read is a miss: Read lock
// 3. It will be probably OK to use Mutex.

#[derive(Debug, PartialEq, Eq)]
pub enum SharedCacheLookupResult {
    Hit(bool), // (is_dirty)
    Miss,
    Unknown(u32), // timestamp difference
}

#[derive(Debug, PartialEq, Eq)]
pub enum SharedCacheLookupAndInsertResult {
    Hit(bool),      // (is_dirty)
    Inserted(bool), // (just_warmed)
    Unknown(u32),   // timestamp difference
}

pub enum VtsViolationResult {
    Violataed(u32), // timestamp difference
    NotViolated,
}

pub trait SharedCache {
    fn new() -> Self;
    fn invalidate(&self, core_id: u32, block_id: u64, ts: u64, v_ts: u64) -> Option<bool>; // (is_modified)

    // abandon_dirty is here to create a replica to the private cache.
    fn lookup(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        abandon_dirty: bool,
        access_type: CacheAccessType,
        is_os: bool,
    ) -> (SharedCacheLookupResult, VtsViolationResult); // (is_modified)

    fn insert(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    );

    fn lookup_and_insert_on_miss(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        abandon_dirty: bool,
        is_store: bool,
        increase_touched_count: bool,
        access_type: CacheAccessType,
        is_os: bool,
    ) -> (SharedCacheLookupResult, VtsViolationResult); // the lookup result: (is_modified)

    fn warmed_sets_count(&self) -> usize;

    fn warmed_slots_count(&self) -> usize;

    fn dump_snapshot(&self, snapshot_name: &str);

    fn information() -> String;

    fn dump_access_frequency(&self, file_name: &str);

    fn serialize(&self, name: &str, numa_node_id: usize);
    fn deserialize(&mut self, name: &str, numa_node_id: usize); // this is in-place deserialization.
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Serialize)]
pub struct SerializedSharedCacheBlock {
    pub tag: u64,
    pub dirty: bool,
    pub writable: bool,
}

pub mod statistics;

mod set_and_line;

pub use set_and_line::SharedCacheBlock;
pub use set_and_line::SharedCacheSet;

mod replicated;
mod single;

pub use replicated::ReplicatedSharedCache;
pub use single::ParallelSingleSharedCache;
pub use single::SerialSingleSharedCache;
pub use single::SingleSharedCache;

use super::hierarchy::CacheAccessType;

#[cfg(test)]
mod warm_counter_test;
