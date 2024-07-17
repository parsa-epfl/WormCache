// This file defines the tests for the memory_delayed module.
// All these tests are taken from the input that triggers a bug.

use crate::components::cache_hierarchy::shared_cache::statistics::ZeroSharedCacheSetStatistics;
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
        ZeroSharedCacheSetStatistics,
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
    let mh = MH::new(true, 0, false);
    let block_id = 1043;

    // core 0 reads a data at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Now, evict the block from the cache.
    for l in 0..parameter::UNIFIED_PRI_CACHE_ASSO {
        let block_id: u64 = ((l + 1) * PCACHE_SET) as u64 + block_id;
        assert_eq!(
            mh.access_memory_pblock_id_with_the_same_ts_and_vts(
                0,
                block_id,
                get_monotonic_ts(),
                CacheAccessType::DataRead
            ),
            CacheHierarchyAccessResult::Miss
        );
    }

    // Then core 1 reads the cache line. It should hit in the shared cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            1,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInSharedCache
    );
}

#[test]
fn one_core_write_first_then_read() {
    let mh = MH::new(true, 0, false);
    let block_id = 1043;

    // core 0 reads a data at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Then, core 0 writes the data at timestamp 20.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );
}

#[test]
fn write_write_read_then_old_write() {
    // This bug is related to the coherence state reconstruction.
    let mh = MH::new(true, 0, false);
    let block_id = 1043;

    // First, there should be a write permission, by core 0, at timestamp 100.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            100,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Second, core 0 writes the same data at timestamp 150. This won't update the write timestamp in the directory.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            150,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Second, core 1 reads the data at timestamp 200.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            1,
            block_id,
            200,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Third, core 2 writes the data at timestamp 125.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            2,
            block_id,
            125,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::Unknown
    );
}

#[test]
fn write_read_then_early_read() {
    let mh = MH::new(true, 0, false);
    let block_id = 1043;

    // First, there should be a write permission, by core 0, at timestamp 100.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            100,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Second, core 1 reads the cache line. This can update the access timestamp but does not touch the write timestamp.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            1,
            block_id,
            150,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Third, core 2 reads the cache line at timestamp 50.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            2,
            block_id,
            50,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::Unknown
    );
}

#[test]
fn read_then_write() {
    let mh = MH::new(true, 0, false);
    let block_id = 1043;

    // First, there should be a write permission, by core 0, at timestamp 100.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            100,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Second, core 0 writes the same data at timestamp 150. This won't update the write timestamp in the directory.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            150,
            CacheAccessType::DataWrite
        ),
        if parameter::ENABLE_EXCLUSIVE_CACHE_STATE {
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        } else {
            CacheHierarchyAccessResult::MissDueToPermission
        }
    );
}

#[test]
fn write_read_after_write() {
    let mh = MH::new(true, 0, false);
    let block_id = 1043;

    // First, there should be a write permission, by core 0, at timestamp 100.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            100,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Second, core 0 read the cache line. This can update the access timestamp but does not touch the write timestamp.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            150,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Third, core 1 writes the data at timestamp 200.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            1,
            block_id,
            150,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );
}

#[test]
fn later_read_after_write_cancel_sharers() {
    let mh = MH::new(true, 0, false);
    let block_id = 1043;

    // Core 0 write, at 10.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            10,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Core 0 write, at 20.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            20,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Core 1 read. at 30.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            1,
            block_id,
            30,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Core 2 read, at 15. We don't know the result.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            2,
            block_id,
            15,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::Unknown
    );

    // But, Core 1's replica should not be invalidated.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            1,
            block_id,
            35,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Core 0 and Core 1 have the cache line.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            40,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // And also, core 1's entry is recorded in the directory.
    // This means when there is an eviction of this cache line in core'1, it should not trigger any panic.
    for i in 0..(parameter::UNIFIED_PRI_CACHE_ASSO + 1) {
        assert_eq!(
            mh.access_memory_pblock_id_with_the_same_ts_and_vts(
                1,
                block_id + ((i + 1) * parameter::UNIFIED_PRI_CACHE_SET) as u64,
                (50 + i) as u64,
                CacheAccessType::DataRead
            ),
            CacheHierarchyAccessResult::Miss
        );
    }
}

#[test]
fn write_evict_read_write() {
    let mh = MH::new(true, 0, false);
    let block_id = 1043;
    // First, there is a write access from core 0, at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            10,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::Miss
    );

    // We evict the cache line from the cache.
    for i in 0..parameter::UNIFIED_PRI_CACHE_ASSO {
        let block_id = block_id + (i + 1) as u64 * PCACHE_SET as u64;
        assert_eq!(
            mh.access_memory_pblock_id_with_the_same_ts_and_vts(
                0,
                block_id,
                20 + i as u64 * 10,
                CacheAccessType::DataWrite
            ),
            CacheHierarchyAccessResult::Miss
        );
    }

    // OK, we read it back, by another core
    // This should not trigger any assertion failure.
    // But it creates an replica on chip. Now it should create a replica.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            1,
            block_id,
            1024,
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInSharedCache
    );

    // what if we have a write access before the first one, from core 1?
    // Will this trigger an assertion failure?
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            2,
            block_id,
            5,
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::Unknown
    );
}
