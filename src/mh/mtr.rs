use crate::cache::ts_set::TimestampCacheLineStatus;
use crate::cache::TimestampCache;
use crate::checkpoint::{SerializedCache, SerializedDirectory};

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
                if status == TimestampCacheLineStatus::Invalid {
                    println!("Warning: ");
                    continue;
                }

                let set_number = b_id % S;

                match self.sets[set_number].get_mut(&b_id) {
                    Some(el) => {
                        // well, update the existing one.
                        if ts > el.ts {
                            // well, you have a newer core touching this line
                            el.ts = ts;
                        }
                        match status {
                            TimestampCacheLineStatus::Invalid => {
                                unreachable!("Something wrong with the iterator")
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
    pub fn remove_invalid_reader(&mut self) {
        self.sets.iter_mut().for_each(|set| {
            set.iter_mut().for_each(|(block_id, mtr)| match mtr.writer {
                Some((writer_id, writer_ts)) => mtr.readers.retain(|read_core_id, read_ts| {
                    return *read_ts >= writer_ts;
                }),
                None => {}
            })
        })
    }

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
        return self.sets.iter().map(|x|{
            todo!();
        }).collect();
    }

    /// Generate the private cache of a given core_id.
    /// Arguments:
    /// - core_id: which core's private cache will be reconstructed
    /// - params: A list of parameters for reconstruction, type: (set, associativity, is_instruction)
    pub fn render_private_caches(&self, core_id: usize, params: Vec<(usize, usize, bool)>) -> Vec<SerializedCache> {
        
        todo!();
    }
}
