use std::{collections::HashMap, sync::MutexGuard};

mod havard;
mod set_and_line;
mod unified;

pub use set_and_line::{PrivateCacheLine, PrivateCachePokeResult, PrivateCacheSet};

pub trait PrivateCaches {
    // This function is for creating all new private caches.
    fn new() -> Self;

    // This function is for checking the whether the private cache hits or miss.
    fn poke_and_update(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_instruction: bool,
        is_store: bool,
    ) -> PrivateCachePokeResult;

    // Find the next victim in the cache set.
    fn poke_victim(&self) -> Option<u64>;

    // This function is for refilling the cache line from the shared cache. Coherence refilling has its own way.
    fn refill_from_shared_cache(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_instruction: bool,
        modified: bool,
    ) -> Option<PrivateCacheLine>;

    // This function is for coherence messages and refill.
    // The rest of the function of coherence logic is handled outside.
    fn get_set_guard_by_sharer_list(
        &self,
        block_id: u64,
        sharers: SharerList,
    ) -> Vec<(usize, MutexGuard<'_, PrivateCacheSet>)>; // (sharer_index, guard)

    // This function is for debugging. It gets the ids of all cores that have the cache line.
    fn in_which_cores(&self, block_id: u64) -> Vec<u32>;

    // This function is for debugging. It gets the state of the cache line in all cores.
    fn query_replica_state(&self, block_id: u64) -> HashMap<u32, bool>; // (core_id, is_modified)

    // These functions are for maintaining the shared list in the directory.
    // For Harvard architecture, each core takes two bits.
    // For unified architecture, each core takes one bit.
    fn find_cache_by_id(id: usize) -> (u32, bool); // (core_id, is_instruction_cache)
    fn get_cache_id_by_cache_info(core_id: u32, is_instruction_cache: bool) -> usize;
}

pub use havard::HarvardPrivateCaches;
pub use unified::UnifiedPrivateCaches;

use super::directory::SharerList;
