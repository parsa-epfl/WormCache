use std::cell::RefCell;
use std::collections::VecDeque;

use super::common::{
    CacheAccessType, CacheHierarchyAccessResult, InfiniteDirectory, InstructionPrefetchTrigger,
    ParallelHarvardPrivateCache, ParallelLRUSharedCache, statistics::ZeroSharedCacheSetStatistics,
};
use super::hierarchy::ParallelMemoryHierarchy;
use super::mmu::{MMUFlushMode, MMUTranslationResult, NoMMU};
use super::{CacheBlockRequest, MemoryAccessRequest, MemoryHierarchy};
use crate::parameter;
use crate::util::get_monotonic_ts;

/// Records physical requests while providing an independently chosen demand result.
struct RecordingHierarchy {
    accesses: RefCell<Vec<(CacheBlockRequest, u64)>>,
    demand_results: RefCell<VecDeque<CacheHierarchyAccessResult>>,
}

impl RecordingHierarchy {
    fn new(results: Vec<CacheHierarchyAccessResult>) -> Self {
        Self {
            accesses: RefCell::new(Vec::new()),
            demand_results: RefCell::new(results.into()),
        }
    }
}

impl MemoryHierarchy for RecordingHierarchy {
    fn access_memory_pblock_id(
        &self,
        request: &CacheBlockRequest,
        ts: u64,
    ) -> (CacheHierarchyAccessResult, (usize, usize, usize)) {
        self.accesses.borrow_mut().push((request.clone(), ts));
        let result = if request.access_type == CacheAccessType::InstructionFetch {
            self.demand_results.borrow_mut().pop_front().unwrap()
        } else {
            CacheHierarchyAccessResult::Miss
        };
        (result, (0, 0, 0))
    }

    fn translate(&self, request: &MemoryAccessRequest, _ts: u64) -> MMUTranslationResult {
        MMUTranslationResult::Hit(request.va, 0)
    }

    fn access_from_device_with_pa(
        &self,
        _paddr: u64,
        _access_type: CacheAccessType,
        _ts: u64,
    ) -> CacheHierarchyAccessResult {
        unreachable!()
    }

    fn flush_mmu(&self, _core_id: u32, _info: MMUFlushMode) {}
    fn prefetch_blocks_sms(&self, _request: &CacheBlockRequest, _ts: u64) {}
    fn prefetch_blocks_stride(&self, _request: &CacheBlockRequest, _ts: u64) {}
    fn record_access(&self, _request: &CacheBlockRequest, _ts: u64) {}
    fn evict_sms(&self, _core_id: u32, _block_id: u64) {}
    fn serialize(&self, _name: &str, _numa_node_id: usize) {}
    fn deserialize(&mut self, _name: &str, _numa_node_id: usize) {}
}

fn instruction_request(block_id: u64) -> CacheBlockRequest {
    CacheBlockRequest {
        core_id: 0,
        block_id,
        access_type: CacheAccessType::InstructionFetch,
        is_os: true,
        pc: 0x1234,
    }
}

fn classified_results() -> [(CacheHierarchyAccessResult, bool); 7] {
    [
        (CacheHierarchyAccessResult::Miss, true),
        (CacheHierarchyAccessResult::HitInSharedCache, true),
        (CacheHierarchyAccessResult::HitInOtherPrivateCache, true),
        (CacheHierarchyAccessResult::MissDueToPermission, true),
        (CacheHierarchyAccessResult::HitInSelfPrivateCache, false),
        (CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache, false),
        // Unknown describes ambiguous lower-level history after a known L1 miss.
        (CacheHierarchyAccessResult::Unknown, true),
    ]
}

#[test]
fn trigger_distinguishes_private_cache_misses_from_hits() {
    for (result, is_miss) in classified_results() {
        assert!(InstructionPrefetchTrigger::Access.should_trigger(&result));
        assert_eq!(InstructionPrefetchTrigger::Miss.should_trigger(&result), is_miss);
        assert_eq!(
            InstructionPrefetchTrigger::MissOrPrefetchHit.should_trigger(&result),
            is_miss || matches!(result, CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache),
        );
    }
}

#[test]
fn access_trigger_prefetches_zero_one_or_many_lines_and_preserves_context() {
    let mut request = instruction_request(100);
    request.core_id = 7;
    for count in [0, 1, 2, 4] {
        let hierarchy = RecordingHierarchy::new(Vec::new());
        hierarchy.prefetch_adjacent_instruction_lines(
            &request,
            &CacheHierarchyAccessResult::HitInSelfPrivateCache,
            42,
            count,
            InstructionPrefetchTrigger::Access,
        );
        let accesses = hierarchy.accesses.borrow();
        assert_eq!(accesses.len(), count);
        for (offset, (prefetch, ts)) in accesses.iter().enumerate() {
            assert_eq!(prefetch.block_id, 101 + offset as u64);
            assert_eq!(prefetch.access_type, CacheAccessType::InstructionPrefetch);
            assert_eq!(prefetch.core_id, request.core_id);
            assert_eq!(prefetch.is_os, request.is_os);
            assert_eq!(prefetch.pc, request.pc);
            assert_eq!(*ts, 42);
        }
        assert_eq!(request.block_id, 100);
        assert_eq!(request.access_type, CacheAccessType::InstructionFetch);
    }
}

#[test]
fn miss_trigger_issues_requests_for_every_private_cache_miss_result() {
    for (result, is_miss) in classified_results() {
        let hierarchy = RecordingHierarchy::new(Vec::new());
        hierarchy.prefetch_adjacent_instruction_lines(
            &instruction_request(100),
            &result,
            42,
            3,
            InstructionPrefetchTrigger::Miss,
        );
        let accesses = hierarchy.accesses.borrow();
        assert_eq!(accesses.len(), if is_miss { 3 } else { 0 });
        for (offset, (prefetch, _)) in accesses.iter().enumerate() {
            assert_eq!(prefetch.block_id, 101 + offset as u64);
        }
    }
}

#[test]
fn combined_trigger_issues_requests_for_misses_and_first_prefetch_hits_only() {
    for (result, is_miss) in classified_results() {
        let hierarchy = RecordingHierarchy::new(Vec::new());
        hierarchy.prefetch_adjacent_instruction_lines(
            &instruction_request(100),
            &result,
            42,
            3,
            InstructionPrefetchTrigger::MissOrPrefetchHit,
        );
        let should_trigger = is_miss
            || matches!(result, CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache);
        let accesses = hierarchy.accesses.borrow();
        assert_eq!(accesses.len(), if should_trigger { 3 } else { 0 });
        for (offset, (prefetch, _)) in accesses.iter().enumerate() {
            assert_eq!(prefetch.block_id, 101 + offset as u64);
        }
    }
}

#[test]
fn data_page_walk_and_prefetch_accesses_never_start_instruction_prefetches() {
    for trigger in [
        InstructionPrefetchTrigger::Access,
        InstructionPrefetchTrigger::Miss,
        InstructionPrefetchTrigger::MissOrPrefetchHit,
    ] {
        for access_type in [
            CacheAccessType::DataRead,
            CacheAccessType::DataWrite,
            CacheAccessType::PageWalkRead,
            CacheAccessType::InstructionPrefetch,
            CacheAccessType::PrefetchRead,
            CacheAccessType::PrefetchWrite,
        ] {
            let hierarchy = RecordingHierarchy::new(Vec::new());
            let mut request = instruction_request(100);
            request.access_type = access_type;
            hierarchy.prefetch_adjacent_instruction_lines(
                &request,
                &CacheHierarchyAccessResult::Miss,
                42,
                3,
                trigger,
            );
            assert!(hierarchy.accesses.borrow().is_empty());
        }
    }
}

#[test]
fn prefetching_stops_at_the_last_representable_physical_line() {
    let max_block = u64::MAX >> parameter::CACHE_LINE_SIZE.trailing_zeros();
    for trigger in [
        InstructionPrefetchTrigger::Access,
        InstructionPrefetchTrigger::Miss,
        InstructionPrefetchTrigger::MissOrPrefetchHit,
    ] {
        let hierarchy = RecordingHierarchy::new(Vec::new());
        hierarchy.prefetch_adjacent_instruction_lines(
            &instruction_request(max_block - 2),
            &CacheHierarchyAccessResult::Miss,
            42,
            4,
            trigger,
        );
        let accesses = hierarchy.accesses.borrow();
        assert_eq!(accesses.len(), 2);
        assert_eq!(accesses[0].0.block_id, max_block - 1);
        assert_eq!(accesses[1].0.block_id, max_block);

        for block_id in [max_block, u64::MAX] {
            let hierarchy = RecordingHierarchy::new(Vec::new());
            hierarchy.prefetch_adjacent_instruction_lines(
                &instruction_request(block_id),
                &CacheHierarchyAccessResult::Miss,
                42,
                4,
                trigger,
            );
            assert!(hierarchy.accesses.borrow().is_empty());
        }
    }
}

#[test]
fn virtual_access_uses_configured_trigger_and_count_after_the_demand_completes() {
    for ((result, _), (demand_result, _)) in
        classified_results().into_iter().zip(classified_results())
    {
        let expected_count = if parameter::ADJACENT_LINE_PREFETCHING
            && parameter::ADJACENT_LINE_PREFETCH_TRIGGER.should_trigger(&result)
        {
            parameter::ADJACENT_LINE_PREFETCH_COUNT
        } else {
            0
        };
        let hierarchy = RecordingHierarchy::new(vec![demand_result]);
        let request = MemoryAccessRequest {
            core_id: 0,
            va: 0x1000,
            access_type: CacheAccessType::InstructionFetch,
            is_os: false,
            pc: 0x8000,
        };
        let physical_address = 100 * parameter::CACHE_LINE_SIZE as u64;
        let returned = hierarchy.access_memory_with_va_and_pa(&request, Some(physical_address), 42);
        let expected_result = if matches!(result, CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache) {
            CacheHierarchyAccessResult::HitInSelfPrivateCache
        } else {
            result
        };
        assert_eq!(returned, expected_result);

        let accesses = hierarchy.accesses.borrow();
        assert_eq!(accesses.len(), 1 + expected_count);
        assert_eq!(accesses[0].0.block_id, 100);
        assert_eq!(accesses[0].0.access_type, CacheAccessType::InstructionFetch);
        for (offset, (prefetch, ts)) in accesses.iter().skip(1).enumerate() {
            assert_eq!(prefetch.block_id, 101 + offset as u64);
            assert_eq!(prefetch.access_type, CacheAccessType::InstructionPrefetch);
            assert_eq!(*ts, 42);
        }
        assert!(hierarchy.demand_results.borrow().is_empty());
    }
}

// Tiny, independent caches exercise the real synchronous fill/coherence path.
type TinyHierarchy = ParallelMemoryHierarchy<
    NoMMU,
    ParallelHarvardPrivateCache<1, 16, 2, 16, 2>,
    ParallelLRUSharedCache<ZeroSharedCacheSetStatistics, 16, 4, false>,
    InfiniteDirectory<16>,
    true,
    true,
    true,
    false,
    1,
    4,
    4,
    4,
    2,
    32,
    false,
    false,
    false,
    false,
    1,
    4,
    16,
    1,
>;

fn demand_and_prefetch(
    hierarchy: &impl MemoryHierarchy,
    request: &CacheBlockRequest,
    trigger: InstructionPrefetchTrigger,
) -> CacheHierarchyAccessResult {
    let ts = get_monotonic_ts();
    let (result, _) = hierarchy.access_memory_pblock_id_with_prefetch_event(request, ts);
    hierarchy.prefetch_adjacent_instruction_lines(request, &result, ts, 1, trigger);
    result
}

#[test]
fn cold_instruction_miss_installs_lines_and_first_prefetch_hit_obeys_the_trigger() {
    for trigger in [
        InstructionPrefetchTrigger::Access,
        InstructionPrefetchTrigger::Miss,
        InstructionPrefetchTrigger::MissOrPrefetchHit,
    ] {
        let hierarchy = TinyHierarchy::new();
        assert_eq!(
            demand_and_prefetch(&hierarchy, &instruction_request(100), trigger),
            CacheHierarchyAccessResult::Miss,
        );
        // Returning from the call means the demand line is already installed.
        assert_eq!(
            hierarchy.access_memory_pblock_id(&instruction_request(100), get_monotonic_ts()).0,
            CacheHierarchyAccessResult::HitInSelfPrivateCache,
        );
        // The prefetched line is also resident before the next demand executes.
        assert_eq!(
            demand_and_prefetch(&hierarchy, &instruction_request(101), trigger),
            CacheHierarchyAccessResult::PrefetchHitInSelfPrivateCache,
        );
        // The signal is consumed by the first demand, never by later hits.
        assert_eq!(
            hierarchy.access_memory_pblock_id(&instruction_request(101), get_monotonic_ts()).0,
            CacheHierarchyAccessResult::HitInSelfPrivateCache,
        );
        assert_eq!(
            hierarchy.access_memory_pblock_id(&instruction_request(102), get_monotonic_ts()).0,
            match trigger {
                InstructionPrefetchTrigger::Access | InstructionPrefetchTrigger::MissOrPrefetchHit =>
                    CacheHierarchyAccessResult::HitInSelfPrivateCache,
                InstructionPrefetchTrigger::Miss => CacheHierarchyAccessResult::Miss,
            },
        );
    }
}

#[test]
fn instruction_miss_satisfied_by_a_private_data_cache_still_triggers_miss_prefetching() {
    let hierarchy = TinyHierarchy::new();
    let mut peer_request = instruction_request(100);
    // The same-core L1D is another private cache relative to the requesting L1I.
    // This also keeps the test valid under one-core configuration templates.
    peer_request.access_type = CacheAccessType::DataRead;
    assert_eq!(
        hierarchy.access_memory_pblock_id(&peer_request, get_monotonic_ts()).0,
        CacheHierarchyAccessResult::Miss,
    );
    assert_eq!(
        demand_and_prefetch(&hierarchy, &instruction_request(100), InstructionPrefetchTrigger::Miss),
        CacheHierarchyAccessResult::HitInOtherPrivateCache,
    );
    assert_eq!(
        hierarchy.access_memory_pblock_id(&instruction_request(101), get_monotonic_ts()).0,
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
}

#[test]
fn instruction_miss_satisfied_by_the_llc_still_triggers_miss_prefetching() {
    let hierarchy = TinyHierarchy::new();
    // Three conflicting lines evict block 100 from the two-way L1I while keeping
    // it in the four-way inclusive LLC.
    for block_id in [100, 116, 132] {
        assert_eq!(
            hierarchy.access_memory_pblock_id(&instruction_request(block_id), get_monotonic_ts()).0,
            CacheHierarchyAccessResult::Miss,
        );
    }
    assert_eq!(
        demand_and_prefetch(&hierarchy, &instruction_request(100), InstructionPrefetchTrigger::Miss),
        CacheHierarchyAccessResult::HitInSharedCache,
    );
    assert_eq!(
        hierarchy.access_memory_pblock_id(&instruction_request(101), get_monotonic_ts()).0,
        CacheHierarchyAccessResult::HitInSelfPrivateCache,
    );
}
