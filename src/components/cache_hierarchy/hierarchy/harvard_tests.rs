use crate::util::get_monotonic_ts;

use crate::components::NoMMU;

use self::{private_cache::ParallelHarvardPrivateCache, shared_cache::ParallelSingleSharedCache};

use super::*;

type MH = MemoryHierarchy<
    NoMMU,
    ParallelHarvardPrivateCache<
        32,
        { DIRECTORY_SET },
        { parameter::HARVARD_PRI_I_CACHE_ASSO },
        { DIRECTORY_SET },
        { parameter::HARVARD_PRI_D_CACHE_ASSO },
    >,
    ParallelSingleSharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    true,
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
>;

#[test]
fn i_create_sharer_from_clean_d() {
    let mh = MH::new();

    let block_id = 203;

    // First, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), false, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), false, true, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert_eq!(replicas[&0], false);
}

#[test]
fn i_create_sharer_from_dirty_d() {
    let mh = MH::new();

    let block_id = 203;

    // First, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), false, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a write request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), true, false, false),
        if parameter::ENABLE_EXCLUSIVE_CACHE_STATE {
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        } else {
            CacheHierarchyAccessResult::MissDueToPermission
        }
    );

    // Third, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), false, true, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert_eq!(replicas[&0], false);
}

#[test]
fn d_create_sharer_from_clean_i() {
    let mh = MH::new();

    let block_id = 203;

    // First, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), false, true, false),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), false, false, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert_eq!(replicas[&0], false);
}

#[test]
fn d_dirty_create_sharer_from_clean_i() {
    let mh = MH::new();

    let block_id = 203;

    // First, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), false, true, false),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a write request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), true, false, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Third, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_monotonic_ts(), false, false, false),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert_eq!(replicas[&0], true);
}
