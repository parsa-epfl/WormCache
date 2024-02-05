use super::checkpoint::ts_checkpoint::{LRUPrioritizing, TsCacheBlock, TsDirectoryBlock};
use super::checkpoint::{
    CacheBlock, CacheBlockState, DirectoryBlock, PrivateCacheParameters, SerializedCache,
    SerializedDirectory,
};
use super::TimestampCache;
use super::TimestampCacheLineStatus;

use std::collections::{BinaryHeap, HashMap};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MTRPermission {
    Instruction,
    InstructionAndCleanData,
    CleanData,
    DirtyData,
}

impl MTRPermission {
    // pub fn is_dirty(&self) -> bool {
    //     return *self == Self::DirtyData;
    // }
    pub fn in_instruction_cache(&self) -> bool {
        return *self == Self::Instruction || *self == Self::InstructionAndCleanData;
    }

    pub fn in_data_cache(&self) -> bool {
        return *self != Self::Instruction;
    }
}

type CoreId = u8; // 256 cores' machine should be enough and fine.

#[derive(Debug)]
pub enum WriterType {
    None,
    Evicted(CoreId, usize), // CoreID + timestamp
    Normal(CoreId, usize),  // CoreID + timestamp
}

impl WriterType {
    pub fn compare_and_replace(
        &mut self,
        core_id: CoreId,
        timestamp: usize,
        is_evicted: bool,
    ) -> bool {
        let new_line = if is_evicted {
            WriterType::Evicted(core_id, timestamp)
        } else {
            WriterType::Normal(core_id, timestamp)
        };
        match self {
            WriterType::None => {
                *self = new_line;
                return true;
            }
            WriterType::Evicted(_core_id, ts) => {
                if *ts < timestamp {
                    *self = new_line;
                    return true;
                } else {
                    return false;
                }
            }
            WriterType::Normal(_core_id, ts) => {
                if *ts < timestamp {
                    *self = new_line;
                    return true;
                } else {
                    return false;
                }
            }
        }
    }

    pub fn get_timestamp(&self) -> Option<usize> {
        match self {
            WriterType::None => None,
            WriterType::Evicted(_, ts) => Some(*ts),
            WriterType::Normal(_, ts) => Some(*ts),
        }
    }
}

#[derive(Debug)]
pub struct MemoryTimestampRecord {
    // Well, this fucking structure has a similar size as a cache block.
    ts: usize,
    invalid: HashMap<CoreId, usize>,
    readers: HashMap<CoreId, usize>, // CoreID + timestamp
    perm: MTRPermission,
    writer: WriterType,
}

impl MemoryTimestampRecord {
    // Filter readers. This only happens when a new writer is registered.
    pub fn filter_readers_by_ts(&mut self, ts: usize) {
        self.readers.retain(|_, &mut reader_ts| reader_ts >= ts);
    }

    // pub fn check_non_outdated_reader(&self) {
    //     if let Some(writer_ts) = self.writer.get_timestamp() {
    //         self.readers.iter().for_each(|(_core_id, ts)| {
    //             assert!(
    //                 *ts <= writer_ts,
    //                 "Reader with larger timestamp than the writer should be evicted."
    //             );
    //         })
    //     }
    // }

    pub fn merge_cache_block(
        &mut self,
        core_id: CoreId,
        _block_id: u64,
        ts: usize,
        status: TimestampCacheLineStatus,
    ) {
        // TODO: Filter cache block at this point.
        // well, update the existing one.
        if ts > self.ts && status != TimestampCacheLineStatus::Invalid {
            // well, you have a newer core touching this line (antsd it is not invalid)
            self.ts = ts;
        }
        match status {
            TimestampCacheLineStatus::Invalid => {} // no action for the invalid block. They are handled by a different function.
            TimestampCacheLineStatus::Instruction
            | TimestampCacheLineStatus::CleanData
            | TimestampCacheLineStatus::CleanInstructionAndData => {
                if let Some(writer_ts) = self.writer.get_timestamp() {
                    if ts < writer_ts {
                        // There is no need to insert this writer.
                        return;
                    }
                }

                if status.is_instruction() {
                    assert!(self.perm.in_instruction_cache(), "NX violated: It is not possible to have the same data being modified and executable.");
                }
                // Well, if I did meet this problem, I need to add a new permission like DirtyDataAndInstruction
                assert!(
                    self.readers.insert(core_id, ts).is_none(),
                    "Each private cache should only keep each cache line once."
                )
            }
            TimestampCacheLineStatus::DirtyData => {
                assert!(!self.perm.in_instruction_cache(), "NX violation: It is not possible to have the same data being modified and executable");
                self.writer.compare_and_replace(core_id, ts, false);
                self.filter_readers_by_ts(ts);
            }
        }
    }

    pub fn merge_evicted_writer(&mut self, core_id: CoreId, ts: usize) {
        // keep the one with a larger timestamp.
        self.writer.compare_and_replace(core_id, ts, true);
        self.filter_readers_by_ts(ts);
    }

    pub fn _genreate_each_holder_state_moesi(&self) -> HashMap<CoreId, CacheBlockState> {
        unimplemented!()
    }

    pub fn _genreate_each_holder_state_mesi(&self) -> HashMap<CoreId, CacheBlockState> {
        unimplemented!();
    }

    pub fn generate_directory_block(&self, block_id: u64) -> Option<TsDirectoryBlock> {
        // self.check_non_outdated_reader();
        return match self.writer {
            WriterType::None => Some(TsDirectoryBlock {
                d: DirectoryBlock {
                    block_id: block_id,
                    replicas: self.readers.iter().map(|(core, _)| *core).collect(),
                    last_writer: None,
                },
                ts: self.ts,
            }),
            WriterType::Evicted(_, _) => None,
            WriterType::Normal(core_id, _) => Some(TsDirectoryBlock {
                d: DirectoryBlock {
                    block_id: block_id,
                    replicas: self.readers.iter().map(|(core, _)| *core).collect(),
                    last_writer: Some(core_id),
                },
                ts: self.ts,
            }),
        };
    }
}

pub struct MemoryTimestampRecordCollection<const S: usize> {
    sets: [HashMap<u64, MemoryTimestampRecord>; S],
}

impl<const S: usize> MemoryTimestampRecordCollection<S> {
    const _SET_COUNT_CHECKER: () = assert!((S & (S - 1)) == 0);

    pub fn new() -> Self {
        return Self {
            sets: std::array::from_fn(|_| HashMap::new()),
        };
    }

    /// Update the MTR info from a given private cache
    /// Arguments:
    /// * core_id: the owner id of the given cache
    /// * other: the reference to the cache
    pub fn absorb_ts_cache<const A: usize, const CACHE_S: usize>(
        &mut self,
        core_id: CoreId,
        other: &TimestampCache<A, CACHE_S>,
    ) {
        // this function will read the content of the cache and update the MTR accordingly.
        // scan all entries in `other` and insert them into the system
        for set in other.sets.iter() {
            for (b_id, ts, status) in set.iter() {
                let set_number = b_id as usize % S;

                // TODO: make this as a standalone function.
                match self.sets[set_number].get_mut(&b_id) {
                    Some(el) => {
                        el.merge_cache_block(core_id, b_id, ts, status);
                    }
                    None => {
                        // append a new one
                        self.sets[set_number].insert(
                            b_id,
                            MemoryTimestampRecord {
                                ts: ts,
                                invalid: HashMap::new(), // invalid is handled by a different function. (absorb_invalid_information)
                                readers: match status {
                                    TimestampCacheLineStatus::Invalid => {
                                        unreachable!("Something wrong with the iterator.")
                                    }
                                    TimestampCacheLineStatus::Instruction => {
                                        HashMap::from([(core_id, ts)])
                                    }
                                    TimestampCacheLineStatus::CleanData => {
                                        HashMap::from([(core_id, ts)])
                                    }
                                    TimestampCacheLineStatus::CleanInstructionAndData => {
                                        HashMap::from([(core_id, ts)])
                                    }
                                    TimestampCacheLineStatus::DirtyData => HashMap::new(),
                                },
                                perm: match status {
                                    TimestampCacheLineStatus::Invalid => {
                                        unreachable!("Something wrong with the iterator.")
                                    }
                                    TimestampCacheLineStatus::Instruction => {
                                        MTRPermission::Instruction
                                    }
                                    TimestampCacheLineStatus::CleanData => MTRPermission::CleanData,
                                    TimestampCacheLineStatus::CleanInstructionAndData => {
                                        MTRPermission::CleanData
                                    }
                                    TimestampCacheLineStatus::DirtyData => MTRPermission::DirtyData,
                                },
                                writer: match status {
                                    TimestampCacheLineStatus::Invalid => {
                                        unreachable!("Something wrong with the iterator.")
                                    }
                                    TimestampCacheLineStatus::Instruction => WriterType::None,
                                    TimestampCacheLineStatus::CleanData => WriterType::None,
                                    TimestampCacheLineStatus::CleanInstructionAndData => {
                                        WriterType::None
                                    }
                                    TimestampCacheLineStatus::DirtyData => {
                                        WriterType::Normal(core_id, ts)
                                    }
                                },
                            },
                        );
                    }
                }
            }
        }
    }

    pub fn absorb_evicted_writer(
        &mut self,
        core_id: CoreId,
        evicted_writer_list: &HashMap<u64, usize>,
    ) {
        // This function will absorb the invalid list from the private cache.
        for (block_id, ts) in evicted_writer_list.iter() {
            let set_number = (*block_id) as usize % S;
            match self.sets[set_number].get_mut(block_id) {
                Some(el) => {
                    el.merge_evicted_writer(core_id, *ts);
                }
                None => {
                    self.sets[set_number].insert(
                        *block_id,
                        MemoryTimestampRecord {
                            ts: *ts,
                            invalid: HashMap::new(),
                            readers: HashMap::new(),
                            perm: MTRPermission::DirtyData,
                            writer: WriterType::None,
                        },
                    );
                }
            }
        }
    }

    pub fn absorb_invalid_history(
        &mut self,
        core_id: CoreId,
        invalid_list: &HashMap<u64, usize>,
    ) {
        // This function will absorb the invalid list from the private cache.
        for (block_id, ts) in invalid_list.iter() {
            let set_number = (*block_id as usize) % S;
            match self.sets[set_number].get_mut(block_id) {
                Some(el) => {
                    // insert the invalid history
                    match el.invalid.get_mut(&core_id) {
                        Some(_) => {
                            // impossible!
                            assert!(
                                false,
                                "Impossible: the same core invalid the same cache line twice."
                            );
                        }
                        None => {
                            el.invalid.insert(core_id, *ts);
                        }
                    }
                }
                None => {
                    self.sets[set_number].insert(
                        *block_id,
                        MemoryTimestampRecord {
                            ts: *ts,
                            invalid: HashMap::from([(core_id, *ts)]),
                            readers: HashMap::new(),
                            perm: MTRPermission::DirtyData,
                            writer: WriterType::None,
                        },
                    );
                }
            }
        }
    }

    /// Remove the invalid reader (e.g., the read record which is before the latest writer.)
    /// This step is necessary before rendering the directory, private caches, and L2.
    // pub fn remove_invalid_reader(&mut self) {
    //     self.sets.iter_mut().for_each(|set| {
    //         set.iter_mut().for_each(|(block_id, mtr)| match mtr.writer {
    //             Some((writer_id, writer_ts)) => mtr.readers.retain(|read_core_id, read_ts| {
    //                 return *read_ts >= writer_ts;
    //             }),
    //             None => {}
    //         })
    //     })
    // }

    /// Update the MTR Collection by considering a finite associativity.
    pub fn prune_by_associativity(self, associativity: usize) -> Self {
        return Self {
            sets: self
                .sets
                .into_iter()
                .map(|mut set| {
                    let mut timestamps: Vec<_> = set.iter().map(|el| return el.1.ts).collect();

                    // Well, if it is smaller than the associativity, we can throw it away.
                    if timestamps.len() <= associativity {
                        return set;
                    };

                    // Then we determine the boundary checkpoint.
                    timestamps.sort_unstable();
                    let minimum = timestamps[timestamps.len() - associativity];

                    set.retain(|_, el| {
                        return el.ts >= minimum;
                    });

                    return set;
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        };
    }

    pub fn export_directory(&self) -> SerializedDirectory {
        return self
            .sets
            .iter()
            .map(|x| {
                let mut res_with_ts: Vec<_> = x
                    .iter()
                    .filter_map(|(block_id, mtr)| mtr.generate_directory_block(*block_id))
                    .collect();

                res_with_ts.sort_unstable_by(|a, b| {
                    return b.ts.cmp(&a.ts);
                });

                return res_with_ts.into_iter().map(|el| el.d).collect();
            })
            .collect();
    }

    /// Generate the private cache of a given core_id.
    /// Arguments:
    /// - core_id: which core's private cache will be reconstructed
    /// - param: the parameters of the private cache
    /// Returns:
    /// - [L1i, L1d, L2]
    pub fn render_private_caches(
        &self,
        core_id: CoreId,
        param: &PrivateCacheParameters,
    ) -> [SerializedCache; 3] {
        use rayon::prelude::*;

        assert!(
            S % param.l2_sets == 0,
            "Required L2 set number must be an multiple of private record set number."
        );

        let l2_with_ts: Vec<_> = (0..param.l2_sets)
            .into_par_iter()
            .map(|group_bias| {
                let mut collected_blocks = BinaryHeap::new();
                for group_index in 0..(S / param.l2_sets) {
                    let set_number = group_index * param.l2_sets + group_bias;
                    for (blk_id, mtr) in self.sets[set_number].iter() {
                        if mtr.invalid.contains_key(&core_id) {
                            // OK, so it has an invalid history there. Amazing.
                            collected_blocks.push(TsCacheBlock {
                                d: CacheBlock {
                                    block_id: *blk_id,
                                    state: CacheBlockState::Invalid,
                                    in_instruction_cache: mtr.perm.in_instruction_cache(),
                                    in_data_cache: mtr.perm.in_data_cache(),
                                },
                                ts: mtr.invalid[&core_id],
                            });
                        // } else if let Some((w_core_id, w_ts)) = mtr.writer {
                        //     // this is a dirty block
                        //     if w_core_id == core_id {
                        //         // well, it is the writer, so this should include.
                        //         collected_blocks.push(TsCacheBlock {
                        //             ts: w_ts,
                        //             d: CacheBlock {
                        //                 block_id: *blk_id,
                        //                 state: if w_ts == mtr.ts {
                        //                     // The write operation is the latest writing
                        //                     // TODO: Maybe the read operation happens at the same time with the write. (careful debugging here)
                        //                     CacheBlockState::ModifiedExclusive
                        //                 } else {
                        //                     // There are read ahead of time
                        //                     CacheBlockState::ModifiedOwned
                        //                 },
                        //                 in_instruction_cache: mtr.perm.in_instruction_cache(),
                        //                 in_data_cache: mtr.perm.in_data_cache(),
                        //             },
                        //         });
                        //     } else {
                        //         // so, there is another writer. We need this information to see whether the correct core has a valid replica.
                        //         if mtr.readers.contains_key(&core_id) {
                        //             if mtr.readers[&core_id] >= w_ts {
                        //                 // it is a valid replica.
                        //                 collected_blocks.push(TsCacheBlock {
                        //                     ts: mtr.readers[&core_id],
                        //                     d: CacheBlock {
                        //                         block_id: *blk_id,
                        //                         state: CacheBlockState::CleanShared,
                        //                         in_instruction_cache: mtr
                        //                             .perm
                        //                             .in_instruction_cache(),
                        //                         in_data_cache: mtr.perm.in_data_cache(),
                        //                     },
                        //                 });
                        //             } else {
                        //                 // This will be an invalid chunk.
                        //                 collected_blocks.push(TsCacheBlock {
                        //                     ts: mtr.readers[&core_id],
                        //                     d: CacheBlock {
                        //                         block_id: *blk_id,
                        //                         state: CacheBlockState::Invalid,
                        //                         in_instruction_cache: mtr
                        //                             .perm
                        //                             .in_instruction_cache(),
                        //                         in_data_cache: mtr.perm.in_data_cache(),
                        //                     },
                        //                 });
                        //             }
                        //         }
                        //     }
                        } else {
                            // OK, now it is clean, and it determines whether this is widely shared.
                            if mtr.readers.contains_key(&core_id) {
                                collected_blocks.push(TsCacheBlock {
                                    ts: mtr.readers[&core_id],
                                    d: CacheBlock {
                                        block_id: *blk_id,
                                        state: if mtr.readers.len() == 1 {
                                            CacheBlockState::CleanExclusive
                                        } else {
                                            CacheBlockState::CleanShared
                                        },
                                        in_instruction_cache: mtr.perm.in_instruction_cache(),
                                        in_data_cache: mtr.perm.in_data_cache(),
                                    },
                                })
                            }
                        }
                    }
                }
                return collected_blocks.get_top_k(param.l2_associativity);
            })
            .collect();
        // Now, use L2 to reconstruct L1i and L1d, with the L2.

        let mut two_caches = [
            (param.l1i_sets, param.l1i_associativity, true),
            (param.l1d_sets, param.l1d_associativity, false),
        ]
        .map(
            |(set, asso, is_instruction)| -> Vec<BinaryHeap<TsCacheBlock>> {
                assert!(
                    (param.l2_sets % set) == 0,
                    "L2 cache set count should be a multiple of L1's"
                );
                // collect all chunks
                return (0..set)
                    .map(|set_idx| {
                        let mut related_blocks = BinaryHeap::new();
                        for affiliated_set_idx in 0..(param.l2_sets / set) {
                            let target_l2_set = affiliated_set_idx * set + set_idx;

                            for el in l2_with_ts[target_l2_set].iter() {
                                if is_instruction && el.d.in_instruction_cache {
                                    related_blocks.push(el.clone());
                                }

                                if !is_instruction && el.d.in_data_cache {
                                    related_blocks.push(el.clone());
                                }
                            }
                        }
                        return related_blocks.get_top_k(asso);
                    })
                    .collect();
            },
        )
        .into_iter();

        // This point is also very dirty.
        return [
            two_caches
                .next()
                .unwrap()
                .into_iter()
                .map(|el| el.export())
                .collect(),
            two_caches
                .next()
                .unwrap()
                .into_iter()
                .map(|el| el.export())
                .collect(),
            l2_with_ts.into_iter().map(|el| el.export()).collect(),
        ];
    }

    pub fn look_up(&self, block_id: u64) -> bool {
        let set_number = block_id as usize % S;
        return self.sets[set_number].contains_key(&block_id);
    }
}
