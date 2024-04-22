use crate::components::{cache_hierarchy::get_memory_ts, NoMMU};

use self::{private_cache::HarvardPrivateCaches, shared_cache::LockedSharedCache};

use super::*;

type MH = LockedMemoryHierarchy<
    NoMMU,
    HarvardPrivateCaches<
        { parameter::CORE_COUNT },
        { parameter::HARVARD_PRI_I_CACHE_SET },
        { parameter::HARVARD_PRI_I_CACHE_ASSO },
        { parameter::HARVARD_PRI_D_CACHE_SET },
        { parameter::HARVARD_PRI_D_CACHE_ASSO },
    >,
    LockedSharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
>;

#[test]
fn i_create_sharer_from_clean_d() {
    let mh = MH::new();

    let block_id = 203;

    // First, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, true),
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
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a write request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, true, false),
        CacheHierarchyAccessResult::MissDueToPermission
    );

    // Third, generate a read request to the core 0 instruction cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, true),
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
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, true),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false),
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
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, true),
        CacheHierarchyAccessResult::Miss
    );

    // Second, generate a write request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, true, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Third, generate a read request to the core 0 data cache.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, get_memory_ts() as u64, false, false),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Now, we get the share information.
    let replicas = mh.private_caches.query_replica_state(block_id);
    assert_eq!(replicas.len(), 1);
    assert_eq!(replicas[&0], true);
}
