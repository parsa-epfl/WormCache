use crate::components::cache_hierarchy::common::{
    CacheAccessType, CacheHierarchyAccessResult, InfiniteDirectory, ParallelHarvardPrivateCache,
    ParallelLRUSharedCache, ParallelUnifiedPrivateCache, PrivateCache, PrivateCacheLine,
    statistics::ZeroSharedCacheSetStatistics,
};
use crate::components::cache_hierarchy::mmu::NoMMU;
use crate::components::cache_hierarchy::{CacheBlockRequest, MemoryHierarchy};
use crate::parameter;
use crate::util::get_monotonic_ts;

use super::ParallelMemoryHierarchy;

// One instruction line and one data line per core make replacements explicit.
// The LLC retains both tested blocks after private-cache replacement.
type TestHierarchy<const CORES: usize> = ParallelMemoryHierarchy<
    NoMMU,
    ParallelHarvardPrivateCache<CORES, 1, 1, 1, 1>,
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

fn access<const CORES: usize>(
    hierarchy: &TestHierarchy<CORES>,
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

fn resident<const CORES: usize>(
    hierarchy: &TestHierarchy<CORES>,
    core_id: u32,
    block_id: u64,
    access_type: CacheAccessType,
) -> Option<PrivateCacheLine> {
    hierarchy
        .private_caches
        .get_set_for_fill(&request(core_id, block_id, access_type))
        .poke(block_id)
}

fn instruction_line<const CORES: usize>(
    hierarchy: &TestHierarchy<CORES>,
    core_id: u32,
    block_id: u64,
) -> PrivateCacheLine {
    resident(
        hierarchy,
        core_id,
        block_id,
        CacheAccessType::InstructionFetch,
    )
    .expect("instruction block should be resident")
}

#[test]
fn cold_instruction_fills_record_demand_or_prefetch_origin() {
    for (access_type, prefetched) in [
        (CacheAccessType::InstructionFetch, false),
        (CacheAccessType::InstructionPrefetch, true),
    ] {
        let hierarchy = TestHierarchy::<1>::new();

        assert_eq!(
            access(&hierarchy, 0, 100, access_type),
            CacheHierarchyAccessResult::Miss,
        );
        let line = instruction_line(&hierarchy, 0, 100);
        assert!(line.is_instruction());
        assert_eq!(line.prefetched, prefetched);
    }
}

#[test]
fn demand_hits_and_redundant_prefetch_hits_preserve_the_original_fill() {
    for (fill_type, prefetched) in [
        (CacheAccessType::InstructionFetch, false),
        (CacheAccessType::InstructionPrefetch, true),
    ] {
        let hierarchy = TestHierarchy::<1>::new();
        access(&hierarchy, 0, 100, fill_type);

        for access_type in [
            CacheAccessType::InstructionFetch,
            CacheAccessType::InstructionPrefetch,
            CacheAccessType::InstructionFetch,
        ] {
            assert_eq!(
                access(&hierarchy, 0, 100, access_type),
                CacheHierarchyAccessResult::HitInSelfPrivateCache,
            );
            assert_eq!(instruction_line(&hierarchy, 0, 100).prefetched, prefetched);
        }
    }
}

#[test]
fn replacement_and_llc_refills_assign_origin_from_the_new_request() {
    let hierarchy = TestHierarchy::<1>::new();

    access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
    assert!(instruction_line(&hierarchy, 0, 100).prefetched);

    // The one-way instruction cache replaces a prefetched line with demand.
    access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);
    assert!(!instruction_line(&hierarchy, 0, 101).prefetched);
    assert!(resident(&hierarchy, 0, 100, CacheAccessType::InstructionFetch).is_none());

    // Its former prefetch origin must not survive a new demand residency.
    assert_eq!(
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::HitInSharedCache,
    );
    assert!(!instruction_line(&hierarchy, 0, 100).prefetched);

    // Conversely, a formerly demand-fetched block is now brought in by prefetch.
    assert_eq!(
        access(&hierarchy, 0, 101, CacheAccessType::InstructionPrefetch),
        CacheHierarchyAccessResult::HitInSharedCache,
    );
    assert!(instruction_line(&hierarchy, 0, 101).prefetched);
}

#[test]
fn instruction_refills_from_a_shared_data_copy_record_the_incoming_origin() {
    for (access_type, prefetched) in [
        (CacheAccessType::InstructionFetch, false),
        (CacheAccessType::InstructionPrefetch, true),
    ] {
        let hierarchy = TestHierarchy::<1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::DataRead);
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch);
        // Keep the data replica but replace the instruction replica. The
        // following read uses the directory's already-shared replica path.
        access(&hierarchy, 0, 101, CacheAccessType::InstructionFetch);

        assert_eq!(
            access(&hierarchy, 0, 100, access_type),
            CacheHierarchyAccessResult::HitInOtherPrivateCache,
        );
        assert_eq!(instruction_line(&hierarchy, 0, 100).prefetched, prefetched);
    }
}

#[test]
fn instruction_refills_from_a_dirty_data_owner_record_the_incoming_origin() {
    for (access_type, prefetched) in [
        (CacheAccessType::InstructionFetch, false),
        (CacheAccessType::InstructionPrefetch, true),
    ] {
        let hierarchy = TestHierarchy::<1>::new();
        access(&hierarchy, 0, 100, CacheAccessType::DataWrite);

        assert_eq!(
            access(&hierarchy, 0, 100, access_type),
            CacheHierarchyAccessResult::HitInOtherPrivateCache,
        );
        assert_eq!(instruction_line(&hierarchy, 0, 100).prefetched, prefetched);
    }
}

#[test]
fn a_store_invalidates_the_origin_and_a_later_fetch_starts_a_new_residency() {
    let hierarchy = TestHierarchy::<1>::new();
    access(&hierarchy, 0, 100, CacheAccessType::InstructionPrefetch);
    assert!(instruction_line(&hierarchy, 0, 100).prefetched);

    access(&hierarchy, 0, 100, CacheAccessType::DataWrite);
    {
        let instruction_request = request(0, 100, CacheAccessType::InstructionFetch);
        let set = hierarchy.private_caches.get_set_for_fill(&instruction_request);
        assert!(set.poke(100).is_none());
        assert!(set.lines.iter().all(|line| !line.is_valid() && !line.prefetched));
    }

    assert_eq!(
        access(&hierarchy, 0, 100, CacheAccessType::InstructionFetch),
        CacheHierarchyAccessResult::HitInOtherPrivateCache,
    );
    assert!(!instruction_line(&hierarchy, 0, 100).prefetched);
}

#[test]
fn data_prefetches_do_not_set_instruction_prefetch_origin() {
    for access_type in [CacheAccessType::PrefetchRead, CacheAccessType::PrefetchWrite] {
        let hierarchy = TestHierarchy::<1>::new();
        access(&hierarchy, 0, 100, access_type);

        let line = resident(&hierarchy, 0, 100, CacheAccessType::DataRead)
            .expect("data-prefetched block should be resident");
        assert!(!line.is_instruction());
        assert!(!line.prefetched);
    }
}

#[test]
fn another_cores_instruction_fill_does_not_inherit_the_suppliers_origin() {
    // Statistics and traffic arrays use the selected global configuration.
    if parameter::CORE_COUNT < 2 {
        return;
    }

    for (supplier_type, requester_type, expected_origin) in [
        (
            CacheAccessType::InstructionPrefetch,
            CacheAccessType::InstructionFetch,
            false,
        ),
        (
            CacheAccessType::InstructionFetch,
            CacheAccessType::InstructionPrefetch,
            true,
        ),
    ] {
        let hierarchy = TestHierarchy::<2>::new();
        access(&hierarchy, 0, 100, supplier_type);
        let supplier_origin = instruction_line(&hierarchy, 0, 100).prefetched;

        assert_eq!(
            access(&hierarchy, 1, 100, requester_type),
            CacheHierarchyAccessResult::HitInOtherPrivateCache,
        );
        assert_eq!(
            instruction_line(&hierarchy, 1, 100).prefetched,
            expected_origin,
        );
        assert_eq!(instruction_line(&hierarchy, 0, 100).prefetched, supplier_origin);
    }
}

#[test]
fn a_unified_cache_permission_upgrade_preserves_the_existing_fill_origin() {
    for (fill_type, prefetched) in [
        (CacheAccessType::InstructionFetch, false),
        (CacheAccessType::InstructionPrefetch, true),
    ] {
        let hierarchy = UnifiedTestHierarchy::new();
        let instruction_request = request(0, 100, fill_type);
        hierarchy.access_memory_pblock_id(&instruction_request, get_monotonic_ts());

        // A store upgrades the existing unified-cache residency. Its internal
        // invalidation/refill must not make this appear to be a new demand fill.
        let store_request = request(0, 100, CacheAccessType::DataWrite);
        assert_eq!(
            hierarchy
                .access_memory_pblock_id(&store_request, get_monotonic_ts())
                .0,
            CacheHierarchyAccessResult::MissDueToPermission,
        );
        let line = hierarchy
            .private_caches
            .get_set_for_fill(&store_request)
            .poke(100)
            .expect("upgraded unified-cache block should remain resident");
        assert!(line.has_write_permission());
        assert!(line.is_modified());
        assert_eq!(line.prefetched, prefetched);
    }
}
