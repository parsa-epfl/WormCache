use crate::{
    components::{
        cache_hierarchy::{
            hierarchy::CacheHierarchyAccessResult, private_cache::ParallelUnifiedPrivateCache,
            shared_cache::ParallelSingleSharedCache,
        },
        debug::statistics::{EventType, Statistics},
        NoMMU,
    },
    parameter,
};

use super::{CacheAccessType, MemoryHierarchy};

const PCACHE_SET: usize = 64;

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
fn read_after_read_has_no_impact() {
    let mh = MH::new();
    let block_id = 0x1234;
    let original_value =
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation);

    // first, core 0 reads, with ts = 10, v_ts = 2.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, 2, CacheAccessType::DataRead),
        CacheHierarchyAccessResult::Miss
    );

    // then, core 1 reads, with ts = 20, and v_ts = 1.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 20, 1, CacheAccessType::DataRead),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // there should be zero impact on the miss rate.
    assert_eq!(
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation)
            - original_value,
        0
    );
}

#[test]
fn read_after_write_has_impact() {
    let mh = MH::new();
    let block_id = 0x1234;

    let original_value =
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation);

    // first, core 0 writes, with ts = 10, v_ts = 2.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, 2, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::Miss
    );

    // To skip the write timestamp in the directory, we let core 0 write it again.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 15, 4, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // then, core 1 reads, with ts = 20, and v_ts = 1.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 20, 3, CacheAccessType::DataRead),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // there should be one impact on the miss rate.
    assert_eq!(
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation)
            - original_value,
        1
    );
}

#[test]
fn write_after_read_has_impact() {
    let mh = MH::new();
    let block_id = 0x1234;

    let original_value =
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation);

    // first, core 0 reads, with ts = 10, v_ts = 2.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, 2, CacheAccessType::DataRead),
        CacheHierarchyAccessResult::Miss
    );

    // then, core 1 writes, with ts = 20, and v_ts = 1.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 20, 1, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // there should be one impact on the miss rate.
    assert_eq!(
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation)
            - original_value,
        1
    );
}

#[test]
fn write_after_write_has_impact() {
    let mh = MH::new();
    let block_id = 0x1234;

    let original_value =
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation);

    // first, core 0 writes, with ts = 10, v_ts = 2.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, 2, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::Miss
    );

    // To skip the write timestamp in the directory, we let core 0 write it again.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 15, 4, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // then, core 1 writes, with ts = 20, and v_ts = 1.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 20, 3, CacheAccessType::DataWrite),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // there should be one impact on the miss rate.
    assert_eq!(
        Statistics::global_query_record(1, EventType::PrivateCacheVTsOrderViolation)
            - original_value,
        1
    );
}
