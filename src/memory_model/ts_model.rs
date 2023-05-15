use std::collections::HashMap;

use super::checkpoint::CacheBlockPermission;
use super::checkpoint::MemoryHierarchyCheckPoint;
use super::checkpoint::SerializedCache;
use crate::cache::ts_cache::TimestampCache;
use crate::cache::ts_cache::TimestampCacheMetaData;

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

// this struct contains the memory model of this structure.
pub struct TimestampMemoryHierarchy<
    const P_A: usize,
    const P_S: usize,
    const S_A: usize,
    const S_S: usize,
> {
    hierarchies: HashMap<u8, TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>>,
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

    pub fn reconstruct_mesi_per_llc(&mut self) -> MemoryHierarchyCheckPoint {
        // 1. scan all the content in the private cache and reconstruct the directory.
        struct DirectoryEntry {
            replicas: Vec<(usize, u8)>,
            last_writer: Option<(usize, u8)>,
        }

        let mut block_sharing_info = HashMap::<usize, DirectoryEntry>::new();

        // go over all the cache lines in private cache and understand the permission.
        self.hierarchies.iter().for_each(|(core_id, cache)| {
            cache.private_cache.sets.iter().for_each(|set| {
                set.iter().for_each(|(block_id, data)| {
                    if block_sharing_info.contains_key(block_id) {
                        // well, this cache line is recorded. 
                        let record = block_sharing_info.get_mut(block_id).unwrap();
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
                                        // TODO: Move this step after constructing the cache.
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
                        block_sharing_info.insert(
                            *block_id,
                            DirectoryEntry {
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
        block_sharing_info.iter_mut().for_each(|(block_id, entry)| {
            match entry.last_writer {
                Some((writer_ts, writer_core_id)) => {
                    entry.replicas.retain(|(replica_ts, replica_core_id)| {
                        // how to handle the equal case is ambiguous.
                        return *replica_ts > writer_ts;
                    });
                }
                None => (),
            };
        });

        // now, reconstruct the caches with the content in the directory.
        // the algorithm is to scan the directory and insert the data into the caches
        let mut private_caches: HashMap<usize, Vec<Vec<(usize, CacheBlock)>>> = HashMap::new();

        block_sharing_info.iter().for_each(|(block_id, data)| {
            // 1. handle the writer
            match (data.last_writer.is_some(), data.replicas.is_empty()) {
                (true, true) => {
                    // there is only one writer, so exclusive and modified.
                    
                },
                (true, false) => todo!(),
                (false, true) => todo!(),
                (false, false) => todo!(),
            };
        });

        return MemoryHierarchyCheckPoint {
            private_cache: todo!(),
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
