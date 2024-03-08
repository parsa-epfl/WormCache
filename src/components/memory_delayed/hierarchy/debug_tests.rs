// This file defines the tests for the memory_delayed module.
// All these tests are taken from the input that triggers a bug.

use crate::components::memory_delayed::get_memory_ts;

use super::*;

type MH = TestingDelayedMemoryHierarchy;

#[test]
fn read_evict_and_other_core_read_back() {
    let mut mh = MH::new();
    let block_id = 1043;

    // core 0 reads a data at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Now, evict the block from the cache.
    for l in 0..parameter::PRI_CACHE_ASSO {
        let block_id: u64 = ((l + 1) * parameter::PRI_CACHE_SET) as u64 + block_id;
        assert_eq!(
            mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false),
            CacheHierarchyAccessResult::Miss
        );
    }

    // Then core 1 reads the cache line. It should hit in the shared cache.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, get_memory_ts() as u64, false, false),
        CacheHierarchyAccessResult::HitInSharedCache
    );
}

#[test]
fn one_core_write_first_then_read() {
    let mut mh = MH::new();
    let block_id = 1043;

    // core 0 reads a data at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, true, false),
        CacheHierarchyAccessResult::Miss
    );

    // Then, core 0 writes the data at timestamp 20.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );
}
