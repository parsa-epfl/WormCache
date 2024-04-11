use serde_json::json;

use super::{PrivateCacheLine, PrivateCachePokeResult, PrivateCacheSet, PrivateCaches};
use std::{collections::HashMap, sync::Mutex};

#[repr(align(64))]
#[derive(Debug)]
pub struct UnifiedPerCorePrivateCache<const SET: usize, const ASSO: usize> {
    cache: Box<[Mutex<PrivateCacheSet>; SET]>,
}

impl<const SET: usize, const ASSO: usize> UnifiedPerCorePrivateCache<SET, ASSO> {
    pub fn new() -> Self {
        Self {
            cache: crate::util::init_heap_array(|_| Mutex::new(PrivateCacheSet::new(ASSO))),
        }
    }

    pub fn get_set(&self, block_id: u64) -> &Mutex<PrivateCacheSet> {
        let set_id = block_id as usize % SET;
        return &self.cache[set_id];
    }

    // pub fn contains_block(&self, block_id: u64) -> bool {
    //     let set_id = block_id as usize % SET;
    //     let set = self.cache[set_id].lock().unwrap();
    //     return set.poke(block_id).is_some();
    // }

    // pub fn is_block_modified(&self, block_id: u64) -> bool {
    //     let set_id = block_id as usize % SET;
    //     let set = self.cache[set_id].lock().unwrap();
    //     let line = set.poke(block_id);
    //     if let Some(line) = line {
    //         return line.is_modified();
    //     } else {
    //         return false;
    //     }
    // }
}

pub struct UnifiedPrivateCaches<const CORE_COUNT: usize, const SET: usize, const ASSO: usize> {
    caches: Box<[UnifiedPerCorePrivateCache<SET, ASSO>; CORE_COUNT]>,
}

impl<const CORE_COUNT: usize, const SET: usize, const ASSO: usize> PrivateCaches
    for UnifiedPrivateCaches<CORE_COUNT, SET, ASSO>
{
    fn new() -> Self {
        return Self {
            caches: crate::util::init_heap_array(|_| UnifiedPerCorePrivateCache::new()),
        };
    }

    #[inline]
    fn poke_and_update(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_instruction: bool,
        is_store: bool,
    ) -> PrivateCachePokeResult {
        self.caches[core_id as usize]
            .get_set(block_id)
            .lock()
            .unwrap()
            .poke_and_update(block_id, ts, is_store, is_instruction)
    }

    #[inline]
    fn poke_victim(&self) -> Option<u64> {
        None
    }

    #[inline]
    fn refill_from_shared_cache(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_instruction: bool,
        modified: bool,
    ) -> Option<PrivateCacheLine> {
        self.caches[core_id as usize]
            .get_set(block_id)
            .lock()
            .unwrap()
            .refill(block_id, ts, is_instruction, modified)
    }

    #[inline]
    fn get_set_guard_by_sharer_list(
        &self,
        block_id: u64,
        sharers: crate::components::cache_hierarchy::directory::SharerList,
    ) -> Vec<(usize, std::sync::MutexGuard<'_, PrivateCacheSet>)> {
        let mut result = Vec::new();

        for core_id in sharers.iter_ones() {
            let set = self.caches[core_id].get_set(block_id);
            let guard = set.lock().unwrap();
            result.push((core_id, guard));
        }

        return result;
    }

    #[inline]
    fn in_which_cores(&self, block_id: u64) -> Vec<u32> {
        let mut result = Vec::new();
        for core_id in 0..CORE_COUNT {
            let set = self.caches[core_id].get_set(block_id);
            let guard = set.lock().unwrap();
            if guard.poke(block_id).is_some() {
                result.push(core_id as u32);
            }
        }
        return result;
    }

    #[inline]
    fn query_replica_state(&self, block_id: u64) -> HashMap<u32, bool> {
        let mut res = HashMap::new();

        // If the i cache has the block, it is considered as shared. Just double check the d cache and make sure it is shared if exists.

        for core_id in 0..CORE_COUNT {
            let is_d = self.caches[core_id].cache[block_id as usize % SET]
                .lock()
                .unwrap()
                .poke(block_id);

            if let Some(d_line) = is_d {
                assert!(d_line.block_id() == block_id);
                res.insert(core_id as u32, d_line.is_modified());
            }
        }

        res
    }

    #[inline]
    fn find_cache_by_id(index: usize) -> (u32, bool) {
        let core_id = index as u32;
        let is_instruction = false;
        return (core_id, is_instruction);
    }

    #[inline]
    fn get_cache_id_by_cache_info(core_id: u32, _: bool) -> usize {
        return core_id as usize;
    }

    #[inline]
    fn dump_snapshot(&self, snapshot_folder: &str) {
        for core_id in 0..CORE_COUNT {
            let serialized_cache = self.caches[core_id].cache.iter().map(|set| {
                set.lock().unwrap().serialize(SET)
            }).collect::<Vec<_>>();

            let private_cache_path = format!("{}/core_{}_private.json", snapshot_folder, core_id);
            std::fs::write(private_cache_path, serde_json::to_string_pretty(&json!(
                {
                    "associativity": ASSO,
                    "tags": serialized_cache
                }
            )).unwrap()).unwrap();
        }
    }
}
