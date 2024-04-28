// This file defines the tests for the memory_delayed module.
// All these tests are taken from the input that triggers a bug.

use crate::components::{cache_hierarchy::get_memory_ts, NoMMU};

use self::{
    private_cache::{ParallelUnifiedPrivateCache, UnifiedPrivateCaches},
    shared_cache::ParallelSingleSharedCache,
};
use super::*;

type MH = MemoryHierarchy<
    NoMMU,
    ParallelUnifiedPrivateCache<
        { parameter::CORE_COUNT },
        { parameter::UNIFIED_PRI_CACHE_SET },
        { parameter::UNIFIED_PRI_CACHE_ASSO },
    >,
    ParallelSingleSharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
>;

#[test]
fn read_evict_and_other_core_read_back() {
    let mut mh = MH::new();
    let block_id = 1043;

    // core 0 reads a data at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Now, evict the block from the cache.
    for l in 0..parameter::UNIFIED_PRI_CACHE_ASSO {
        let block_id: u64 = ((l + 1) * parameter::UNIFIED_PRI_CACHE_SET) as u64 + block_id;
        assert_eq!(
            mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false, false),
            CacheHierarchyAccessResult::Miss
        );
    }

    // Then core 1 reads the cache line. It should hit in the shared cache.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, get_memory_ts() as u64, false, false, false),
        CacheHierarchyAccessResult::HitInSharedCache
    );
}

#[test]
fn one_core_write_first_then_read() {
    let mut mh = MH::new();
    let block_id = 1043;

    // core 0 reads a data at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, true, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Then, core 0 writes the data at timestamp 20.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false, false),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );
}

#[test]
fn write_write_read_then_old_write() {
    // This bug is related to the coherence state reconstruction.
    if DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION {
        println!("This test is disabled because of the DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION flag.");
        return;
    }

    let mut mh = MH::new();
    let block_id = 1043;

    // First, there should be a write permission, by core 0, at timestamp 100.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 100, true, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Second, core 0 writes the same data at timestamp 150. This won't update the write timestamp in the directory.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 150, true, false, false),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Second, core 1 reads the data at timestamp 200.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 200, false, false, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Third, core 2 writes the data at timestamp 125. This should trigger an assertion failure.
    assert_eq!(
        mh.access_memory_pblock_id(2, block_id, 125, true, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );
}

#[test]
fn read_then_write() {
    let mut mh = MH::new();
    let block_id = 1043;

    // First, there should be a write permission, by core 0, at timestamp 100.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 100, false, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Second, core 0 writes the same data at timestamp 150. This won't update the write timestamp in the directory.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 150, true, false, false),
        CacheHierarchyAccessResult::MissDueToPermission
    );
}
