use crate::components::cache_hierarchy::common::{
    CacheAccessType, CacheHierarchyAccessResult, InfiniteDirectory, InstructionPrefetchTrigger,
    ParallelHarvardPrivateCache, ParallelLRUSharedCache, ParallelUnifiedPrivateCache, PrivateCache,
    PrivateCacheLine, statistics::ZeroSharedCacheSetStatistics,
};
use crate::components::cache_hierarchy::mmu::NoMMU;
use crate::components::cache_hierarchy::{CacheBlockRequest, MemoryHierarchy};
use crate::parameter;
use crate::util::get_monotonic_ts;

use super::ParallelMemoryHierarchy;

// One instruction set makes replacement explicit. The LLC has room to retain
// the blocks used below, so refills also exercise the shared-cache hit path.
type TestHierarchy<const CORES: usize, const I_WAYS: usize> = ParallelMemoryHierarchy<
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
        .access_memory_pblock_id(&request(core_id, block_id, access_type), get_monotonic_ts())
        .0
}

fn resident<const CORES: usize, const I_WAYS: usize>(
    hierarchy: &TestHierarchy<CORES, I_WAYS>,
    core_id: u32,
    block_id: u64,
) -> Option<PrivateCacheLine> {
    // Inspection must not perform a demand access, which would change the flag.
    hierarchy
        .private_caches
        .get_set_for_fill(&request(core_id, block_id, CacheAccessType::InstructionFetch))
        .poke(block_id)
}

fn assert_state<const CORES: usize, const I_WAYS: usize>(
    hierarchy: &TestHierarchy<CORES, I_WAYS>,
    core_id: u32,
    block_id: u64,
    prefetched: bool,
    demand_accessed: bool,
) {
    let line = resident(hierarchy, core_id, block_id)
        .expect("instruction block should be resident");
    assert_eq!(line.prefetched, prefetched);
    assert_eq!(line.demand_accessed, demand_accessed);
    assert_eq!(line.has_been_demand_accessed(), demand_accessed);
}

#[test]
fn cold_instruction_fill_distinguishes_demand_from_prefetch() {
    for (access_type, prefetched, demand_accessed) in [
        (CacheAccessType::InstructionFetch, false, true),
        (CacheAccessType::InstructionPrefetch, true, false),
    ] {
        let hierarchy = TestHierarchy::<1, 1>::new();
        assert_eq!(access(&hierarchy, 0, 100, access_type), CacheHierarchyAccessResult::Miss);
        assert_state(&hierarchy, 0, 100, prefetched, demand_accessed);
    }
}

#[test]
fn first_demand_hit_marks_a_prefetched_line_without_changing_its_origin() {
    let hierarchy = TestHierarchy::<1, 1>::new();
    access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
    assert_state(&hierarchy, 0, 100, true, false);

    // Redundant prefetches do not count as consuming the instruction block.
    assert_eq!(
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch),
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
    assert_state(&hierarchy, 0, 100, true, false);

    for access_type in [
        CacheAccessType::InstructionFetch,
        CacheAccessType::InstructionPrefetch,
        CacheAccessType::InstructionFetch,
    ] {
        assert_eq!(access(&hierarchy, 0, 100, access_type), CacheHierarchyAccessResult::HitInSelfPrivateCache);
        assert_state(&hierarchy, 0, 100, true, true);
    }
}

#[test]
fn redundant_prefetch_hits_do_not_clear_a_demand_filled_lines_history() {
    let hierarchy = TestHierarchy::<1, 1>::new();
    access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
    access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
    assert_state(&hierarchy, 0, 100, false, true);
}

#[test]
fn both_adjacent_line_triggers_install_unused_prefetches_and_track_their_first_use() {
    for trigger in [InstructionPrefetchTrigger::Access, InstructionPrefetchTrigger::Miss] {
        let hierarchy = TestHierarchy::<1, 4>::new();
        let demand = request(0, 100, CacheAccessType::InstructionFetch);
        let ts = get_monotonic_ts();
        let (result, _) = hierarchy.access_memory_pblock_id(&demand, ts);
        assert_eq!(result, CacheHierarchyAccessResult::Miss);
        hierarchy.prefetch_adjacent_instruction_lines(&demand, &result, ts, 2, trigger);

        assert_state(&hierarchy, 0, 100, false, true);
        assert_state(&hierarchy, 0, 101, true, false);
        assert_state(&hierarchy, 0, 102, true, false);

        let next_demand = request(0, 101, CacheAccessType::InstructionFetch);
        let ts = get_monotonic_ts();
        let (result, _) = hierarchy.access_memory_pblock_id(&next_demand, ts);
        assert_eq!(result, CacheHierarchyAccessResult::HitInSelfPrivateCache);
        hierarchy.prefetch_adjacent_instruction_lines(&next_demand, &result, ts, 2, trigger);

        assert_state(&hierarchy, 0, 101, true, true);
        // Access mode re-prefetches 102, which must still be unused.
        assert_state(&hierarchy, 0, 102, true, false);
        match trigger {
            InstructionPrefetchTrigger::Access => assert_state(&hierarchy, 0, 103, true, false),
            InstructionPrefetchTrigger::Miss => assert!(resident(&hierarchy, 0, 103).is_none()),
        }
    }
}

#[test]
fn replacement_and_llc_refill_start_a_new_demand_access_history() {
    let hierarchy = TestHierarchy::<1, 1>::new();
    access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
    assert_state(&hierarchy, 0, 100, false, true);

    access(&hierarchy, 0, 101, CacheAccessType::InstructionPrefetch);
    assert!(resident(&hierarchy, 0, 100).is_none());
    assert_state(&hierarchy, 0, 101, true, false);

    // Neither the old local residency nor the LLC supply carries use history
    // into this new, still-unused instruction-prefetch installation.
    assert_eq!(
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch),
        CacheHierarchyAccessResult::HitInSharedCache,
    );
    assert_state(&hierarchy, 0, 100, true, false);

    assert_eq!(
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::HitInSharedCache,
    );
    assert_state(&hierarchy, 0, 101, false, true);
}

#[test]
fn instruction_refill_from_shared_data_copy_uses_the_incoming_access_type() {
    for (access_type, prefetched, demand_accessed) in [
        (CacheAccessType::InstructionFetch, false, true),
        (CacheAccessType::InstructionPrefetch, true, false),
    ] {
        let hierarchy = TestHierarchy::<1, 1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::DataRead);
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        // Keep the data replica, but replace the instruction replica so this
        // refill uses the directory's already-shared replica path.
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);
        assert_eq!(
            access(&hierarchy, 0, 100, access_type),
            CacheHierarchyAccessResult::HitInOtherPrivateCache,
        );
        assert_state(&hierarchy, 0, 100, prefetched, demand_accessed);
    }
}

#[test]
fn instruction_refill_from_dirty_owner_uses_the_incoming_access_type() {
    for (access_type, prefetched, demand_accessed) in [
        (CacheAccessType::InstructionFetch, false, true),
        (CacheAccessType::InstructionPrefetch, true, false),
    ] {
        let hierarchy = TestHierarchy::<1, 1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::DataWrite);
        assert_eq!(
            access(&hierarchy, 0, 100, access_type),
            CacheHierarchyAccessResult::HitInOtherPrivateCache,
        );
        assert_state(&hierarchy, 0, 100, prefetched, demand_accessed);
    }
}

#[test]
fn coherence_invalidation_clears_history_before_a_new_instruction_residency() {
    for consume_prefetch in [false, true] {
        let hierarchy = TestHierarchy::<1, 1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        if consume_prefetch {
            access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        }
        assert_state(&hierarchy, 0, 100, true, consume_prefetch);
        access(&hierarchy, 0, 100, CacheAccessType::DataWrite);
        {
            let demand = request(0, 100, CacheAccessType::InstructionFetch);
            let set = hierarchy.private_caches.get_set_for_fill(&demand);
            assert!(set.poke(100).is_none());
            assert!(set.lines.iter().all(|line| !line.is_valid() && !line.demand_accessed));
        }
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        assert_state(&hierarchy, 0, 100, true, false);
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        assert_state(&hierarchy, 0, 100, true, true);
    }
}

#[test]
fn non_instruction_fills_do_not_record_instruction_demand_accesses() {
    for access_type in [
        CacheAccessType::DataRead,
        CacheAccessType::DataWrite,
        CacheAccessType::PageWalkRead,
        CacheAccessType::PrefetchRead,
        CacheAccessType::PrefetchWrite,
    ] {
        let hierarchy = TestHierarchy::<1, 1>::new();
        access(&hierarchy, 0, 100, access_type);
        let data_request = request(0, 100, CacheAccessType::DataRead);
        let line = hierarchy.private_caches.get_set_for_fill(&data_request)
            .poke(100).expect("data block should be resident");
        assert!(!line.demand_accessed);
    }
}

#[test]
fn unified_data_accesses_and_permission_upgrades_preserve_instruction_use_history() {
    for consume_prefetch in [false, true] {
        let hierarchy = UnifiedTestHierarchy::new();
        access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
        if consume_prefetch {
            access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        }

        // DataWrite requires the store/upgrade fill branch; the other accesses
        // are hits. None counts as a demand instruction access or clears one.
        for access_type in [
            CacheAccessType::DataRead,
            CacheAccessType::PageWalkRead,
            CacheAccessType::PrefetchRead,
            CacheAccessType::DataWrite,
            CacheAccessType::PrefetchWrite,
            CacheAccessType::InstructionPrefetch,
        ] {
            access(&hierarchy, 0, 100, access_type);
            let data_request = request(0, 100, CacheAccessType::DataRead);
            let line = hierarchy.private_caches.get_set_for_fill(&data_request)
                .poke(100).expect("unified block should remain resident");
            assert!(line.prefetched);
            assert_eq!(line.demand_accessed, consume_prefetch);
        }

        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        let demand = request(0, 100, CacheAccessType::InstructionFetch);
        let line = hierarchy.private_caches.get_set_for_fill(&demand)
            .poke(100).unwrap();
        assert!(line.has_been_demand_accessed());
        assert!(line.prefetched);
    }
}

#[test]
fn a_unified_data_residency_records_its_first_instruction_demand_only() {
    let hierarchy = UnifiedTestHierarchy::new();
    access(&hierarchy, 0, 100, CacheAccessType::DataRead);
    access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
    {
        let demand = request(0, 100, CacheAccessType::InstructionFetch);
        let line = hierarchy.private_caches.get_set_for_fill(&demand)
            .poke(100).unwrap();
        assert!(!line.prefetched);
        assert!(!line.demand_accessed);
    }

    assert_eq!(
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
    // A subsequent data store upgrades permissions and changes instruction/data
    // classification, but must retain the instruction demand already served.
    access(&hierarchy, 0, 100, CacheAccessType::DataWrite);
    let data_request = request(0, 100, CacheAccessType::DataRead);
    let line = hierarchy.private_caches.get_set_for_fill(&data_request)
        .poke(100).unwrap();
    assert!(!line.prefetched);
    assert!(line.demand_accessed);
}

#[test]
fn another_cores_instruction_fill_does_not_inherit_the_suppliers_use_history() {
    // Global statistics/traffic storage follows the selected configuration.
    if parameter::CORE_COUNT < 2 {
        return;
    }
    for (supplier_type, requester_type, requester_prefetched, requester_accessed) in [
        (CacheAccessType::InstructionPrefetch, CacheAccessType::InstructionFetch, false, true),
        (CacheAccessType::InstructionFetch, CacheAccessType::InstructionPrefetch, true, false),
    ] {
        let hierarchy = TestHierarchy::<2, 1>::new();
        let supplier_accessed = supplier_type == CacheAccessType::InstructionFetch;
        access(&hierarchy, 0, 100, supplier_type);
        assert_eq!(
            access(&hierarchy, 1, 100, requester_type),
            CacheHierarchyAccessResult::HitInOtherPrivateCache,
        );
        assert_state(&hierarchy, 1, 100, requester_prefetched, requester_accessed);
        assert_state(&hierarchy, 0, 100, !supplier_accessed, supplier_accessed);
    }
}
