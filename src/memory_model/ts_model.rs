use std::collections::HashMap;

use super::checkpoint::CacheBlockPermission;
use super::checkpoint::CacheSet;
use super::checkpoint::MemoryHierarchyCheckPoint;
use super::checkpoint::SerializedCache;
use crate::cache::ts_cache::TimestampCache;

use crate::memory_model::checkpoint::CacheBlock;
use crate::QEMUPlugin;

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

    pub fn access_memory(&mut self, ts: usize, paddr: usize, is_store: bool) {
        let block_id = paddr >> 6;
        let res = self.private_cache.record(block_id, is_store, ts);
        match res {
            crate::cache::CacheReturnResult::Miss => {
                self.local_shared_cache.record(block_id, is_store, ts);
            }
            crate::cache::CacheReturnResult::Hit => {}
            crate::cache::CacheReturnResult::MissWithEviction(blk) => {
                self.local_shared_cache.record(blk, false, ts);
            }
            crate::cache::CacheReturnResult::MissWithWriteBack(blk) => {
                self.local_shared_cache.record(blk, true, ts);
            }
        }
    }

    pub fn get_serilized_private_cache(&self) -> SerializedCache {
        todo!();
    }
}

// this struct contains the memory model, basically the private .
pub struct TimestampMemoryHierarchy<
    const P_A: usize,
    const P_S: usize,
    const S_A: usize,
    const S_S: usize,
> {
    hierarchies: HashMap<u8, TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>>,
}

struct CacheLineSharingInfo {
    replicas: Vec<(usize, u8)>,
    last_writer: Option<(usize, u8)>,
}

impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    pub fn new(core_ids: Vec<u8>) -> Self {
        return TimestampMemoryHierarchy {
            hierarchies: HashMap::from_iter(
                core_ids
                    .into_iter()
                    .map(|i| return (i, TimestampSingleCoreMemoryHierarchy::new())),
            ),
        };
    }

    fn get_private_cache_line_sharing_info(&self) -> HashMap<usize, CacheLineSharingInfo> {
        let mut res = HashMap::<usize, CacheLineSharingInfo>::new();
        // go over all the cache lines in private cache and understand the permission.
        self.hierarchies.iter().for_each(|(core_id, cache)| {
            cache.private_cache.sets.iter().for_each(|set| {
                set.iter().for_each(|(block_id, data)| {
                    if res.contains_key(block_id) {
                        // well, this cache line is recorded. 
                        let record = res.get_mut(block_id).unwrap();
                        match data.is_dirty {
                            true => {
                                // if this is a dirty cache line, I need to check if this guy is the last writer.
                                match record.last_writer {
                                    Some((writer_ts, writer_core_id)) => {
                                        if writer_ts > data.ts {
                                            // well, there is no need to argue.
                                            assert_ne!(*core_id, writer_core_id);
                                        } else if writer_ts == data.ts {
                                            if writer_core_id != *core_id {
                                                // well, ambiguous case happened. 
                                                eprintln!(
                                                    "Ambiguous case detected: core {} and core {} are writing the same cache block (addr = {:x}) at the same time (ts = {})",
                                                    *core_id,
                                                    writer_core_id,
                                                    block_id * 64,
                                                    data.ts
                                                );
                                            } else {
                                                // This case is also not possible, because the same core cannot write the same place with the same ts.
                                                panic!("The same cache block is written twice by the same core at the same time, which is impossible");
                                            }
                                        } else {
                                            // then update the last writer, and invalid all earlier sharer.
                                            record.last_writer = Some((data.ts, *core_id));
                                        }
                                    },
                                    None => {
                                        // good chance, I will take control of this directory.
                                        record.last_writer = Some((data.ts, *core_id));
                                        // clean all the old writer. 
                                        record.replicas.retain(|(replica_ts, _)|{
                                            return *replica_ts > data.ts;
                                        });
                                    },
                                };
                            },
                            false => {
                                if record.replicas.iter().find(|x|{
                                    return x.1 == *core_id
                                }).is_some() {
                                    // it is impossible for the same thread to 
                                    panic!("A replica (block_id = {}, core_id = {}) is duplicated.", *block_id, *core_id);
                                }
                                // this is a read request. I just need to append this request to the record list.
                                record.replicas.push((data.ts, *core_id));
                            },
                        };
                    } else {
                        // fine, it is a new one, so put it there.
                        res.insert(
                            *block_id,
                            CacheLineSharingInfo {
                                replicas: vec![(data.ts, *core_id)],
                                last_writer: match data.is_dirty {
                                    true => Some((data.ts, *core_id)),
                                    false => None,
                                },
                            },
                        );
                    }
                })
            })
        });

        // clean the outdated replicas.
        res.iter_mut().for_each(|(block_id, entry)| {
            match entry.last_writer {
                Some((writer_ts, writer_core_id)) => {
                    // add a sanity check. It is impossible to have one core_id as both read and writer.
                    assert!(entry.replicas.iter().find(|el|{el.1 == writer_core_id}).is_none(), "it is impossible to have the same core being the reader and writer for a cache line (block_id = {})", block_id);
                    entry.replicas.retain(|(replica_ts, _)| {
                        // The way to handle the equal case is ambiguous.
                        return *replica_ts > writer_ts;
                    });
                }
                None => (),
            };
        });

        return res;
    }

    fn reconstruct_private_cache(
        &self,
        sharing_info: &HashMap<usize, CacheLineSharingInfo>,
    ) -> HashMap<u8, SerializedCache> {
        let mut private_caches: HashMap<_, _> = self
            .hierarchies
            .iter()
            .map(|(core_id, _)| {
                return (
                    *core_id,
                    Vec::from_iter((0..P_S).map(|_| Vec::<(usize, CacheBlock)>::new())),
                );
            })
            .collect();

        // the algorithm is to scan the directory and insert the data into the caches
        sharing_info
            .iter()
            .for_each(|(block_id, directory_entry)| {
                let set_number = (P_S - 1) & block_id;
                // 1. handle the writer
                match (
                    directory_entry.last_writer.is_some(),
                    directory_entry.replicas.is_empty(),
                ) {
                    (true, true) => {
                        // there is only one writer, so exclusive and modified.
                        let writer = directory_entry.last_writer.unwrap();
                        private_caches.get_mut(&writer.1).unwrap()[set_number].push((
                            writer.0,
                            CacheBlock {
                                block_id: *block_id,
                                perm: CacheBlockPermission::ModifiedExclusive,
                            },
                        ));
                    }
                    (true, false) => {
                        // the writer will have owned permission, and others will have shared permission.
                        // When the checkpoint is loaded into a machine without owned permission, the owned permission is exported as a shared state.
                        let writer = directory_entry.last_writer.unwrap();
                        private_caches.get_mut(&writer.1).unwrap()[set_number].push((
                            writer.0,
                            CacheBlock {
                                block_id: *block_id,
                                perm: CacheBlockPermission::ModifiedOwned,
                            },
                        ));
                        // all the replicas have the shared permission.
                        directory_entry.replicas.iter().for_each(
                            |(replica_ts, replica_core_id)| {
                                private_caches.get_mut(replica_core_id).unwrap()[set_number].push((
                                    *replica_ts,
                                    CacheBlock {
                                        block_id: *block_id,
                                        perm: CacheBlockPermission::CleanShared,
                                    },
                                ))
                            },
                        );
                    }
                    (false, true) => {
                        // there is no writer, and all requests are read-only
                        let perm = if directory_entry.replicas.len() == 1 {
                            CacheBlockPermission::CleanExclusive
                        } else {
                            CacheBlockPermission::CleanShared
                        };
                        directory_entry.replicas.iter().for_each(
                            |(replica_ts, replica_core_id)| {
                                private_caches.get_mut(replica_core_id).unwrap()[set_number].push((
                                    *replica_ts,
                                    CacheBlock {
                                        block_id: *block_id,
                                        perm,
                                    },
                                ))
                            },
                        );
                    }
                    (false, false) => {
                        // Amazing, then why do we even have this cache line???
                        panic!("A strange cache line (block_id = {}) is detected: It has no replicas and no writer.", block_id);
                    }
                };
            });

        // then, determine the cold blocks.
        let cold_block_count: HashMap<u8, Vec<usize>> = self
            .hierarchies
            .iter()
            .map(|(core_id, mh)| {
                return (
                    *core_id,
                    mh.private_cache
                        .sets
                        .iter()
                        .map(|s| {
                            return P_A - s.len();
                        })
                        .collect::<Vec<_>>(),
                );
            })
            .collect();

        // finally, build the result
        return private_caches
            .into_iter()
            .map(|(core_id, private_cache_primitive)| {
                return (
                    core_id,
                    private_cache_primitive
                        .into_iter()
                        .zip(cold_block_count[&core_id].iter())
                        .map(|(set, cold_lines)| {
                            let mut set = set;
                            set.sort_unstable_by(|(ts1, _), (ts2, _)| {
                                ts2.cmp(ts1) // cache line with larger ts should be kept.
                            });
                            if set.len() > P_A {
                                // this should never happen. 
                                set.drain(P_A..set.len());
                            } else {
                                // well, then we just left others to be empty.
                            }
                            let set: Vec<_> = set.into_iter().map(|x| x.1).collect();
                            return CacheSet::<CacheBlock> {
                                set,
                                untouched_blocks: *cold_lines,
                            };
                        })
                        .collect(),
                );
            })
            .collect();
    }

    

    pub fn reconstruct_moesi_per_llc(&mut self) -> MemoryHierarchyCheckPoint {
        // 1. scan all private caches and determine their shared info.
        let block_sharing_info = self.get_private_cache_line_sharing_info();

        return MemoryHierarchyCheckPoint {
            private_cache: self.reconstruct_private_cache(&block_sharing_info),
            shared_cache: todo!(),
            directory: todo!(),
        };
    }
}

unsafe impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize> QEMUPlugin
    for TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    unsafe fn on_translation(
        &mut self,
        tb: &crate::plugin::QEMUPluginBasicBlock,
    ) -> Vec<*mut std::ffi::c_void> {
        todo!()
    }

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

    unsafe fn on_qemu_exit(&mut self) {
        todo!()
    }
}
