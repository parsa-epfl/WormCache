use super::PrivateCaches;
use super::{PrivateCacheLine, PrivateCachePokeResult, PrivateCacheSet};
use std::collections::HashMap;
use std::sync::Mutex;

#[repr(align(64))]
#[derive(Debug)]
pub struct HarvardPerCorePrivateCache<
    const I_SET: usize,
    const I_ASSO: usize,
    const D_SET: usize,
    const D_ASSO: usize,
> {
    i_cache: Box<[Mutex<PrivateCacheSet>; I_SET]>,
    d_cache: Box<[Mutex<PrivateCacheSet>; D_SET]>,
}

impl<const I_SET: usize, const I_ASSO: usize, const D_SET: usize, const D_ASSO: usize>
    HarvardPerCorePrivateCache<I_SET, I_ASSO, D_SET, D_ASSO>
{
    pub fn new() -> Self {
        Self {
            i_cache: crate::util::init_heap_array(|_| Mutex::new(PrivateCacheSet::new(I_ASSO))),
            d_cache: crate::util::init_heap_array(|_| Mutex::new(PrivateCacheSet::new(D_ASSO))),
        }
    }
}

pub struct HarvardPrivateCaches<
    const CORE_COUNT: usize,
    const I_SET: usize,
    const I_ASSO: usize,
    const D_SET: usize,
    const D_ASSO: usize,
> {
    caches: Box<[HarvardPerCorePrivateCache<I_SET, I_ASSO, D_SET, D_ASSO>; CORE_COUNT]>,
}

impl<
        const CORE_COUNT: usize,
        const I_SET: usize,
        const I_ASSO: usize,
        const D_SET: usize,
        const D_ASSO: usize,
    > PrivateCaches for HarvardPrivateCaches<CORE_COUNT, I_SET, I_ASSO, D_SET, D_ASSO>
{
    fn new() -> Self {
        return Self {
            caches: crate::util::init_heap_array(|_| HarvardPerCorePrivateCache::new()),
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
        if is_instruction {
            self.caches[core_id as usize].i_cache[block_id as usize % I_SET]
                .lock()
                .unwrap()
                .poke_and_update(block_id, ts, is_store, is_instruction)
        } else {
            self.caches[core_id as usize].d_cache[block_id as usize % D_SET]
                .lock()
                .unwrap()
                .poke_and_update(block_id, ts, is_store, is_instruction)
        }
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
        if is_instruction {
            let mut set = self.caches[core_id as usize].i_cache[block_id as usize % I_SET]
                .lock()
                .unwrap();
            set.refill(block_id, ts, true, modified)
        } else {
            let mut set = self.caches[core_id as usize].d_cache[block_id as usize % D_SET]
                .lock()
                .unwrap();
            set.refill(block_id, ts, false, modified)
        }
    }

    #[inline]
    fn get_set_guard_by_sharer_list(
        &self,
        block_id: u64,
        sharers: crate::components::cache_hierarchy::directory::SharerList,
    ) -> Vec<(usize, std::sync::MutexGuard<'_, PrivateCacheSet>)> {
        // Now it really depends on how to interpret the sharer list.
        assert_eq!(sharers.len(), usize::max(CORE_COUNT * 2, 64));

        let mut res = Vec::new();

        for sharer_index in sharers.iter_ones() {
            let core_id = sharer_index / 2;
            let is_instruction = sharer_index % 2 == 0;

            if is_instruction {
                let set = &self.caches[core_id as usize].i_cache[block_id as usize % I_SET];
                let guard = set.lock().unwrap();
                res.push((sharer_index, guard));
            } else {
                let set = &self.caches[core_id as usize].d_cache[block_id as usize % D_SET];
                let guard = set.lock().unwrap();
                res.push((sharer_index, guard));
            }
        }

        res
    }

    #[inline]
    fn in_which_cores(&self, block_id: u64) -> Vec<u32> {
        let mut res: Vec<u32> = Vec::new();

        for core_id in 0..CORE_COUNT {
            if self.caches[core_id].i_cache[block_id as usize % I_SET]
                .lock()
                .unwrap()
                .poke(block_id)
                .is_some()
            {
                res.push(core_id as u32);
            } else if self.caches[core_id].d_cache[block_id as usize % D_SET]
                .lock()
                .unwrap()
                .poke(block_id)
                .is_some()
            {
                res.push(core_id as u32);
            }
        }

        res
    }

    #[inline]
    fn query_replica_state(&self, block_id: u64) -> HashMap<u32, bool> {
        let mut res = HashMap::new();

        // If the i cache has the block, it is considered as shared. Just double check the d cache and make sure it is shared if exists.

        for core_id in 0..CORE_COUNT {
            let is_i = self.caches[core_id].i_cache[block_id as usize % I_SET]
                .lock()
                .unwrap()
                .poke(block_id);

            let is_d = self.caches[core_id].d_cache[block_id as usize % D_SET]
                .lock()
                .unwrap()
                .poke(block_id);

            if let Some(i_line) = is_i {
                assert!(i_line.block_id() == block_id);
                assert!(i_line.is_instruction());
                assert!(i_line.is_modified() == false);

                // alright, we check the data cache and make sure there is no modified copy.
                if let Some(d_line) = is_d {
                    assert!(d_line.block_id() == block_id);
                    assert!(!d_line.is_instruction());
                    assert!(!d_line.is_modified());
                }

                res.insert(core_id as u32, false);
            } else {
                if let Some(d_line) = is_d {
                    assert!(d_line.block_id() == block_id);
                    assert!(!d_line.is_instruction());
                    res.insert(core_id as u32, d_line.is_modified());

                    continue;
                }
            }
        }

        res
    }
    #[inline]
    fn find_cache_by_id(index: usize) -> (u32, bool) {
        let core_id = index / 2;
        let is_instruction_cache = index % 2 == 0;
        (core_id as u32, is_instruction_cache)
    }
    #[inline]
    fn get_cache_id_by_cache_info(core_id: u32, is_instruction_cache: bool) -> usize {
        core_id as usize * 2 + if is_instruction_cache { 0 } else { 1 }
    }
}
