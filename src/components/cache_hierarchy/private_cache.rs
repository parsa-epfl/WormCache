use std::collections::HashMap;
use std::ops::DerefMut;

mod havard;
mod set_and_line;
mod unified;

pub use set_and_line::{EvictedSlot, PrivateCacheLine, PrivateCachePokeResult, PrivateCacheSet};

pub trait PrivateCaches {
    // This function is for creating all new private caches.
    fn new() -> Self;

    // This function is for checking the whether the private cache hits or miss.
    fn poke_and_update(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        is_instruction: bool,
        is_store: bool,
    ) -> PrivateCachePokeResult;

    // This function is for filling the cache line from the shared LLC.
    fn get_set_for_fill(
        &self,
        core_id: u32,
        block_id: u64,
        is_instruction: bool,
    ) -> impl DerefMut<Target = PrivateCacheSet>;

    // This function is for coherence messages and refill.
    // The rest of the function of coherence logic is handled outside.
    fn get_set_guard_by_sharer_list(
        &self,
        block_id: u64,
        sharers: SharerList,
    ) -> Vec<(
        usize,
        impl DerefMut<Target = PrivateCacheSet>,
        Option<usize>,
    )>; // (sharer_index, guard, index)

    // This function is for debugging. It gets the ids of all cores that have the cache line.
    fn in_which_cores(&self, block_id: u64) -> Vec<u32>;

    // This function is for debugging. It gets the state of the cache line in all cores.
    fn query_replica_state(&self, block_id: u64) -> HashMap<u32, bool>; // (core_id, is_modified)

    // These functions are for maintaining the shared list in the directory.
    // For Harvard architecture, each core takes two bits.
    // For unified architecture, each core takes one bit.
    fn find_cache_info_by_cache_id(id: usize) -> (u32, bool); // (core_id, is_instruction_cache)
    fn get_cache_id_by_cache_info(core_id: u32, is_instruction_cache: bool) -> usize;

    // This function is for saving the snapshot of the private cache.
    fn dump_flexus_checkpoint(&self, snapshot_folder: &str);

    fn information() -> String;

    // This function is for printing diagnose information. It is used for debugging.
    fn print_debug_info(&self);

    const DIRECTORY_SET: usize;

    fn serialize(&self, name: &str, numa_node_id: usize);
    fn deserialize(&mut self, name: &str, numa_node_id: usize); // this is in-place deserialization.
}

pub use havard::ParallelHarvardPrivateCache;
pub use havard::SerialHarvardPrivateCache;

pub use unified::ParallelUnifiedPrivateCache;
pub use unified::SerialUnifiedPrivateCache;

use super::directory::SharerList;
