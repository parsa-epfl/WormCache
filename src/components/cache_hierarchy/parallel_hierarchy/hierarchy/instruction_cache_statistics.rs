use crate::components::cache_hierarchy::CacheBlockRequest;
use crate::components::cache_hierarchy::common::{PrivateCacheEvictedSlot, PrivateCacheSet};
use crate::debug::statistics::{EventType, Statistics};

/// Account for actual installations and replacement evictions while the caller
/// holds the private-cache set lock. The underlying fill and its return value
/// retain their existing behavior.
#[inline]
pub(super) fn fill_private_cache_set(
    set: &mut PrivateCacheSet,
    request: &CacheBlockRequest,
    potential_slot: PrivateCacheEvictedSlot,
    ts: u64,
    writable: bool,
    modified: bool,
    prefetched: bool,
    demand_accessed: bool,
) -> Option<bool> {
    let new_instruction_residency = request.is_instruction()
        && !matches!(potential_slot, PrivateCacheEvictedSlot::Same(_));
    let unused_prefetched_victim = match &potential_slot {
        PrivateCacheEvictedSlot::Valid(index, _) => {
            let victim = &set.lines[*index];
            // prefetched records instruction-prefetch origin even if a data
            // access has since changed is_instruction in a unified cache.
            victim.is_valid() && victim.prefetched && !victim.demand_accessed
        }
        _ => false,
    };

    let evicted = set.fill_with_potential_eviction_slot(
        potential_slot,
        request.block_id,
        ts,
        request.is_instruction(),
        writable,
        modified,
        prefetched,
        demand_accessed,
    );

    // An intervening invalidation can let the fill use an invalid slot instead
    // of the proposed victim. Only Some means a live residency was replaced.
    if evicted.is_some() && unused_prefetched_victim {
        Statistics::global_record(
            request.core_id,
            EventType::InstructionCacheOverfetches,
            request.is_os,
        );
    }
    if new_instruction_residency {
        Statistics::global_record(
            request.core_id,
            EventType::InstructionCacheFills,
            request.is_os,
        );
    }

    evicted
}
