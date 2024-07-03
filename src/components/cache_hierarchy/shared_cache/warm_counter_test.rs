use statistics::ZeroSharedCacheSetStatistics;

use super::ParallelSingleSharedCache;
use super::*;

#[test]
fn warm_counter_normal_function() {
    let cache = ParallelSingleSharedCache::<ZeroSharedCacheSetStatistics, 1024, 4, false>::new();

    let which_set_to_target = 127;

    for idx in 0..4 {
        cache.insert(0, which_set_to_target + idx * 1024, 1, false, true);
    }

    assert_eq!(cache.warmed_sets_count(), 1);
}

#[test]
fn warm_counter_not_done() {
    let cache = ParallelSingleSharedCache::<ZeroSharedCacheSetStatistics, 1024, 5, false>::new();

    let which_set_to_target = 127;

    for idx in 0..4 {
        cache.insert(0, which_set_to_target + idx * 1024, 1, false, false);
    }

    assert_eq!(cache.warmed_sets_count(), 0);
}
