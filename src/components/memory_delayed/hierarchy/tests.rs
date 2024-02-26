use std::collections::HashMap;

use super::*;

type MH = TestingDelayedMemoryHierarchy;

#[derive(Debug, PartialEq, Eq)]
enum BlockPosition {
    InPrivateCache(Vec<u32>),
    InSharedCache,
    NotInCache,
}

impl TestingDelayedMemoryHierarchy {
    fn where_is_the_block(&mut self, block_id: u64) -> BlockPosition {
        let mut private_owner = vec![];
        for i in 0..parameter::CORE_COUNT {
            if self.private_caches[i].contains_block(block_id) {
                private_owner.push(i as u32);
            }
        }
        
        if !private_owner.is_empty() {
            return BlockPosition::InPrivateCache(private_owner);
        }

        if self.shared_cache.lookup(block_id) {
            return BlockPosition::InSharedCache;
        }
        return BlockPosition::NotInCache;
    }

    fn get_all_private_replicas(&mut self, block_id: u64) -> HashMap<u32, PrivateCacheState> {
        let mut result = HashMap::new();
        for i in 0..parameter::CORE_COUNT {
            if self.private_caches[i].contains_block(block_id) {
                result.insert(i as u32, self.private_caches[i].get_block_state(block_id));
            }
        }
        result
    }
}

#[test]
#[should_panic(expected = "assertion failed: res.ts <= ts")]
fn accesses_with_reversed_timestamp() {
    let mut mh = MH::new();
    let mut ts = 100;
    // Fill one cache set with some data.
    for l in 0..parameter::PRI_CACHE_ASSO {
        let block_id: u64 = (l * parameter::PRI_CACHE_SET) as u64;
        mh.access_memory_pblock_id(0, block_id, l as u64 + ts, false, false);
    }

    ts += 100;

    // If I access any touched block, it should be hit.
    for l in 0..parameter::PRI_CACHE_ASSO {
        let block_id: u64 = (l * parameter::PRI_CACHE_SET) as u64;
        assert_eq!(
            mh.access_memory_pblock_id(0, block_id, l as u64 + ts, false, false),
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        );
    }

    // OK, now there is an access with a reversed timestamp.
    let eval_block_id = (128 * parameter::PRI_CACHE_SET) as u64;
    // The following line should trigger an assertion failure.
    assert_eq!(
        mh.access_memory_pblock_id(0, eval_block_id, 0, false, false),
        CacheHierarchyAccessResult::Miss
    );
}

#[test]
fn coherence_invalidation() {
    let mut mh = MH::new();

    let block_id = 1024;
    // Core 0 gets a read permission at 0.
    assert_eq!(mh.access_memory_pblock_id(0, block_id, 0, false, false), CacheHierarchyAccessResult::Miss);
    // Core 1 get a read permission at 10. 
    assert_eq!(mh.access_memory_pblock_id(1, block_id, 10, false, false), CacheHierarchyAccessResult::HitInOtherPrivateCache);
    // Core 2 get a write permission at 5.
    assert_eq!(mh.access_memory_pblock_id(2, block_id, 50, true, false), CacheHierarchyAccessResult::HitInOtherPrivateCache);
    // Now, core 0 and core 1 should have invalid the cache.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 1);
    assert_eq!(sharers[&2], PrivateCacheState::DirtyExclusive);

    // Now core 0 gets a read permission at 100.
    assert_eq!(mh.access_memory_pblock_id(0, block_id, 100, false, false), CacheHierarchyAccessResult::HitInOtherPrivateCache);
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 2);
    assert_eq!(sharers[&2], PrivateCacheState::DirtyShared);
    assert_eq!(sharers[&0], PrivateCacheState::CleanShared);
}

#[test]
fn coherence_invalidation_with_reversed_timestamp() {
    let mut mh = MH::new();

    let block_id = 1024;
    // Core 0 gets a read permission at 0.
    assert_eq!(mh.access_memory_pblock_id(0, block_id, 0, false, false), CacheHierarchyAccessResult::Miss);
    // Core 1 get a read permission at 10. 
    assert_eq!(mh.access_memory_pblock_id(1, block_id, 10, false, false), CacheHierarchyAccessResult::HitInOtherPrivateCache);
    // Core 2 get a write permission at 5.
    assert_eq!(mh.access_memory_pblock_id(2, block_id, 5, true, false), CacheHierarchyAccessResult::HitInOtherPrivateCache);
    // Now, core 0 and core 1 should have invalid the cache.
    let sharers = mh.get_all_private_replicas(block_id);
    assert_eq!(sharers.len(), 2);
    assert_eq!(sharers[&2], PrivateCacheState::DirtyShared);
    assert_eq!(sharers[&1], PrivateCacheState::CleanShared);
}   

#[test]
fn coherence_get_sharer_with_reversed_timestamp() {
    
}
