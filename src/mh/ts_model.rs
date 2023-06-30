use std::collections::{BinaryHeap, HashMap};

use super::mtr::MemoryTimestampRecordCollection;
use crate::cache::TimestampCache;
use crate::checkpoint::ts_checkpoint::{LRUPrioritizing, TsCacheBlock};
use crate::checkpoint::{CacheBlock, CacheBlockState, MemoryHierarchyCheckPoint, SerializedCache};
use crate::plugin::{PerInstructionInstrumentation, QEMUPluginPerCoreActor};
use crate::QEMUPlugin;

use std::sync::mpsc::Receiver;

// This file builds a memory hierarchy model using Cache recording timestamp.
// TODO: Add the traffic from the page walker and the prefetcher.

pub struct TimestampSingleCoreMemoryHierarchy<
    const P_A: usize, // associativity of the private cache
    const P_S: usize, // set number of the private cache
    const S_A: usize, // associativity of the shared cache
    const S_S: usize, // set number of the shared cache
> {
    pub private_cache: TimestampCache<P_A, P_S>,
    pub local_shared_cache: TimestampCache<S_A, S_S>,
}

impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    pub fn new() -> Self {
        return Self {
            private_cache: TimestampCache::new(),
            local_shared_cache: TimestampCache::new(),
        };
    }

    pub fn access_memory(&mut self, ts: usize, paddr: usize, is_instruction: bool, is_store: bool) {
        let block_id = paddr >> 6;
        let res = self
            .private_cache
            .record(block_id, is_instruction, is_store, ts);
        match res {
            crate::cache::CacheReturnResult::Miss => {
                self.local_shared_cache
                    .peek(block_id, is_instruction, is_store, ts);
            }
            crate::cache::CacheReturnResult::Hit => {}
            crate::cache::CacheReturnResult::MissWithEviction(blk) => {
                self.local_shared_cache
                    .record(blk, is_instruction, false, ts);
            }
            crate::cache::CacheReturnResult::MissWithWriteBack(blk) => {
                self.local_shared_cache
                    .record(blk, is_instruction, true, ts);
            }
        }
    }
}

unsafe impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    QEMUPluginPerCoreActor for TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    unsafe fn on_instruction_execution(&mut self, cpu_idx: u32, user_data: *mut std::ffi::c_void) {
        todo!()
    }

    unsafe fn on_memory_access(
        &mut self,
        cpu_idx: u32,
        info: &crate::plugin::QEMUMemoryInfo,
        vaddr: u64,
        user_data: *mut std::ffi::c_void,
    ) {
        todo!()
    }
}

// this struct contains the memory model, basically the private .
pub struct TimestampMemoryHierarchy<
    const P_A: usize,
    const P_S: usize,
    const S_A: usize,
    const S_S: usize,
> {
    // quantum channels
    tx: HashMap<u8, Receiver<usize>>,
    rx: HashMap<u8, Receiver<usize>>,

    // reference to the hierarchy
    hierarchies: HashMap<u8, &'static TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>>,
}

impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    pub fn new() -> Self {
        // Start a thread here.
        return TimestampMemoryHierarchy {
            tx: HashMap::new(),
            rx: HashMap::new(),
            hierarchies: HashMap::new(),
        };
    }

    pub unsafe fn register_core_channels(
        &mut self,
        core_id: u8,
        tx: Receiver<usize>,
        rx: Receiver<usize>,
        hierarchy: &'static TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>,
    ) {
        self.tx.insert(core_id, tx);
        self.rx.insert(core_id, rx);
        self.hierarchies.insert(core_id, hierarchy);
    }

    pub fn render_mtr<const S: usize>(&self) -> MemoryTimestampRecordCollection<S> {
        let mut res = MemoryTimestampRecordCollection::new();
        for (&core_id, v) in self.hierarchies.iter() {
            res.absorb_ts_cache(core_id, &v.local_shared_cache);
        }
        return res;
    }

    pub fn render_llc<const S: usize>(
        &self,
        mtr: &MemoryTimestampRecordCollection<S>,
    ) -> SerializedCache {
        let mut merging_sets: [HashMap<usize, TsCacheBlock>; S_S] =
            std::array::from_fn(|_| HashMap::new());

        for (&core_id, hierarchy) in self.hierarchies.iter() {
            // putting its private cache to the merging sets.
            for (idx, set) in hierarchy.private_cache.sets.iter().enumerate() {
                for (block_id, ts, status) in set.iter() {
                    if mtr.look_up(block_id) {
                        continue;
                    }

                    match merging_sets[idx].get_mut(&block_id) {
                        Some(existing) => {
                            // merge request by only updating the dirty bits
                            if status.is_dirty() {
                                existing.d.state = CacheBlockState::ModifiedExclusive;
                                assert!(existing.d.in_data_cache);
                            }
                        }
                        None => {
                            merging_sets[idx].insert(
                                block_id,
                                TsCacheBlock {
                                    d: CacheBlock {
                                        block_id,
                                        state: match status {
                                            crate::cache::ts_set::TimestampCacheLineStatus::Invalid => unreachable!(),
                                            crate::cache::ts_set::TimestampCacheLineStatus::DirtyData => CacheBlockState::ModifiedExclusive,
                                            _ => CacheBlockState::CleanExclusive,
                                        },
                                        in_instruction_cache: status.is_instruction(),
                                        in_data_cache: status.is_data(),
                                    },
                                    ts,
                                },
                            );
                        }
                    };
                }
            }
        }

        // Then, we convert the HashMap to the BinaryHeap, for its order.

        let merging_sets: Vec<_> = merging_sets
            .into_iter()
            .map(|x| {
                let mut res = BinaryHeap::new();
                for el in x {
                    res.push(el.1);
                }
                return res;
            })
            .collect();

        return merging_sets.into_iter().map(|x| x.export()).collect();
    }

    pub fn render_cache_hierarchy<const S: usize>(
        &self,
        mtr: &MemoryTimestampRecordCollection<S>,
    ) -> MemoryHierarchyCheckPoint {
        todo!();
    }
}

unsafe impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize> QEMUPlugin
    for TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    type PerCorePlugin = TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>;

    unsafe fn on_translation(
        &mut self,
        tb: &crate::plugin::QEMUPluginBasicBlock,
    ) -> Vec<PerInstructionInstrumentation> {
        todo!()
    }

    unsafe fn on_qemu_exit(&mut self) {
        todo!()
    }
}
