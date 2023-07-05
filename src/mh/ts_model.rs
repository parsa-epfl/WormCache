use std::collections::{BinaryHeap, HashMap};
use std::ffi::c_void;
use std::io::prelude::*;
use std::fs;

use super::mtr::MemoryTimestampRecordCollection;
use super::pbb_metadata::PBBMetadata;
use super::ts_per_core::TimestampSingleCoreMemoryHierarchy;
use crate::checkpoint::ts_checkpoint::{LRUPrioritizing, TsCacheBlock};
use crate::checkpoint::{
    CacheBlock, CacheBlockState, MemoryHierarchyCheckPoint, PrivateCacheParameters, SerializedCache,
};
use crate::plugin::PerInstructionInstrumentation;
use crate::{QEMUPlugin, INSTRUMENTED_CORE_LIST};

// This file builds a memory hierarchy model using Cache recording timestamp.
// TODO: Add the traffic from the page walker and the prefetcher.

// This module contains the logic of quantum management and cache reconstruction.
#[derive(Debug)]
pub struct TimestampMemoryHierarchy<
    const P_A: usize,
    const P_S: usize,
    const S_A: usize,
    const S_S: usize,
> {
    // reference to the hierarchy
    hierarchies: HashMap<u8, &'static TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>>,
}

impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    pub fn new() -> Self {
        // Start a thread here.
        return TimestampMemoryHierarchy {
            hierarchies: HashMap::new(),
        };
    }

    pub unsafe fn register_core_channels(
        &mut self,
        core_id: u8,
        hierarchy: &'static TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>,
    ) {
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

        for (&_, hierarchy) in self.hierarchies.iter() {
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
        param: &PrivateCacheParameters,
    ) -> MemoryHierarchyCheckPoint {
        let mut l1i = HashMap::new();
        let mut l1d = HashMap::new();
        let mut l2 = HashMap::new();
        for core_id in INSTRUMENTED_CORE_LIST.iter() {
            let mut pri = mtr.render_private_caches(*core_id, param).into_iter();
            l1i.insert(*core_id, pri.next().unwrap());
            l1d.insert(*core_id, pri.next().unwrap());
            l2.insert(*core_id, pri.next().unwrap());
        }

        return MemoryHierarchyCheckPoint {
            l1i,
            l1d,
            l2,
            directory: mtr.export_directory(),
            shared_cache: self.render_llc(mtr),
        };
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
        // Okay, now it is time to generate the translation.
        // TODO: Make the `6 (64B)` here a variable.
        let cache_line_size: Vec<_> = tb.iter().map(|x| x.physical_address() >> 6).collect();
        let mut first_appearance: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut ordered_cache_line = vec![];
        let mut base_index: usize = 0;
        for cache_line in cache_line_size.into_iter() {
            if !first_appearance.contains_key(&cache_line) {
                first_appearance.insert(cache_line, vec![]);
                ordered_cache_line.push(cache_line);
                base_index = 0;
            } else {
                base_index += 1;
            }
            first_appearance
                .get_mut(&cache_line)
                .unwrap()
                .push(base_index);
        }

        // OK, now the goal is to flatten the HashMap
        let mut res = vec![];
        for line in ordered_cache_line {
            for pbb_idx in first_appearance[&line].iter() {
                if *pbb_idx == 0 {
                    // Fuck, this is the first instruction of the pBB, thus the helper should be inserted.
                    let metadata = PBBMetadata {
                        physical_addr: line << 6,
                        instruction_count: first_appearance[&line].len() as u8,
                    };
                    res.push(PerInstructionInstrumentation {
                        instruction_execution: Some(metadata.encode() as *mut c_void),
                        memory_access: Some(*pbb_idx as *mut c_void),
                    });
                } else {
                    // Now it is in the middle, so I just need to insert memory access helper
                    res.push(PerInstructionInstrumentation {
                        instruction_execution: None,
                        memory_access: Some(*pbb_idx as *mut c_void),
                    });
                }
            }
        }
        return res;
    }

    unsafe fn on_qemu_exit(&mut self) {
        let private_param = PrivateCacheParameters {
            l1i_sets: 128,
            l1i_associativity: 8,
            l1d_sets: 128,
            l1d_associativity: 8,
            l2_sets: 2048,
            l2_associativity: 16,
        };

        let mtr = self.render_mtr::<P_S>();
        let caches = self.render_cache_hierarchy(&mtr, &private_param);

        let exported_json = serde_json::to_string(&caches).unwrap();

        let mut output = fs::File::create("./dumped.json").unwrap();
    }
}
