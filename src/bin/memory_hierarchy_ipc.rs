use worm_cache::components::cache_hierarchy::hierarchy::MemoryHierarchy;
use worm_cache::components::cache_hierarchy::private_cache::ParallelUnifiedPrivateCache;
use worm_cache::components::cache_hierarchy::shared_cache::ParallelSingleSharedCache;
use worm_cache::components::NoMMU;

use worm_cache::parameter;

type MH = MemoryHierarchy<
    NoMMU,
    ParallelUnifiedPrivateCache<
        1,
        { parameter::UNIFIED_PRI_CACHE_SET },
        { parameter::UNIFIED_PRI_CACHE_ASSO },
    >,
    ParallelSingleSharedCache<
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    // ReplicatedSharedCache<
    //     { parameter::CORE_COUNT },
    //     { parameter::SHARED_CACHE_SET },
    //     { parameter::SHARED_CACHE_ASSO },
    //     { parameter::SHARED_CACHE_EXCLUSIVE },
    // >,
    { !parameter::DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION },
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
>;

pub fn main() {
    let mh = MH::new();

    // Case 1: Accesses hits different sets and the first way.

    // generate a simple array for accesses.
    // let accesses: Vec<u64> = (0..64).collect();
    // let mut ts = 0;

    // loop {
    //     for addr in &accesses {
    //         mh.access_memory_pblock_id(0, *addr, ts, false, false, false);
    //         ts += 1;
    //     }
    // }

    // Case 2: Access hit the same set and the last way.

    // Fill one specific cache set.

    let set_idx = 1;
    for i in 0..parameter::UNIFIED_PRI_CACHE_ASSO {
        mh.access_memory_pblock_id(
            0,
            (i as u64) * (parameter::UNIFIED_PRI_CACHE_SET as u64) + set_idx,
            0,
            false,
            false,
            false,
        );
    }

    println!("Set filled: {}", set_idx);

    // start testing. Access the same set and the last way.
    let addr = (parameter::UNIFIED_PRI_CACHE_ASSO as u64 - 1)
        * (parameter::UNIFIED_PRI_CACHE_SET as u64)
        + set_idx;

    let mut ts = 0;
    loop {
        mh.access_memory_pblock_id(0, addr, ts, false, false, false);
        ts += 1;
    }
}
