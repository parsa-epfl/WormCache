use crate::components::cache_hierarchy::common::{
    CacheAccessType, CacheHierarchyAccessResult, InfiniteDirectory, InstructionPrefetchTrigger,
    ParallelHarvardPrivateCache, ParallelLRUSharedCache, ParallelUnifiedPrivateCache, PrivateCache,
    PrivateCacheEvictedSlot, PrivateCacheSet, statistics::ZeroSharedCacheSetStatistics,
};
use crate::components::cache_hierarchy::mmu::NoMMU;
use crate::components::cache_hierarchy::{CacheBlockRequest, MemoryHierarchy};
use crate::debug::statistics::{EventType, Statistics, capture_records};
use crate::util::get_monotonic_ts;

use super::ParallelMemoryHierarchy;
use super::instruction_cache_statistics::fill_private_cache_set;

type TestHierarchy<const CORES: usize, const I_WAYS: usize = 1> = ParallelMemoryHierarchy<
    NoMMU,
    ParallelHarvardPrivateCache<CORES, 1, I_WAYS, 1, 1>,
    ParallelLRUSharedCache<ZeroSharedCacheSetStatistics, 4, 2, false>,
    InfiniteDirectory<4>,
    true,
    true,
    true,
    true,
    CORES,
    2,
    2,
    2,
    2,
    4,
    false,
    false,
    false,
    false,
    1,
    1,
    4,
    1,
>;

type UnifiedTestHierarchy = ParallelMemoryHierarchy<
    NoMMU,
    ParallelUnifiedPrivateCache<1, 1, 1>,
    ParallelLRUSharedCache<ZeroSharedCacheSetStatistics, 4, 2, false>,
    InfiniteDirectory<4>,
    true,
    true,
    true,
    true,
    1,
    2,
    2,
    2,
    2,
    4,
    false,
    false,
    false,
    false,
    1,
    1,
    4,
    1,
>;

fn request(core_id: u32, block_id: u64, access_type: CacheAccessType) -> CacheBlockRequest {
    CacheBlockRequest {
        core_id,
        block_id,
        access_type,
        is_os: false,
        pc: 0x1000,
    }
}

fn access(
    hierarchy: &impl MemoryHierarchy,
    core_id: u32,
    block_id: u64,
    access_type: CacheAccessType,
) -> CacheHierarchyAccessResult {
    hierarchy
        .access_memory_pblock_id(
            &request(core_id, block_id, access_type),
            get_monotonic_ts(),
        )
        .0
}

fn count(events: &[(u32, EventType, bool, u64)], event: EventType) -> u64 {
    events
        .iter()
        .filter(|(_, recorded, _, _)| *recorded as usize == event as usize)
        .map(|(_, _, _, increment)| *increment)
        .sum()
}

#[test]
fn instruction_fills_include_demand_and_prefetch_from_each_supply_path() {
    let (_, events) = capture_records(|| {
        for fill_type in [
            CacheAccessType::InstructionFetch,
            CacheAccessType::InstructionPrefetch,
        ] {
            let cold = TestHierarchy::<1>::new();
            assert_eq!(
                access(&cold, 0, 100, fill_type.clone()),
                CacheHierarchyAccessResult::Miss,
            );

            let llc = TestHierarchy::<1>::new();
            access(&llc, 0, 100, CacheAccessType::InstructionFetch);
            access(&llc, 0, 101, CacheAccessType::InstructionFetch);
            assert_eq!(
                access(&llc, 0, 100, fill_type.clone()),
                CacheHierarchyAccessResult::HitInSharedCache,
            );

            let shared_copy = TestHierarchy::<1>::new();
            access(&shared_copy, 0, 100, CacheAccessType::DataRead);
            access(&shared_copy, 0, 100, CacheAccessType::InstructionFetch);
            access(&shared_copy, 0, 101, CacheAccessType::InstructionFetch);
            assert_eq!(
                access(&shared_copy, 0, 100, fill_type.clone()),
                CacheHierarchyAccessResult::HitInOtherPrivateCache,
            );

            let dirty_owner = TestHierarchy::<1>::new();
            access(&dirty_owner, 0, 100, CacheAccessType::DataWrite);
            assert_eq!(
                access(&dirty_owner, 0, 100, fill_type),
                CacheHierarchyAccessResult::HitInOtherPrivateCache,
            );
        }
    });
    assert_eq!(count(&events, EventType::InstructionCacheFills), 16);
    assert_eq!(count(&events, EventType::InstructionCacheOverfetches), 0);
    // The prefetch iteration misses L1-I once for each supply path. Setup
    // accesses and the demand-fetch iteration must not count as prefetches.
    assert_eq!(count(&events, EventType::InstructionCachePrefetchesIssued), 4);
}

#[test]
fn demand_only_instruction_fills_have_zero_overfetches() {
    let (_, events) = capture_records(|| {
        let hierarchy = TestHierarchy::<1>::new();
        for block_id in [100, 101, 100] {
            access(&hierarchy, 0, block_id, CacheAccessType::InstructionFetch);
        }
    });
    assert_eq!(count(&events, EventType::InstructionCacheFills), 3);
    assert_eq!(count(&events, EventType::InstructionCacheOverfetches), 0);
    assert_eq!(count(&events, EventType::InstructionCachePrefetchesIssued), 0);
}

#[test]
fn unused_prefetch_is_counted_once_on_replacement_and_hits_are_not_fills() {
    let (_, events) = capture_records(|| {
        let hierarchy = TestHierarchy::<1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);
        access(&hierarchy, 0, 101, CacheAccessType::InstructionPrefetch);
        access(&hierarchy, 0, 102, CacheAccessType::InstructionFetch);
    });
    assert_eq!(count(&events, EventType::InstructionCacheFills), 3);
    assert_eq!(count(&events, EventType::InstructionCacheOverfetches), 1);
    // Both redundant prefetches hit L1-I: one on a prefetch-installed line,
    // and the other on a demand-installed line. Only the initial miss counts.
    assert_eq!(count(&events, EventType::InstructionCachePrefetchesIssued), 1);
}

#[test]
fn instruction_prefetch_is_counted_again_after_its_previous_residency_is_evicted() {
    let (_, events) = capture_records(|| {
        let hierarchy = TestHierarchy::<1>::new();
        assert_eq!(
            access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch),
            CacheHierarchyAccessResult::Miss,
        );
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);
        assert_eq!(
            access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch),
            CacheHierarchyAccessResult::HitInSharedCache,
        );
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
    });
    assert_eq!(count(&events, EventType::InstructionCachePrefetchesIssued), 2);
    assert_eq!(count(&events, EventType::InstructionCacheFills), 3);
}

#[test]
fn demand_and_data_prefetch_requests_are_not_issued_instruction_prefetches() {
    let (_, events) = capture_records(|| {
        let hierarchy = TestHierarchy::<1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        access(&hierarchy, 0, 200, CacheAccessType::PrefetchRead);
        access(&hierarchy, 0, 201, CacheAccessType::PrefetchWrite);
    });
    assert_eq!(count(&events, EventType::InstructionCachePrefetchesIssued), 0);
}

#[test]
fn adjacent_line_prefetches_count_only_candidates_missing_from_l1_instruction_cache() {
    let (_, events) = capture_records(|| {
        let hierarchy = TestHierarchy::<1, 4>::new();
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);
        let demand = request(0, 100, CacheAccessType::InstructionFetch);
        let (result, _) = hierarchy.access_memory_pblock_id(&demand, get_monotonic_ts());
        // Candidate 101 is already resident. Candidates 102 and 103 are not.
        hierarchy.prefetch_adjacent_instruction_lines(
            &demand,
            &result,
            get_monotonic_ts(),
            3,
            InstructionPrefetchTrigger::Access,
        );
        for block_id in [100, 101, 102, 103] {
            assert!(
                hierarchy
                    .private_caches
                    .get_set_for_fill(&request(0, block_id, CacheAccessType::InstructionFetch))
                    .poke(block_id)
                    .is_some()
            );
        }
    });
    assert_eq!(count(&events, EventType::InstructionCachePrefetchesIssued), 2);
    assert_eq!(count(&events, EventType::InstructionCacheFills), 4);
}

#[test]
fn consumed_prefetches_demand_lines_and_data_lines_are_not_overfetches() {
    let (_, events) = capture_records(|| {
        let hierarchy = TestHierarchy::<1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);
        access(&hierarchy, 0, 102, CacheAccessType::InstructionFetch);
        access(&hierarchy, 0, 200, CacheAccessType::DataRead);
        access(&hierarchy, 0, 201, CacheAccessType::PrefetchRead);
        access(&hierarchy, 0, 202, CacheAccessType::DataRead);
    });
    assert_eq!(count(&events, EventType::InstructionCacheFills), 3);
    assert_eq!(count(&events, EventType::InstructionCacheOverfetches), 0);
}

#[test]
fn unified_data_hits_and_permission_upgrades_preserve_unused_instruction_origin() {
    let (_, events) = capture_records(|| {
        let hierarchy = UnifiedTestHierarchy::new();
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        access(&hierarchy, 0, 100, CacheAccessType::DataRead);
        access(&hierarchy, 0, 100, CacheAccessType::DataWrite);
        let line = hierarchy
            .private_caches
            .get_set_for_fill(&request(0, 100, CacheAccessType::DataRead))
            .poke(100)
            .unwrap();
        assert!(line.prefetched);
        assert!(!line.demand_accessed);
        assert!(!line.is_instruction);
        // A data fill can evict an unused instruction-prefetch residency.
        access(&hierarchy, 0, 101, CacheAccessType::DataRead);
    });
    assert_eq!(count(&events, EventType::InstructionCacheFills), 1);
    assert_eq!(count(&events, EventType::InstructionCacheOverfetches), 1);
}

#[test]
fn coherence_invalidation_is_not_a_replacement_overfetch() {
    let (_, events) = capture_records(|| {
        let hierarchy = TestHierarchy::<1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        access(&hierarchy, 0, 100, CacheAccessType::DataWrite);
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);
    });
    assert_eq!(count(&events, EventType::InstructionCacheFills), 2);
    assert_eq!(count(&events, EventType::InstructionCacheOverfetches), 0);
}

#[test]
fn intervening_invalidation_prevents_counting_an_unevicted_candidate() {
    let (_, events) = capture_records(|| {
        let mut set = PrivateCacheSet::new(2);
        let prefetched = request(0, 100, CacheAccessType::InstructionPrefetch);
        fill_private_cache_set(
            &mut set,
            &prefetched,
            PrivateCacheEvictedSlot::Invalid(0),
            1,
            false,
            false,
            true,
            false,
        );
        let demand = request(0, 101, CacheAccessType::InstructionFetch);
        fill_private_cache_set(
            &mut set,
            &demand,
            PrivateCacheEvictedSlot::Invalid(1),
            2,
            false,
            false,
            false,
            true,
        );
        let proposed_victim = set.find_eviction_index();
        assert_eq!(proposed_victim, PrivateCacheEvictedSlot::Valid(0, 100));
        // The valid prefetch candidate remains resident; a newly invalid slot
        // is selected by the fill instead, so there is no actual eviction.
        set.invalidate(1);
        let demand = request(0, 102, CacheAccessType::InstructionFetch);
        assert_eq!(
            fill_private_cache_set(
                &mut set,
                &demand,
                proposed_victim,
                3,
                false,
                false,
                false,
                true,
            ),
            None,
        );
        assert!(set.poke(100).is_some());
    });
    assert_eq!(count(&events, EventType::InstructionCacheFills), 3);
    assert_eq!(count(&events, EventType::InstructionCacheOverfetches), 0);
}

#[test]
fn instruction_counter_columns_are_exported_in_statistics_csv() {
    let header = Statistics::get_header();
    let columns: Vec<_> = header.split(',').collect();
    for name in [
        "InstructionCacheFills",
        "InstructionCacheOverfetches",
        "InstructionCachePrefetchesIssued",
    ] {
        assert!(columns.contains(&name));
        assert!(columns.contains(&format!("{name}:u").as_str()));
        assert!(columns.contains(&format!("{name}:k").as_str()));
    }
}
