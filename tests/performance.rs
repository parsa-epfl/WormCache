// This file contains the performance test of various configurations.

use perf_event::Builder;

use worm_cache::components::cache_hierarchy::hierarchy::CacheAccessType;
use worm_cache::components::cache_hierarchy::hierarchy::MemoryHierarchy;
use worm_cache::components::cache_hierarchy::private_cache::ParallelUnifiedPrivateCache;
use worm_cache::components::cache_hierarchy::shared_cache::statistics::ZeroSharedCacheSetStatistics;
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
        ZeroSharedCacheSetStatistics,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    { !parameter::DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION },
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
    { parameter::DIRECTORY_SHARD_COUNT },
>;

#[test]
fn testing_pcache_always_miss() {
    let mh = MH::new(true, 0, false);

    // What I need to do is just to access the block id belonging to a specific shared cache set.
    // The block id is calculated as follows:
    // block_id = set_id * associativity + way_id

    let mut ts: u64 = 1;
    let mut block_id = 42;

    let mut counter = Builder::new().build().unwrap();

    counter.enable().unwrap();
    loop {
        for _ in 0..64 {
            mh.access_memory_pblock_id_with_the_same_ts_and_vts(
                0,
                block_id,
                ts,
                CacheAccessType::DataRead,
            );
            ts += 1;
            block_id += parameter::UNIFIED_PRI_CACHE_SET as u64;
        }
        block_id = 42;

        if ts > 1024 * 1024 * 10 {
            break;
        }
    }
    counter.disable().unwrap();

    let count = counter.read().unwrap();
    let ave_count = count / 1024 / 1024 / 10;

    println!("{}", ave_count);
    assert!(ave_count < 1200);

    // print the miss rate of the data cache and shared cache from core 0. They should be 100%.
    println!("{}", Statistics::global_get_line_for_all_cores(0)[0]);
}

#[test]
fn testing_pcache_always_hit() {
    let mh = MH::new(true, 0, false);

    // What I need to do is just to access the block id belonging to a specific shared cache set.
    // The block id is calculated as follows:
    // block_id = set_id * associativity + way_id

    let mut ts: u64 = 1;
    let block_id = 42;

    let mut counter = Builder::new().build().unwrap();

    counter.enable().unwrap();
    loop {
        for _ in 0..64 {
            mh.access_memory_pblock_id_with_the_same_ts_and_vts(
                0,
                block_id,
                ts,
                CacheAccessType::DataRead,
            );
            ts += 1;
        }

        if ts > 1024 * 1024 * 10 {
            break;
        }
    }
    counter.disable().unwrap();

    let count = counter.read().unwrap();
    let ave_count = count / 1024 / 1024 / 10;

    println!("{}", ave_count);
    assert!(ave_count < 110);

    // print the miss rate of the data cache and shared cache from core 0. They should be 100%.
    println!("{}", Statistics::global_get_line_for_all_cores(0)[0]);
}
