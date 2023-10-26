use super::TimestampCacheLineStatus;
use super::TimestampCache;
use super::checkpoint::ts_checkpoint::{LRUPrioritizing, TsCacheBlock, TsDirectoryBlock};
use super::checkpoint::{
    CacheBlock, CacheBlockState, DirectoryBlock, PrivateCacheParameters, SerializedCache,
    SerializedDirectory,
};

use std::collections::{BinaryHeap, HashMap};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MTRPermission {
    Instruction,
    InstructionAndCleanData,
    CleanData,
    DirtyData,
}

impl MTRPermission {
    pub fn is_dirty(&self) -> bool {
        return *self == Self::DirtyData;
    }
    pub fn in_instruction_cache(&self) -> bool {
        return *self == Self::Instruction || *self == Self::InstructionAndCleanData;
    }

    pub fn in_data_cache(&self) -> bool {
        return *self != Self::Instruction;
    }
}

type CoreId = u8; // 256 cores' machine should be enough and fine.

#[derive(Debug)]
pub struct MemoryTimestampRecord {
    // Well, this fucking structure has a similar size as a cache block.
    ts: usize,
    invalid: HashMap<CoreId, usize>,
    readers: HashMap<CoreId, usize>, // CoreID + timestamp
    perm: MTRPermission,
    writer: Option<(CoreId, usize)>, // CoreID + timestamp
}

impl MemoryTimestampRecord {
    pub fn generate_directory_block(&self, block_id: usize) -> TsDirectoryBlock {
        return TsDirectoryBlock {
            ts: self.ts,
            d: match self.writer {
                Some((writer, writer_ts)) => DirectoryBlock {
                    block_id,
                    replicas: self
                        .readers
                        .iter()
                        .filter_map(|(core, ts)| {
                            return if *ts < writer_ts { None } else { Some(*core) };
                        })
                        .collect(),
                    last_writer: Some(writer),
                },
                None => DirectoryBlock {
                    block_id: block_id,
                    replicas: self.readers.iter().map(|(core, _)| *core).collect(),
                    last_writer: None,
                },
            },
        };
    }
}

pub struct MemoryTimestampRecordCollection<const S: usize> {
    sets: [HashMap<usize, MemoryTimestampRecord>; S],
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
                let set_number = b_id % S;

                // TODO: make this as a standalone function.
                match self.sets[set_number].get_mut(&b_id) {
                    Some(el) => {
                        // well, update the existing one.
                        if ts > el.ts && status != TimestampCacheLineStatus::Invalid {
                            // well, you have a newer core touching this line (and it is not invalid)
                            el.ts = ts;
                        }
                        match status {
                            TimestampCacheLineStatus::Invalid => {
                                assert!(
                                    el.invalid.insert(core_id, ts).is_none(),
                                    "Each private cache should only keep cache line once."
                                );
                            }
                            TimestampCacheLineStatus::Instruction
                            | TimestampCacheLineStatus::CleanData
                            | TimestampCacheLineStatus::CleanInstructionAndData => {
                                if status.is_instruction() {
                                    assert!(el.perm.in_instruction_cache(), "NX violated: It is not possible to have the same data being modified and executable.");
                                }
                                // Well, if I did meet this problem, I need to add a new permission like DirtyDataAndInstruction
                                assert!(
                                    el.readers.insert(core_id, ts).is_none(),
                                    "Each private cache should only keep each cache line once."
                                )
                            }
                            TimestampCacheLineStatus::DirtyData => {
                                assert!(!el.perm.in_instruction_cache(), "NX violation: It is not possible to have the same data being modified and executable");
                                match el.writer {
                                    Some((writer_id, writer_ts)) => {
                                        assert!(writer_id != core_id, "Are you trying to absorb the ts_cache from the same core multiple times?");
                                        if writer_ts < ts {
                                            el.writer = Some((core_id, ts));
                                        } else if writer_ts == ts {
                                            println!("Possible inaccuracy: two cores Core[{}] and Core[{}] are writing to the same cache block({:x}) at the same time (ts={}).", writer_id, core_id, b_id, ts);
                                        }
                                    }
                                    None => {
                                        el.writer = Some((core_id, ts));
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        // append a new one
                        self.sets[set_number].insert(
                            b_id,
                            MemoryTimestampRecord {
                                ts: ts,
                                invalid: if status == TimestampCacheLineStatus::Invalid {
                                    HashMap::from([(core_id, ts)])
                                } else {
                                    HashMap::new()
                                },
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
                                    TimestampCacheLineStatus::Instruction => None,
                                    TimestampCacheLineStatus::CleanData => None,
                                    TimestampCacheLineStatus::CleanInstructionAndData => None,
                                    TimestampCacheLineStatus::DirtyData => Some((core_id, ts)),
                                },
                            },
                        );
                    }
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
                    .map(|(block_id, mtr)| mtr.generate_directory_block(*block_id))
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

        assert!(S % param.l2_sets == 0, "Required L2 set number must be an multiple of private record set number.");

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
                        } else if let Some((w_core_id, w_ts)) = mtr.writer {
                            // this is a dirty block
                            if w_core_id == core_id {
                                // well, it is the writer, so this should include.
                                collected_blocks.push(TsCacheBlock {
                                    ts: w_ts,
                                    d: CacheBlock {
                                        block_id: *blk_id,
                                        state: if w_ts == mtr.ts {
                                            // The write operation is the latest writing
                                            // TODO: Maybe the read operation happens at the same time with the write. (careful debugging here)
                                            CacheBlockState::ModifiedExclusive
                                        } else {
                                            // There are read ahead of time
                                            CacheBlockState::ModifiedOwned
                                        },
                                        in_instruction_cache: mtr.perm.in_instruction_cache(),
                                        in_data_cache: mtr.perm.in_data_cache(),
                                    },
                                });
                            } else {
                                // so, there is another writer. We need this information to see whether the correct core has a valid replica.
                                if mtr.readers.contains_key(&core_id) {
                                    if mtr.readers[&core_id] >= w_ts {
                                        // it is a valid replica.
                                        collected_blocks.push(TsCacheBlock {
                                            ts: mtr.readers[&core_id],
                                            d: CacheBlock {
                                                block_id: *blk_id,
                                                state: CacheBlockState::CleanShared,
                                                in_instruction_cache: mtr
                                                    .perm
                                                    .in_instruction_cache(),
                                                in_data_cache: mtr.perm.in_data_cache(),
                                            },
                                        });
                                    } else {
                                        // This will be an invalid chunk.
                                        collected_blocks.push(TsCacheBlock {
                                            ts: mtr.readers[&core_id],
                                            d: CacheBlock {
                                                block_id: *blk_id,
                                                state: CacheBlockState::Invalid,
                                                in_instruction_cache: mtr
                                                    .perm
                                                    .in_instruction_cache(),
                                                in_data_cache: mtr.perm.in_data_cache(),
                                            },
                                        });
                                    }
                                }
                            }
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

    pub fn look_up(&self, block_id: usize) -> bool {
        let set_number = block_id % S;
        return self.sets[set_number].contains_key(&block_id);
    }
}
