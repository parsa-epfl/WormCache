// This file defines the tests for the memory_delayed module.
// All these tests are taken from the input that triggers a bug.

use crate::components::NoMMU;
use crate::util::get_monotonic_ts;

use self::private_cache::ParallelUnifiedPrivateCache;
use super::*;
use crate::components::cache_hierarchy::shared_cache::ParallelSingleSharedCache;

const PCACHE_SET: usize = 1024;

type MH = MemoryHierarchy<
    NoMMU,
    ParallelUnifiedPrivateCache<32, { PCACHE_SET }, { parameter::UNIFIED_PRI_CACHE_ASSO }>,
    ParallelSingleSharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    true,
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
    { parameter::DIRECTORY_SHARD_COUNT },
>;

#[test]
fn read_evict_and_other_core_read_back() {
    let mh = MH::new();
    let block_id = 1043;

    // core 0 reads a data at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), CacheAccessType::DataRead),
        CacheHierarchyAccessResult::Miss
    );

    // Now, evict the block from the cache.
    for l in 0..parameter::UNIFIED_PRI_CACHE_ASSO {
        let block_id: u64 = ((l + 1) * PCACHE_SET) as u64 + block_id;
        assert_eq!(
            mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), CacheAccessType::DataRead),
            CacheHierarchyAccessResult::Miss
        );
    }

    // Then core 1 reads the cache line. It should hit in the shared cache.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, get_monotonic_ts(), CacheAccessType::DataRead),
        CacheHierarchyAccessResult::HitInSharedCache
    );
}

#[test]
fn one_core_write_first_then_read() {
    let mh = MH::new();
    let block_id = 1043;

    // core 0 reads a data at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::Miss
    );

    // Then, core 0 writes the data at timestamp 20.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), CacheAccessType::DataRead),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );
}

#[test]
fn write_write_read_then_old_write() {
    // This bug is related to the coherence state reconstruction.
    let mh = MH::new();
    let block_id = 1043;

    // First, there should be a write permission, by core 0, at timestamp 100.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 100, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::Miss
    );

    // Second, core 0 writes the same data at timestamp 150. This won't update the write timestamp in the directory.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 150, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Second, core 1 reads the data at timestamp 200.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 200, CacheAccessType::DataRead),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Third, core 2 writes the data at timestamp 125.
    assert_eq!(
        mh.access_memory_pblock_id(2, block_id, 125, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );
}

#[test]
fn read_then_write() {
    let mh = MH::new();
    let block_id = 1043;

    // First, there should be a write permission, by core 0, at timestamp 100.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 100, CacheAccessType::DataRead),
        CacheHierarchyAccessResult::Miss
    );

    // Second, core 0 writes the same data at timestamp 150. This won't update the write timestamp in the directory.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 150, CacheAccessType::DataWrite),
        if parameter::ENABLE_EXCLUSIVE_CACHE_STATE {
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        } else {
            CacheHierarchyAccessResult::MissDueToPermission
        }
    );
}
