use worm_cache::components::cache_hierarchy::hierarchy::MemoryHierarchy;
use worm_cache::components::cache_hierarchy::private_cache::ParallelUnifiedPrivateCache;
use worm_cache::components::cache_hierarchy::shared_cache::ParallelSingleSharedCache;
use worm_cache::components::debug::statistics::Statistics;
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
    { !parameter::DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION },
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
>;

#[allow(dead_code)]
fn test_hit_last() {
    let mh = MH::new();

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

#[allow(dead_code)]
fn testing_pcache_always_miss() {
    let mh = MH::new();

    // What I need to do is just to access the block id belonging to a specific shared cache set.
    // The block id is calculated as follows:
    // block_id = set_id * associativity + way_id

    let mut ts: u64 = 0;
    let mut block_id = 42;

    loop {
        for _ in 0..64 {
            mh.access_memory_pblock_id(0, block_id, ts, false, false, false);
            ts += 1;
            block_id += parameter::UNIFIED_PRI_CACHE_SET as u64;
        }
        block_id = 42;

        if ts > 1024 * 1024 * 10 {
            break;
        }
    }

    // print the miss rate of the data cache and shared cache from core 0. They should be 100%.
    println!("{}", Statistics::global_get_line_for_all_cores(0)[0]);
}

#[allow(dead_code)]
fn testing_always_miss() {
    let mh = MH::new();

    // What I need to do is just to access the block id belonging to a specific shared cache set.
    // The block id is calculated as follows:
    // block_id = set_id * associativity + way_id

    let mut ts: u64 = 0;
    let mut block_id = 37;

    loop {
        for _ in 0..64 {
            mh.access_memory_pblock_id(0, block_id, ts, false, false, false);
            ts += 1;
            block_id += parameter::SHARED_CACHE_SET as u64;
        }
        block_id = 37;

        if ts > 1024 * 1024 * 1000 {
            break;
        }
    }

    // print the miss rate of the data cache and shared cache from core 0. They should be 100%.
    println!("{}", Statistics::global_get_line_for_all_cores(0)[0]);
}

pub fn main() {
    testing_pcache_always_miss();
}
