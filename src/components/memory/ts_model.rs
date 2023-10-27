use std::collections::{BinaryHeap, HashMap};

use super::mtr::MemoryTimestampRecordCollection;
use super::TimestampSingleCoreMemoryHierarchy;
use super::checkpoint::ts_checkpoint::{LRUPrioritizing, TsCacheBlock};
use super::checkpoint::{
    CacheBlock, CacheBlockState, MemoryHierarchyCheckPoint, PrivateCacheParameters, SerializedCache,
};

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
    // hierarchies: HashMap<u8, TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>>,
    hierarchies: Vec<TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>>,
}

impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    pub fn new(core_count: usize) -> Self {
        return TimestampMemoryHierarchy {
            // hierarchies: Box::new([TimestampSingleCoreMemoryHierarchy::new(); crate::CORE_COUNT]),
            hierarchies: Vec::from_iter(
                (0..core_count).map(|_| TimestampSingleCoreMemoryHierarchy::new()),
            )
        };
    }

    pub fn hierarchies(
        &self,
        core_id: u8,
    ) -> &mut TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S> {
        // This function can be only called from each vCPU, and it meets the following requirement:
        // - Each thread has its unique core_id (no cases for two thread access the same hierarchy)
        // - During reconstruction, all other threads must stop.
        assert!(core_id < self.hierarchies.len() as u8);
        unsafe {
            let target_ref = &self.hierarchies[core_id as usize]
                as *const TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>;
            let target_ref =
                target_ref as *mut TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>;
            return &mut *target_ref;
        }
    }

    pub fn render_mtr<const S: usize>(&self) -> MemoryTimestampRecordCollection<S> {
        let mut res = MemoryTimestampRecordCollection::new();
        for (core_id, per_core_record) in self.hierarchies.iter().enumerate() {
            res.absorb_ts_cache(core_id as u8, &per_core_record.private_cache);
        }
        return res;
    }

    pub fn render_llc<const S: usize>(
        &self,
        mtr: &MemoryTimestampRecordCollection<S>,
    ) -> SerializedCache {
        let mut merging_sets: Vec<HashMap<usize, TsCacheBlock>> =
            Vec::from_iter((0..crate::parameter::SHARED_CACHE_SET).map(|_| HashMap::new()));

        for per_core_record in self.hierarchies.iter() {
            // putting its private cache to the merging sets.
            for (idx, set) in per_core_record.local_shared_cache.sets.iter().enumerate() {
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
                                            super::TimestampCacheLineStatus::Invalid => unreachable!(),
                                            super::TimestampCacheLineStatus::DirtyData => CacheBlockState::ModifiedExclusive,
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

        return merging_sets.into_iter().map(|x| x.get_top_k(crate::parameter::SHARED_CACHE_ASSO).export()).collect();
    }

    pub fn render_cache_hierarchy<const S: usize>(
        &self,
        mtr: &MemoryTimestampRecordCollection<S>,
        param: &PrivateCacheParameters,
    ) -> MemoryHierarchyCheckPoint {
        let mut l1i = HashMap::new();
        let mut l1d = HashMap::new();
        let mut l2 = HashMap::new();
        for core_id in 0..self.hierarchies.len() as u8 {
            let mut pri = mtr.render_private_caches(core_id, param).into_iter();
            l1i.insert(core_id, pri.next().unwrap());
            l1d.insert(core_id, pri.next().unwrap());
            l2.insert(core_id, pri.next().unwrap());
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

// unsafe impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize> QEMUPlugin
//     for TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>
// {
//     type PerCorePlugin = TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>;

//     unsafe fn on_translation(
//         &self,
//         tb: &crate::plugin::QEMUPluginBasicBlock,
//     ) -> Vec<PerInstructionInstrumentation> {
//         // Okay, now it is time to generate the translation.
//         // TODO: Make the `6 (64B)` here a variable.
//         let cache_line_size: Vec<_> = tb.iter().map(|x| x.physical_address() >> 6).collect();
//         let mut first_appearance: HashMap<usize, Vec<usize>> = HashMap::new();
//         let mut ordered_cache_line = vec![];
//         let mut base_index: usize = 0;
//         for cache_line in cache_line_size.into_iter() {
//             if !first_appearance.contains_key(&cache_line) {
//                 first_appearance.insert(cache_line, vec![]);
//                 ordered_cache_line.push(cache_line);
//                 base_index = 0;
//             } else {
//                 base_index += 1;
//             }
//             first_appearance
//                 .get_mut(&cache_line)
//                 .unwrap()
//                 .push(base_index);
//         }

//         // OK, now the goal is to flatten the HashMap
//         let mut res = vec![];
//         for line in ordered_cache_line {
//             for pbb_idx in first_appearance[&line].iter() {
//                 if *pbb_idx == 0 {
//                     // Fuck, this is the first instruction of the pBB, thus the helper should be inserted.
//                     let metadata = PBBMetadata {
//                         physical_addr: line << 6,
//                         instruction_count: first_appearance[&line].len() as u8,
//                     };
//                     res.push(PerInstructionInstrumentation {
//                         instruction_execution: Some(metadata.encode() as *mut c_void),
//                         memory_access: Some(*pbb_idx as *mut c_void),
//                     });
//                 } else {
//                     // Now it is in the middle, so I just need to insert memory access helper
//                     res.push(PerInstructionInstrumentation {
//                         instruction_execution: None,
//                         memory_access: Some(*pbb_idx as *mut c_void),
//                     });
//                 }
//             }
//         }
//         return res;
//     }

//     unsafe fn on_qemu_exit(&self) {
//         let private_param = PrivateCacheParameters {
//             l1i_sets: 1,
//             l1i_associativity: 8,
//             l1d_sets: 1,
//             l1d_associativity: 8,
//             l2_sets: P_S,
//             l2_associativity: P_A,
//         };

//         let mtr = self.render_mtr::<P_S>();
//         let caches = self.render_cache_hierarchy(&mtr, &private_param);

//         let exported_json = serde_json::to_string_pretty(&caches).unwrap();

//         let mut output = fs::File::create("./dumped.json").unwrap();

//         output.write_all(exported_json.as_bytes()).unwrap();
//     }
// }
