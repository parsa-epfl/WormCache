use serde::Serialize;

// There are two possible operations for an exclusive shared cache
// 1. Empty to the cache, which means a write lock is required.
// 2. Read from the cache, depending on the result:
//    - Read is a hit: Read lock + write lock
//    - Read is a miss: Read lock
// 3. It will be probably OK to use Mutex.

pub trait SharedCache {
    fn new() -> Self;
    fn invalidate(&self, core_id: u32, block_id: u64, ts: u64) -> Option<bool>; // (is_modified)
    fn lookup(&self, core_id: u32, block_id: u64, ts: u64) -> Option<bool>; // (is_modified)
    fn insert(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    );

    fn lookup_and_insert(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_store: bool,
        increase_touched_count: bool,
    ) -> Option<bool>; // the lookup result: (is_modified)

    fn warmed_sets_count(&self) -> usize;

    fn warmed_slots_count(&self) -> usize;

    fn dump_snapshot(&self, snapshot_name: &str);

    fn information() -> String;
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Serialize)]
pub struct SerializedSharedCacheBlock {
    pub tag: u64,
    pub dirty: bool,
    pub writable: bool,
}

mod set_and_line;

pub use set_and_line::SharedCacheBlock;
pub use set_and_line::SharedCacheSet;

mod replicated;
mod single;

pub use replicated::ReplicatedSharedCache;
pub use single::ParallelSingleSharedCache;
pub use single::SerialSingleSharedCache;
pub use single::SingleSharedCache;

#[cfg(test)]
mod warm_counter_test;
