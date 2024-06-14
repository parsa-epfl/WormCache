use serde_json::json;

use crate::components::cache_hierarchy::util::CCell;

use super::{PrivateCachePokeResult, PrivateCacheSet, PrivateCaches};
use spin::mutex::SpinMutex;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ops::DerefMut;

#[repr(align(64))]
#[derive(Debug)]
pub struct UnifiedPerCorePrivateCache<
    G: CCell<PrivateCacheSet> + std::fmt::Debug,
    const SET: usize,
    const ASSO: usize,
> {
    cache: Box<[G; SET]>,
}

impl<G: CCell<PrivateCacheSet> + std::fmt::Debug, const SET: usize, const ASSO: usize>
    UnifiedPerCorePrivateCache<G, SET, ASSO>
{
    pub fn new() -> Self {
        Self {
            cache: crate::util::init_heap_array(|_| G::new(PrivateCacheSet::new(ASSO))),
        }
    }

    pub fn get_set(&self, block_id: u64) -> &G {
        let set_id = block_id as usize % SET;
        &self.cache[set_id]
    }
}

pub struct UnifiedPrivateCaches<
    G: CCell<PrivateCacheSet> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const SET: usize,
    const ASSO: usize,
> {
    caches: Box<[UnifiedPerCorePrivateCache<G, SET, ASSO>; CORE_COUNT]>,
}

impl<
        G: CCell<PrivateCacheSet> + std::fmt::Debug,
        const CORE_COUNT: usize,
        const SET: usize,
        const ASSO: usize,
    > PrivateCaches for UnifiedPrivateCaches<G, CORE_COUNT, SET, ASSO>
{
    const DIRECTORY_SET: usize = SET;

    fn new() -> Self {
        Self {
            caches: crate::util::init_heap_array(|_| UnifiedPerCorePrivateCache::new()),
        }
    }

    #[inline]
    fn poke_and_update(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        is_instruction: bool,
        is_store: bool,
    ) -> PrivateCachePokeResult {
        self.caches[core_id as usize]
            .get_set(block_id)
            .inner()
            .poke_and_update(block_id, ts, v_ts, is_store, is_instruction)
    }

    #[inline]
    fn get_set_guard_by_sharer_list(
        &self,
        block_id: u64,
        sharers: crate::components::cache_hierarchy::directory::SharerList,
    ) -> Vec<(
        usize,
        impl DerefMut<Target = PrivateCacheSet>,
        Option<usize>,
    )> {
        let mut result = Vec::new();

        for core_id in sharers.iter_ones() {
            let set = self.caches[core_id].get_set(block_id);
            let guard = set.inner();
            let index = guard.index_of(block_id);
            result.push((core_id, guard, index));
        }

        result
    }

    #[inline]
    fn in_which_cores(&self, block_id: u64) -> Vec<u32> {
        let mut result = Vec::new();
        for core_id in 0..CORE_COUNT {
            let set = self.caches[core_id].get_set(block_id);
            let guard = set.inner();
            if guard.poke(block_id).is_some() {
                result.push(core_id as u32);
            }
        }
        result
    }

    #[inline]
    fn query_replica_state(&self, block_id: u64) -> HashMap<u32, bool> {
        let mut res = HashMap::new();

        // If the i cache has the block, it is considered as shared. Just double check the d cache and make sure it is shared if exists.

        for core_id in 0..CORE_COUNT {
            let is_d = self.caches[core_id].cache[block_id as usize % SET]
                .inner()
                .poke(block_id);

            if let Some(d_line) = is_d {
                assert!(d_line.block_id() == block_id);
                res.insert(core_id as u32, d_line.is_modified());
            }
        }

        res
    }

    #[inline]
    fn find_cache_info_by_cache_id(index: usize) -> (u32, bool) {
        let core_id = index as u32;
        let is_instruction = false;
        (core_id, is_instruction)
    }

    #[inline]
    fn get_cache_id_by_cache_info(core_id: u32, _: bool) -> usize {
        core_id as usize
    }

    #[inline]
    fn dump_snapshot(&self, snapshot_folder: &str) {
        for core_id in 0..CORE_COUNT {
            let serialized_cache = self.caches[core_id]
                .cache
                .iter()
                .map(|set| set.inner().serialize(SET))
                .collect::<Vec<_>>();

            let private_cache_path = format!("{}/core_{}_private.json", snapshot_folder, core_id);
            std::fs::write(
                private_cache_path,
                serde_json::to_string_pretty(&json!(
                    {
                        "associativity": ASSO,
                        "tags": serialized_cache
                    }
                ))
                .unwrap(),
            )
            .unwrap();
        }
    }

    fn information() -> String {
        format!(
            "Type: UnifiedPrivateCache, Core Count: {}, Set: {}, Associativity: {}, Is Parallel: {}",
            { CORE_COUNT },
            SET,
            ASSO,
            G::support_parallel_access()
        )
    }

    #[inline]
    fn get_set_for_fill(
        &self,
        core_id: u32,
        block_id: u64,
        _: bool,
    ) -> impl DerefMut<Target = PrivateCacheSet> {
        self.caches[core_id as usize].get_set(block_id).inner()
    }
}

pub type ParallelUnifiedPrivateCache<const CORE_COUNT: usize, const SET: usize, const ASSO: usize> =
    UnifiedPrivateCaches<SpinMutex<PrivateCacheSet>, CORE_COUNT, SET, ASSO>;

pub type SerialUnifiedPrivateCache<const CORE_COUNT: usize, const SET: usize, const ASSO: usize> =
    UnifiedPrivateCaches<UnsafeCell<PrivateCacheSet>, CORE_COUNT, SET, ASSO>;
