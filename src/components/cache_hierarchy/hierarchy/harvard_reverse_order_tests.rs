use std::collections::HashMap;

use crate::components::NoMMU;

use self::private_cache::ParallelHarvardPrivateCache;
use crate::components::cache_hierarchy::shared_cache::ParallelSingleSharedCache;

use super::*;

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
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    true,
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
>;

#[derive(Debug, PartialEq, Eq)]
enum BlockPosition {
    InPrivateCache(Vec<u32>),
    InSharedCache,
    NotInCache,
}

#[derive(Debug, PartialEq, Eq)]
enum BlockState {
    Shared,
    Modified,
}

impl MH {
    fn where_is_the_block(&mut self, block_id: u64) -> BlockPosition {
        let private_owner = self.private_caches.in_which_cores(block_id);

        if !private_owner.is_empty() {
            return BlockPosition::InPrivateCache(private_owner);
        }

        if self.shared_cache.lookup(0, block_id, 0).is_some() {
            return BlockPosition::InSharedCache;
        }
        BlockPosition::NotInCache
    }

    fn get_all_private_replicas(&mut self, block_id: u64) -> HashMap<u32, BlockState> {
        let result = self.private_caches.query_replica_state(block_id);
        result
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    if v {
                        BlockState::Modified
                    } else {
                        BlockState::Shared
                    },
                )
            })
            .collect()
    }
}

#[test]
#[should_panic(expected = "assertion failed: self.lines[idx_of_slot_to_fill].ts <= ts")]
fn reversed_timestamp_from_the_same_core() {
    let mh = MH::new();
    let mut ts = 100;
    // Fill one cache set with some data.
    for l in 0..parameter::HARVARD_PRI_D_CACHE_ASSO {
        let block_id: u64 = (l * PCACHE_SET) as u64;
        mh.access_memory_pblock_id(0, block_id, l as u64 + ts + 1, false, false, false);
    }

    ts += 100;

    // If I access any touched block, it should be hit.
    for l in 0..parameter::HARVARD_PRI_D_CACHE_ASSO {
        let block_id: u64 = (l * PCACHE_SET) as u64;
        assert_eq!(
            mh.access_memory_pblock_id(0, block_id, l as u64 + ts + 1, false, false, false),
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        );
    }

    // OK, now there is an access with a reversed timestamp.
    let eval_block_id = (128 * PCACHE_SET) as u64;
    // The following line should trigger an assertion failure.
    assert_eq!(
        mh.access_memory_pblock_id(0, eval_block_id, 1, false, false, false),
        CacheHierarchyAccessResult::Miss
    );
}

#[test]
fn write_invalidation_coherence() {
    let mut mh = MH::new();

    let block_id = 1024;
    // Core 0 gets a read permission at 1.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 1, false, false, false),
        CacheHierarchyAccessResult::Miss
    );
    // Core 1 get a read permission at 10.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 10, false, false, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );
    // Core 2 get a write permission at 50.
    assert_eq!(
        mh.access_memory_pblock_id(2, block_id, 50, true, false, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );
    // Now, core 0 and core 1 should have invalid the cache.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 1);
    assert_eq!(sharers[&2], BlockState::Modified);

    // Now core 0 gets a read permission at 100.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 100, false, false, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 2);
    assert_eq!(sharers[&2], BlockState::Shared);
    assert_eq!(sharers[&0], BlockState::Shared);
}

#[test]
fn raw_and_war() {
    let mut mh = MH::new();

    let block_id = 1024;
    // Core 0 gets a read permission at 0.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 1, false, false, false),
        CacheHierarchyAccessResult::Miss
    );
    // Core 1 get a read permission at 10.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 10, false, false, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );
    // Core 2 get a write permission at 5.
    assert_eq!(
        mh.access_memory_pblock_id(2, block_id, 5, true, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );
    // Now, core 0 should have invalid the cache.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 2);
    assert_eq!(sharers[&2], BlockState::Shared);
    assert_eq!(sharers[&1], BlockState::Shared);
}

#[test]
fn rarw() {
    let mut mh = MH::new();

    let block_id = 1024;
    // Core 0 gets a read permission at 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, false, false, false),
        CacheHierarchyAccessResult::Miss
    );
    // Core 0 get a write permission at 20.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 20, true, false, false),
        if parameter::ENABLE_EXCLUSIVE_CACHE_STATE {
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        } else {
            CacheHierarchyAccessResult::MissDueToPermission
        }
    );

    // Core 1 get a read permission at 0.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 0, false, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );

    // Now, core 1 should have invalid the cache.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 1);
    assert_eq!(sharers[&0], BlockState::Modified);
}

#[test]
fn waw() {
    let mut mh = MH::new();
    let block_id = 1024;
    // Core 0 gets a write permission at timestamp 10
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, true, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Core 1 gets a write permission at timestamp 5.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 5, true, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );

    // Now, the only owner of the data should be core 0.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 1);
    assert_eq!(sharers[&0], BlockState::Modified);
}

#[test]
fn wwaw() {
    let mut mh = MH::new();
    let block_id = 1024;
    // Core 0 gets a write permission at timestamp 10
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, true, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Core 0 gets a write permission at timestamp 20. It should be a bit and no broadcast to the directory.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 20, true, false, false),
        CacheHierarchyAccessResult::HitInSelfPrivateCache
    );

    // Core 1 gets a write permission at timestamp 15.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 15, true, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );

    // Now, the only owner of the data should be core 0.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 1);
    assert_eq!(sharers[&0], BlockState::Modified);
}

#[test]
fn rae() {
    let mut mh = MH::new();
    let block_id = 1024;
    // Core 0 writes to this block at timestamp 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, true, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // This block is evicted due to contention.
    for i in 0..parameter::HARVARD_PRI_D_CACHE_ASSO {
        let block_id: u64 = (10 * (i + 1) * PCACHE_SET + block_id as usize) as u64;
        mh.access_memory_pblock_id(0, block_id, (100 + i) as u64, false, false, false);
    }

    // Now, the line is not in the memory hierarchy anymore.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 0);

    // Then, there is a reader replica which is created before core 0 writes to the position.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 5, false, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );

    // Still, there should be no reader replica.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 0);
}

#[test]
fn eae() {
    let mut mh = MH::new();
    // Core 0 accesses the core at 200 and evicts the block at 216 with the dirty permission.
    let block_id = 1024;
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 200, true, false, false),
        CacheHierarchyAccessResult::Miss
    );

    for i in 0..parameter::HARVARD_PRI_D_CACHE_ASSO {
        let block_id: u64 = (100 * (i + 1) * PCACHE_SET + block_id as usize) as u64;
        mh.access_memory_pblock_id(0, block_id, (200 + i) as u64, false, false, false);
    }

    // That block should be evicted from the memory hierarchy.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 0);

    // Core 1 accesses the core at 10 and evict the block
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 10, true, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );

    for i in 0..parameter::HARVARD_PRI_D_CACHE_ASSO {
        let block_id: u64 = (255 * (i + 1) * PCACHE_SET + block_id as usize) as u64;
        mh.access_memory_pblock_id(1, block_id, (10 + i) as u64, false, false, false);
    }

    // Now there should be nothing in the private cache.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 0);

    // Now, the block should be in the last-level cache.
    assert_eq!(
        mh.where_is_the_block(block_id),
        BlockPosition::InSharedCache
    );
}

#[test]
fn wae() {
    let mut mh = MH::new();
    let block_id = 1024;
    // Core 0 writes the block at 200 and evicts from the 217.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 200, true, false, false),
        CacheHierarchyAccessResult::Miss
    );

    for i in 0..parameter::HARVARD_PRI_D_CACHE_ASSO {
        let block_id: u64 = (100 * (i + 1) * PCACHE_SET + block_id as usize) as u64;
        mh.access_memory_pblock_id(0, block_id, (200 + i) as u64, false, false, false);
    }

    // That block should be evicted from the memory hierarchy.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 0);

    // Core 1 writes to the block at 10.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 10, true, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );

    // Now there should be nothing in the private cache.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 0);

    // And it is in the last-level cache.
    assert_eq!(
        mh.where_is_the_block(block_id),
        BlockPosition::InSharedCache
    );
}

#[test]
fn eaw() {
    let mut mh = MH::new();
    let block_id = 1024;

    // Core 0 writes to the block at 200.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 200, true, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Core 1 evicts the block at 100 with the dirty permission. the write happens at 10.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 10, true, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );

    for i in 0..parameter::HARVARD_PRI_D_CACHE_ASSO {
        let block_id: u64 = (100 * (i + 1) * PCACHE_SET + block_id as usize) as u64;
        mh.access_memory_pblock_id(1, block_id, (10 + i) as u64, false, false, false);
    }

    // Now there should be only a copy from core 0 in private caches.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 1);
}

#[test]
fn ear() {
    let mut mh = MH::new();
    let block_id = 1024;

    // Core 0 reads the block at 100.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 100, false, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Then, core 1 evicts the block before 50. It creates a write access at 10.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 10, true, false, false),
        CacheHierarchyAccessResult::MissInPrivateCache
    );

    for i in 0..parameter::HARVARD_PRI_D_CACHE_ASSO {
        let block_id: u64 = (100 * (i + 1) * PCACHE_SET + block_id as usize) as u64;
        mh.access_memory_pblock_id(1, block_id, (10 + i) as u64, false, false, false);
    }

    // Now, only core 0 has the block. And it has the exclusive permission.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 1);
    assert_eq!(sharers[&0], BlockState::Shared);
}

#[test]
fn rar() {
    let mut mh = MH::new();
    let block_id = 1024;

    // Core 0 reads the block at 10.
    assert_eq!(
        mh.access_memory_pblock_id(0, block_id, 10, false, false, false),
        CacheHierarchyAccessResult::Miss
    );

    // Core 1 reads the block at 5.
    assert_eq!(
        mh.access_memory_pblock_id(1, block_id, 5, false, false, false),
        CacheHierarchyAccessResult::HitInOtherPrivateCache
    );

    // Now there should be two replicas of core 0 and core 1 in the private cache.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 2);
    assert_eq!(sharers[&0], BlockState::Shared);
    assert_eq!(sharers[&1], BlockState::Shared);
}
