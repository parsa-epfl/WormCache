use crate::cache::ts_set::TimestampCacheLineStatus;
use crate::cache::TimestampCache;
use crate::checkpoint::{
    CacheBlock, CacheBlockPermission, DirectoryBlock, PrivateCacheParameters, SerializedCache,
    SerializedDirectory,
};

use std::collections::HashMap;

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
    pub fn is_instruction(&self) -> bool {
        return *self == Self::Instruction || *self == Self::InstructionAndCleanData;
    }
}

type CoreId = u8; // 256 cores' machine should be enough and fine.

#[derive(Debug)]
struct MemoryTimestampRecord {
    // Well, this fucking structure has a similar size as a cache block.
    ts: usize,
    invalid: HashMap<CoreId, usize>,
    readers: HashMap<CoreId, usize>, // CoreID + timestamp
    perm: MTRPermission,
    writer: Option<(CoreId, usize)>, // CoreID + timestamp
}

struct MemoryTimestampRecordCollection<const S: usize> {
    sets: [HashMap<usize, MemoryTimestampRecord>; S],
}

impl<const S: usize> MemoryTimestampRecordCollection<S> {
    const _SET_COUNT_CHECKER: () = assert!((S & (S - 1)) == 0);
    const S_LOG2: usize = S.trailing_zeros() as usize;

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
                                assert!(el.perm != MTRPermission::DirtyData, "NX violated: It is not possible to have the same data being modified and executable.");
                                // Well, if I did meet this problem, I need to add a new permission like DirtyDataAndInstruction
                                assert!(
                                    el.readers.insert(core_id, ts).is_none(),
                                    "Each private cache should only keep each cache line once."
                                )
                            }
                            TimestampCacheLineStatus::DirtyData => {
                                assert!(el.perm == MTRPermission::DirtyData, "NX violation: It is not possible to have the same data being modified and executable");
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
                    .map(|(block_id, mtr)| {
                        return (
                            mtr.ts,
                            match mtr.writer {
                                Some((writer, writer_ts)) => DirectoryBlock {
                                    tag: *block_id,
                                    replicas: mtr
                                        .readers
                                        .iter()
                                        .filter_map(|(core, ts)| {
                                            return if *ts < writer_ts {
                                                None
                                            } else {
                                                Some(*core)
                                            };
                                        })
                                        .collect(),
                                    last_writer: Some(writer),
                                },
                                None => DirectoryBlock {
                                    tag: *block_id,
                                    replicas: mtr.readers.iter().map(|(core, _)| *core).collect(),
                                    last_writer: None,
                                },
                            },
                        );
                    })
                    .collect();

                res_with_ts.sort_unstable_by(|a, b| {
                    return b.0.cmp(&a.0);
                });

                return res_with_ts.into_iter().map(|el| el.1).collect();
            })
            .collect();
    }

    /// Generate the private cache of a given core_id.
    /// Arguments:
    /// - core_id: which core's private cache will be reconstructed
    pub fn render_private_caches(
        &self,
        core_id: CoreId,
        param: PrivateCacheParameters,
    ) -> [SerializedCache; 3] {
        use rayon::prelude::*;
        let l2_with_ts: Vec<_> = (0..param.l2_sets)
            .into_par_iter()
            .map(|group_bias| {
                let mut collected_blocks = vec![];
                for group_index in 0..(S / param.l2_sets) {
                    let set_number = group_index * param.l2_sets + group_bias;
                    for (blk_id, mtr) in self.sets[set_number].iter() {
                        if mtr.invalid.contains_key(&core_id) {
                            // OK, so it has an invalid history there. Amazing.
                            collected_blocks.push((
                                mtr.invalid[&core_id],
                                CacheBlock {
                                    block_id: *blk_id,
                                    perm: CacheBlockPermission::Invalid,
                                },
                                mtr.perm.is_instruction(),
                            ));
                        } else if let Some((w_core_id, w_ts)) = mtr.writer {
                            // this is a dirty block
                            if w_core_id == core_id {
                                // well, it is the writer, so this should include.
                                collected_blocks.push((
                                    w_ts,
                                    CacheBlock {
                                        block_id: *blk_id,
                                        perm: if w_ts == mtr.ts {
                                            // The write operation is the latest writing
                                            // TODO: Maybe the read operation happens at the same time with the write. (careful debugging here)
                                            CacheBlockPermission::ModifiedExclusive
                                        } else {
                                            // There are read ahead of time
                                            CacheBlockPermission::ModifiedOwned
                                        },
                                    },
                                    mtr.perm.is_instruction(),
                                ));
                            } else {
                                // so, there is another writer. We need this information to see whether the correct core has a valid replica.
                                if mtr.readers.contains_key(&core_id) {
                                    if mtr.readers[&core_id] >= w_ts {
                                        // it is a valid replica.
                                        collected_blocks.push((
                                            mtr.readers[&core_id],
                                            CacheBlock {
                                                block_id: *blk_id,
                                                perm: CacheBlockPermission::CleanShared,
                                            },
                                            mtr.perm.is_instruction(),
                                        ));
                                    } else {
                                        // This will be an invalid chunk.
                                        collected_blocks.push((
                                            mtr.readers[&core_id],
                                            CacheBlock {
                                                block_id: *blk_id,
                                                perm: CacheBlockPermission::Invalid,
                                            },
                                            mtr.perm.is_instruction(),
                                        ));
                                    }
                                }
                            }
                        } else {
                            // OK, now it is clean, and it determines whether this is widely shared.
                            if mtr.readers.contains_key(&core_id) {
                                collected_blocks.push((
                                    mtr.readers[&core_id],
                                    CacheBlock {
                                        block_id: *blk_id,
                                        perm: if mtr.readers.len() == 1 {
                                            CacheBlockPermission::CleanExclusive
                                        } else {
                                            CacheBlockPermission::CleanShared
                                        },
                                    },
                                    mtr.perm.is_instruction(),
                                ))
                            }
                        }
                    }
                }
                // Now, we can sort the array based on the timestamp.
                collected_blocks.sort_unstable_by(|a, b| b.0.cmp(&a.0));

                if collected_blocks.len() > param.l2_associativity {
                    // remove excessive elements
                    collected_blocks.drain(param.l2_associativity..);
                }
                return collected_blocks;
            })
            .collect();
        // Now, use L2 to reconstruct L1i and L1d, with the L2.
        for (set, asso, is_instruction) in param.private_cache_iter() {
            assert!((param.l2_sets % set) == 0, "L2 cache set count should be a multiple of L1's");
            // Fuck! I have to merge the set again. 
            
        }
        todo!();
    }
}
