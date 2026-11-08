use crate::components::cache_hierarchy::common::{
    statistics::ZeroSharedCacheSetStatistics, CacheAccessType, CacheHierarchyAccessResult,
    Directory, DirectorySet, InfiniteDirectory, InstructionPrefetchTrigger,
    ParallelHarvardPrivateCache, ParallelLRUSharedCache, PrivateCache, PrivateCacheLine,
};
use crate::components::cache_hierarchy::mmu::NoMMU;
use crate::components::cache_hierarchy::{CacheBlockRequest, MemoryAccessRequest, MemoryHierarchy};
use crate::parameter;

use super::ParallelMemoryHierarchy;

type TestHierarchy = ParallelMemoryHierarchy<
    NoMMU,
    ParallelHarvardPrivateCache<1, 1, 4, 1, 2>,
    ParallelLRUSharedCache<ZeroSharedCacheSetStatistics, 4, 4, false>,
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

fn request(block_id: u64, access_type: CacheAccessType) -> CacheBlockRequest {
    CacheBlockRequest {
        core_id: 0,
        block_id,
        access_type,
        is_os: false,
        pc: 0x1000,
    }
}

fn resident(hierarchy: &TestHierarchy, block_id: u64) -> Option<PrivateCacheLine> {
    hierarchy
        .private_caches
        .get_set_for_fill(&request(block_id, CacheAccessType::InstructionFetch))
        .poke(block_id)
}

fn ordinary_result(result: CacheHierarchyAccessResult) -> CacheHierarchyAccessResult {
    match result {
        CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache => {
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        }
        result => result,
    }
}

// Read-only snapshots also compare replacement timestamps, hit counters,
// permissions, and directory/LLC bookkeeping, rather than only resident tags.
fn assert_same_cache_state(left: &TestHierarchy, right: &TestHierarchy) {
    for access_type in [CacheAccessType::InstructionFetch, CacheAccessType::DataRead] {
        let req = request(0, access_type);
        let left_set = (*left.private_caches.get_set_for_fill(&req)).clone();
        let right_set = (*right.private_caches.get_set_for_fill(&req)).clone();
        assert_eq!(left_set, right_set);
    }
    assert_eq!(
        left.shared_cache.to_checkpoint_helper(),
        right.shared_cache.to_checkpoint_helper(),
    );
    for set_id in 0..4 {
        assert_eq!(
            left.directory.fetch_one_entry(set_id).raw(),
            right.directory.fetch_one_entry(set_id).raw(),
        );
    }
}

#[test]
fn original_physical_access_returns_an_ordinary_hit_on_first_prefetch_use() {
    let hierarchy = TestHierarchy::new();
    hierarchy.access_memory_pblock_id(&request(100, CacheAccessType::InstructionPrefetch), 1);
    let before = resident(&hierarchy, 100).unwrap();
    assert!(before.prefetched && !before.demand_accessed);

    let (result, _) =
        hierarchy.access_memory_pblock_id(&request(100, CacheAccessType::InstructionFetch), 2);
    assert_eq!(result, CacheHierarchyAccessResult::HitInSelfPrivateCache);
    let after = resident(&hierarchy, 100).unwrap();
    assert!(after.prefetched && after.demand_accessed);
    assert_eq!(after.ts, 2);
    // The original physical API does not implicitly start adjacent prefetches.
    assert!(resident(&hierarchy, 101).is_none());

    let (result, _) = hierarchy.access_memory_pblock_id_with_prefetch_event(
        &request(100, CacheAccessType::InstructionFetch),
        3,
    );
    assert_eq!(result, CacheHierarchyAccessResult::HitInSelfPrivateCache);
}

#[test]
fn detailed_access_reports_first_use_once_without_changing_cache_effects() {
    let ordinary = TestHierarchy::new();
    let detailed = TestHierarchy::new();
    let accesses = [
        (CacheAccessType::InstructionPrefetch, CacheHierarchyAccessResult::Miss),
        (
            CacheAccessType::InstructionPrefetch,
            CacheHierarchyAccessResult::HitInSelfPrivateCache,
        ),
        (
            CacheAccessType::InstructionFetch,
            CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
        ),
        (
            CacheAccessType::InstructionPrefetch,
            CacheHierarchyAccessResult::HitInSelfPrivateCache,
        ),
        (
            CacheAccessType::InstructionFetch,
            CacheHierarchyAccessResult::HitInSelfPrivateCache,
        ),
    ];

    for (index, (access_type, expected_event)) in accesses.into_iter().enumerate() {
        let req = request(100, access_type);
        let ts = index as u64 + 1;
        let (ordinary_hit, ordinary_stats) = ordinary.access_memory_pblock_id(&req, ts);
        let (event, detailed_stats) =
            detailed.access_memory_pblock_id_with_prefetch_event(&req, ts);
        assert_eq!(event, expected_event);
        assert_eq!(ordinary_hit, ordinary_result(event));
        assert_eq!(ordinary_stats, detailed_stats);
        assert_same_cache_state(&ordinary, &detailed);
    }
}

#[test]
fn original_virtual_access_wrappers_keep_the_ordinary_hit_result() {
    // The highest physical line has no legal successor. This keeps the check
    // independent of the globally selected adjacent-line count and policy.
    let offset_bits = parameter::CACHE_LINE_SIZE.trailing_zeros();
    let block_id = u64::MAX >> offset_bits;
    for explicit_pa in [false, true] {
        let hierarchy = TestHierarchy::new();
        hierarchy.access_memory_pblock_id(
            &request(block_id, CacheAccessType::InstructionPrefetch),
            1,
        );
        let req = MemoryAccessRequest {
            core_id: 0,
            va: block_id << offset_bits,
            access_type: CacheAccessType::InstructionFetch,
            is_os: false,
            pc: 0x1000,
        };
        let result = if explicit_pa {
            hierarchy.access_memory_with_va_and_pa(&req, Some(req.va), 2)
        } else {
            hierarchy.access_memory_with_va(&req, 2)
        };
        assert_eq!(result, CacheHierarchyAccessResult::HitInSelfPrivateCache);
        let line = resident(&hierarchy, block_id).unwrap();
        assert!(line.prefetched && line.demand_accessed);
        assert_eq!(line.ts, 2);
    }
}

#[test]
fn existing_access_and_miss_policies_retain_cache_and_coherence_behavior() {
    for trigger in [InstructionPrefetchTrigger::Access, InstructionPrefetchTrigger::Miss] {
        let ordinary = TestHierarchy::new();
        let detailed = TestHierarchy::new();
        // Include first-prefetch consumption, repeat hits, read sharing,
        // write invalidation, an owner downgrade, and instruction replacement.
        let accesses = [
            (100, CacheAccessType::InstructionFetch),
            (101, CacheAccessType::InstructionFetch),
            (101, CacheAccessType::InstructionFetch),
            (100, CacheAccessType::DataRead),
            (100, CacheAccessType::DataWrite),
            (100, CacheAccessType::InstructionPrefetch),
            (100, CacheAccessType::InstructionFetch),
            (104, CacheAccessType::InstructionFetch),
            (108, CacheAccessType::InstructionFetch),
            (100, CacheAccessType::InstructionFetch),
        ];
        for (index, (block_id, access_type)) in accesses.into_iter().enumerate() {
            let req = request(block_id, access_type);
            let ts = index as u64 + 1;
            let (original_result, original_stats) = ordinary.access_memory_pblock_id(&req, ts);
            let (event, detailed_stats) =
                detailed.access_memory_pblock_id_with_prefetch_event(&req, ts);
            assert_eq!(trigger.should_trigger(&original_result), trigger.should_trigger(&event));
            ordinary.prefetch_adjacent_instruction_lines(&req, &original_result, ts, 2, trigger);
            detailed.prefetch_adjacent_instruction_lines(&req, &event, ts, 2, trigger);
            assert_eq!(original_result, ordinary_result(event));
            assert_eq!(original_stats, detailed_stats);
            assert_same_cache_state(&ordinary, &detailed);
        }
    }
}
