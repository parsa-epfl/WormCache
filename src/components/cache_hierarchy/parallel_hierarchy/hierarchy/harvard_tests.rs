use crate::components::cache_hierarchy::common::PrivateCaches;
use crate::components::NoMMU;
use crate::parameter;
use crate::util::get_monotonic_ts;

use super::super::super::common::{
    statistics::ZeroSharedCacheSetStatistics, ParallelHarvardPrivateCache,
    ParallelSingleSharedCache,
};
use super::MemoryHierarchy;
use super::{CacheAccessType, CacheHierarchyAccessResult};

const PCACHE_SET: usize = 64;

type MH = MemoryHierarchy<
    NoMMU,
    ParallelHarvardPrivateCache<
        32,
        { PCACHE_SET },
        { parameter::HARVARD_PRI_I_CACHE_ASSO },
        { PCACHE_SET },
        { parameter::HARVARD_PRI_D_CACHE_ASSO },
    >,
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
fn i_create_sharer_from_clean_d() {
    let mh = MH::new(true, 0, false);

    let block_id = 203;

    // First, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::InstructionFetch
        ),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert!(!replicas[&0]);
}

#[test]
fn i_create_sharer_from_dirty_d() {
    let mh = MH::new(true, 0, false);

    let block_id = 203;

    // First, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a write request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataWrite
        ),
        if parameter::ENABLE_EXCLUSIVE_CACHE_STATE {
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        } else {
            CacheHierarchyAccessResult::MissDueToPermission
        }
    );

    // Third, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::InstructionFetch
        ),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert!(!replicas[&0]);
}

#[test]
fn d_create_sharer_from_clean_i() {
    let mh = MH::new(true, 0, false);

    let block_id = 203;

    // First, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::InstructionFetch
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert!(!replicas[&0]);
}

#[test]
fn d_dirty_create_sharer_from_clean_i() {
    let mh = MH::new(true, 0, false);

    let block_id = 203;

    // First, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::InstructionFetch
        ),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a write request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataWrite
        ),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Third, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id_with_the_same_ts_and_vts(
            0,
            block_id,
            get_monotonic_ts(),
            CacheAccessType::DataRead
        ),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert!(replicas[&0]);
}
