use crate::components::cache_hierarchy::common::{
    statistics::ZeroSharedCacheSetStatistics, CacheAccessType, CacheHierarchyAccessResult,
    InfiniteDirectory, InstructionPrefetchTrigger, ParallelHarvardPrivateCache,
    ParallelLRUSharedCache, ParallelUnifiedPrivateCache, PrivateCache, PrivateCacheLine,
};
use crate::components::cache_hierarchy::mmu::NoMMU;
use crate::components::cache_hierarchy::{CacheBlockRequest, MemoryHierarchy};
use crate::util::get_monotonic_ts;

use super::ParallelMemoryHierarchy;

// A single instruction set makes replacement predictable. Tests inspect lines
// through poke(), so checking a flag never consumes an unused prefetch.
type TestHierarchy<const I_WAYS: usize> = ParallelMemoryHierarchy<
    NoMMU,
    ParallelHarvardPrivateCache<1, 1, I_WAYS, 1, 1>,
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

type UnifiedTestHierarchy = ParallelMemoryHierarchy<
    NoMMU,
    ParallelUnifiedPrivateCache<1, 1, 4>,
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

fn modes() -> [InstructionPrefetchTrigger; 3] {
    [
        InstructionPrefetchTrigger::Access,
        InstructionPrefetchTrigger::Miss,
        InstructionPrefetchTrigger::MissOrPrefetchHit,
    ]
}

fn request(block_id: u64, access_type: CacheAccessType) -> CacheBlockRequest {
    CacheBlockRequest {
        core_id: 0,
        block_id,
        access_type,
        is_os: false,
        pc: 0x1000,
    }
}

fn access(
    hierarchy: &impl MemoryHierarchy,
    block_id: u64,
    access_type: CacheAccessType,
) -> CacheHierarchyAccessResult {
    hierarchy
        .access_memory_pblock_id_with_prefetch_event(&request(block_id, access_type), get_monotonic_ts())
        .0
}

fn demand_and_prefetch(
    hierarchy: &impl MemoryHierarchy,
    block_id: u64,
    count: usize,
    trigger: InstructionPrefetchTrigger,
) -> CacheHierarchyAccessResult {
    let demand = request(block_id, CacheAccessType::InstructionFetch);
    let ts = get_monotonic_ts();
    let (result, _) = hierarchy.access_memory_pblock_id_with_prefetch_event(&demand, ts);
    hierarchy.prefetch_adjacent_instruction_lines(&demand, &result, ts, count, trigger);
    result
}

fn resident<const I_WAYS: usize>(
    hierarchy: &TestHierarchy<I_WAYS>,
    block_id: u64,
) -> Option<PrivateCacheLine> {
    hierarchy
        .private_caches
        .get_set_for_fill(&request(block_id, CacheAccessType::InstructionFetch))
        .poke(block_id)
}

fn assert_flags<const I_WAYS: usize>(
    hierarchy: &TestHierarchy<I_WAYS>,
    block_id: u64,
    prefetched: bool,
    demand_accessed: bool,
) {
    let line = resident(hierarchy, block_id).expect("instruction line should be resident");
    assert_eq!(line.prefetched, prefetched);
    assert_eq!(line.demand_accessed, demand_accessed);
}

#[test]
fn trigger_policy_distinguishes_first_prefetch_use_from_ordinary_hits_and_misses() {
    use CacheHierarchyAccessResult::*;
    for (result, access_expected, miss_expected, new_expected) in [
        (HitInSelfPrivateCache, true, false, false),
        (PrefetchHitInSelfPrivateCache, true, false, true),
        (MissDueToPermission, true, true, true),
        (HitInOtherPrivateCache, true, true, true),
        (HitInSharedCache, true, true, true),
        (Miss, true, true, true),
        (Unknown, true, true, true),
    ] {
        assert_eq!(InstructionPrefetchTrigger::Access.should_trigger(&result), access_expected);
        assert_eq!(InstructionPrefetchTrigger::Miss.should_trigger(&result), miss_expected);
        assert_eq!(
            InstructionPrefetchTrigger::MissOrPrefetchHit.should_trigger(&result),
            new_expected,
        );
    }
}

#[test]
fn all_modes_prefetch_exactly_the_configured_count_after_a_cold_miss() {
    for trigger in modes() {
        for count in [0, 1, 3] {
            let hierarchy = TestHierarchy::<8>::new();
            assert_eq!(
                demand_and_prefetch(&hierarchy, 100, count, trigger),
                CacheHierarchyAccessResult::Miss,
            );
            assert_flags(&hierarchy, 100, false, true);
            for offset in 1..=count {
                assert_flags(&hierarchy, 100 + offset as u64, true, false);
            }
            // A generated prefetch must not recursively start its own stream.
            assert!(resident(&hierarchy, 101 + count as u64).is_none());
        }
    }
}

#[test]
fn ordinary_demand_hits_only_trigger_access_mode() {
    for trigger in modes() {
        let hierarchy = TestHierarchy::<4>::new();
        access(&hierarchy, 100, CacheAccessType::InstructionFetch);
        assert_eq!(
            demand_and_prefetch(&hierarchy, 100, 1, trigger),
            CacheHierarchyAccessResult::HitInSelfPrivateCache,
        );
        assert_flags(&hierarchy, 100, false, true);
        assert_eq!(
            resident(&hierarchy, 101).is_some(),
            trigger == InstructionPrefetchTrigger::Access,
        );
    }
}

#[test]
fn first_prefetch_consumption_triggers_the_new_mode_exactly_once_per_residency() {
    for trigger in modes() {
        let hierarchy = TestHierarchy::<8>::new();
        access(&hierarchy, 100, CacheAccessType::InstructionPrefetch);
        assert_flags(&hierarchy, 100, true, false);
        assert_eq!(
            demand_and_prefetch(&hierarchy, 100, 1, trigger),
            CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
        );
        // The result carries the old state although the resident line has
        // already been marked consumed before the prefetch decision runs.
        assert_flags(&hierarchy, 100, true, true);
        let first_should_trigger = trigger != InstructionPrefetchTrigger::Miss;
        assert_eq!(resident(&hierarchy, 101).is_some(), first_should_trigger);
        assert!(resident(&hierarchy, 102).is_none());

        // A larger second count makes a mistakenly repeated trigger visible.
        assert_eq!(
            demand_and_prefetch(&hierarchy, 100, 2, trigger),
            CacheHierarchyAccessResult::HitInSelfPrivateCache,
        );
        assert_flags(&hierarchy, 100, true, true);
        assert_eq!(
            resident(&hierarchy, 102).is_some(),
            trigger == InstructionPrefetchTrigger::Access,
        );
    }
}

#[test]
fn new_mode_extends_a_stream_when_each_unused_prefetched_line_is_consumed() {
    let hierarchy = TestHierarchy::<8>::new();
    let trigger = InstructionPrefetchTrigger::MissOrPrefetchHit;
    assert_eq!(demand_and_prefetch(&hierarchy, 100, 2, trigger), CacheHierarchyAccessResult::Miss);
    assert_flags(&hierarchy, 101, true, false);
    assert_flags(&hierarchy, 102, true, false);
    assert_eq!(
        demand_and_prefetch(&hierarchy, 101, 2, trigger),
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
    );
    assert_flags(&hierarchy, 101, true, true);
    // The redundant prefetch of 102 has not consumed its first-demand event.
    assert_flags(&hierarchy, 102, true, false);
    assert_flags(&hierarchy, 103, true, false);
    assert!(resident(&hierarchy, 104).is_none());
    assert_eq!(
        demand_and_prefetch(&hierarchy, 101, 3, trigger),
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
    assert!(resident(&hierarchy, 104).is_none());
    assert_eq!(
        demand_and_prefetch(&hierarchy, 102, 2, trigger),
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
    );
    assert_flags(&hierarchy, 102, true, true);
    assert_flags(&hierarchy, 103, true, false);
    assert_flags(&hierarchy, 104, true, false);
}

#[test]
fn zero_count_still_consumes_the_first_demand_hit_without_generating_requests() {
    let hierarchy = TestHierarchy::<4>::new();
    access(&hierarchy, 100, CacheAccessType::InstructionPrefetch);
    assert_eq!(
        demand_and_prefetch(&hierarchy, 100, 0, InstructionPrefetchTrigger::MissOrPrefetchHit),
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
    );
    assert_flags(&hierarchy, 100, true, true);
    assert!(resident(&hierarchy, 101).is_none());
    assert_eq!(
        demand_and_prefetch(&hierarchy, 100, 1, InstructionPrefetchTrigger::MissOrPrefetchHit),
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
    assert!(resident(&hierarchy, 101).is_none());
}

#[test]
fn redundant_prefetch_hits_do_not_consume_or_rearm_the_first_use_event() {
    let hierarchy = TestHierarchy::<4>::new();
    access(&hierarchy, 100, CacheAccessType::InstructionPrefetch);
    for _ in 0..2 {
        assert_eq!(
            access(&hierarchy, 100, CacheAccessType::InstructionPrefetch),
            CacheHierarchyAccessResult::HitInSelfPrivateCache,
        );
        assert_flags(&hierarchy, 100, true, false);
    }
    assert_eq!(
        access(&hierarchy, 100, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
    );
    access(&hierarchy, 100, CacheAccessType::InstructionPrefetch);
    assert_flags(&hierarchy, 100, true, true);
    assert_eq!(
        access(&hierarchy, 100, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
}

#[test]
fn replacement_and_llc_refill_allow_a_fresh_first_use_event() {
    let hierarchy = TestHierarchy::<1>::new();
    access(&hierarchy, 100, CacheAccessType::InstructionPrefetch);
    assert_eq!(
        access(&hierarchy, 100, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
    );
    access(&hierarchy, 101, CacheAccessType::InstructionPrefetch);
    assert!(resident(&hierarchy, 100).is_none());
    assert_eq!(
        access(&hierarchy, 100, CacheAccessType::InstructionPrefetch),
        CacheHierarchyAccessResult::HitInSharedCache,
    );
    assert_flags(&hierarchy, 100, true, false);
    assert_eq!(
        access(&hierarchy, 100, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
    );
    assert_flags(&hierarchy, 100, true, true);
}

#[test]
fn coherence_invalidation_allows_a_fresh_first_use_event_after_refill() {
    let hierarchy = TestHierarchy::<4>::new();
    access(&hierarchy, 100, CacheAccessType::InstructionPrefetch);
    access(&hierarchy, 100, CacheAccessType::InstructionFetch);
    assert_flags(&hierarchy, 100, true, true);
    access(&hierarchy, 100, CacheAccessType::DataWrite);
    assert!(resident(&hierarchy, 100).is_none());
    assert_eq!(
        access(&hierarchy, 100, CacheAccessType::InstructionPrefetch),
        CacheHierarchyAccessResult::HitInOtherPrivateCache,
    );
    assert_flags(&hierarchy, 100, true, false);
    assert_eq!(
        demand_and_prefetch(&hierarchy, 100, 1, InstructionPrefetchTrigger::MissOrPrefetchHit),
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
    );
    assert_flags(&hierarchy, 100, true, true);
    assert_flags(&hierarchy, 101, true, false);
}

#[test]
fn private_data_supplier_does_not_determine_the_instruction_copy_first_use_state() {
    for supplier_type in [CacheAccessType::DataRead, CacheAccessType::DataWrite] {
        let hierarchy = TestHierarchy::<4>::new();
        access(&hierarchy, 100, supplier_type);
        assert_eq!(
            access(&hierarchy, 100, CacheAccessType::InstructionPrefetch),
            CacheHierarchyAccessResult::HitInOtherPrivateCache,
        );
        assert_flags(&hierarchy, 100, true, false);
        assert_eq!(
            demand_and_prefetch(&hierarchy, 100, 1, InstructionPrefetchTrigger::MissOrPrefetchHit),
            CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
        );
        assert_flags(&hierarchy, 100, true, true);
        assert_flags(&hierarchy, 101, true, false);
    }
}

#[test]
fn non_demand_instruction_requests_never_start_a_stream_in_the_new_mode() {
    for access_type in [
        CacheAccessType::InstructionPrefetch,
        CacheAccessType::DataRead,
        CacheAccessType::DataWrite,
        CacheAccessType::PageWalkRead,
        CacheAccessType::PrefetchRead,
        CacheAccessType::PrefetchWrite,
    ] {
        let hierarchy = TestHierarchy::<4>::new();
        // Even a triggering result must be gated by the actual request type.
        hierarchy.prefetch_adjacent_instruction_lines(
            &request(100, access_type),
            &CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
            get_monotonic_ts(),
            1,
            InstructionPrefetchTrigger::MissOrPrefetchHit,
        );
        assert!(resident(&hierarchy, 101).is_none());
    }
}

#[test]
fn unified_cache_data_access_does_not_consume_an_instruction_prefetch() {
    let hierarchy = UnifiedTestHierarchy::new();
    access(&hierarchy, 100, CacheAccessType::InstructionPrefetch);
    assert_eq!(
        access(&hierarchy, 100, CacheAccessType::DataRead),
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
    assert_eq!(
        demand_and_prefetch(&hierarchy, 100, 1, InstructionPrefetchTrigger::MissOrPrefetchHit),
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
    );
    let line = hierarchy
        .private_caches
        .get_set_for_fill(&request(100, CacheAccessType::InstructionFetch))
        .poke(100)
        .unwrap();
    assert!(line.prefetched && line.demand_accessed);
    assert_eq!(
        access(&hierarchy, 100, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
}
