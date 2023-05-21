use crate::cache::single::CacheMetaData;
use crate::cache::single::PrivateCache;
use crate::cache::CacheReturnResult;
use crate::QEMUPlugin;

use std::collections::HashMap;
use std::fs;
use std::io::prelude::*;

use serde_json;

use super::PerInstructionInstrumentation;
use super::QEMUMemoryInfo;
use super::QEMUPluginBasicBlock;

#[derive(Clone, Copy)]
struct CacheMetaWithType {
    dirty: bool,
    is_instruction: bool,
}

impl CacheMetaData for CacheMetaWithType {
    fn is_dirty(&self) -> bool {
        return self.dirty;
    }

    fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }
}

pub struct LLCSetAccessDistributionPlugin {
    private_cache: PrivateCache<16, 2048, CacheMetaWithType>,
    llc_counter: Vec<HashMap<usize, (usize, bool)>>,
}

const LLC_SET: usize = 1024 * 1024;

impl LLCSetAccessDistributionPlugin {
    pub fn new() -> Self {
        return LLCSetAccessDistributionPlugin {
            private_cache: PrivateCache::new(),
            llc_counter: (0..LLC_SET)
                .map(|_| {
                    return HashMap::new();
                })
                .collect(),
        };
    }

    pub fn increase_llc_counter(&mut self, block_id: usize, is_instruction: bool) {
        match self.llc_counter[block_id % LLC_SET].get_mut(&block_id) {
            Some(freq) => {
                freq.0 += 1;
                freq.1 = is_instruction | freq.1; // once a block is touched by instruction, it will become a instruction block.
            },
            None => {
                self.llc_counter[block_id % LLC_SET].insert(block_id, (1, is_instruction));
            }
        };
    }
}

unsafe impl QEMUPlugin for LLCSetAccessDistributionPlugin {
    unsafe fn on_translation(
        &mut self,
        tb: &QEMUPluginBasicBlock,
    ) -> Vec<PerInstructionInstrumentation> {
        return tb
            .iter()
            .enumerate()
            .map(|(idx, x)| {
                let pc = x.physical_address();
                let pc_as_param = pc as *mut std::ffi::c_void;
                if idx == 0 {
                    return PerInstructionInstrumentation {
                        instruction_execution: Some(pc as *mut std::ffi::c_void),
                        memory_access: Some(pc_as_param),
                    };
                } else {
                    if (pc % 64) == 0 {
                        // only insert when there it crosses the cache line
                        return PerInstructionInstrumentation {
                            instruction_execution: Some(pc as *mut std::ffi::c_void),
                            memory_access: Some(pc_as_param),
                        };
                    } else {
                        return PerInstructionInstrumentation {
                            instruction_execution: None,
                            memory_access: Some(pc_as_param),
                        };
                    }
                }
            })
            .collect();
    }

    unsafe fn on_instruction_execution(&mut self, cpu_idx: u32, user_data: *mut std::ffi::c_void) {
        if cpu_idx != 1 {
            return;
        }

        let addr = user_data as usize;
        let block_id = addr >> 6;

        match self.private_cache.update(
            block_id,
            CacheMetaWithType {
                dirty: false,
                is_instruction: true,
            },
        ) {
            crate::cache::single::SingleCacheResult::Hit => {},
            crate::cache::single::SingleCacheResult::Miss => self.increase_llc_counter(block_id, true),
            crate::cache::single::SingleCacheResult::MissAndEvicted(evicted_block_id, meta_data) => {
                self.increase_llc_counter(block_id, true);
                self.increase_llc_counter(evicted_block_id, meta_data.is_instruction);
            },
        }
    }

    unsafe fn on_memory_access(
        &mut self,
        cpu_idx: u32,
        info: &QEMUMemoryInfo,
        vaddr: u64,
        user_data: *mut std::ffi::c_void,
    ) {
        if cpu_idx != 1 {
            return;
        }

        let addr = info.translate(vaddr);
        if addr.is_none() {
            return;
        }
        let addr = addr.unwrap() as usize;
        let block_id = addr >> 6;

        match self.private_cache.update(
            block_id,
            CacheMetaWithType {
                dirty: info.is_store_operation(),
                is_instruction: false,
            },
        ) {
            crate::cache::single::SingleCacheResult::Hit => {},
            crate::cache::single::SingleCacheResult::Miss => self.increase_llc_counter(block_id, false),
            crate::cache::single::SingleCacheResult::MissAndEvicted(evicted_block_id, meta_data) => {
                self.increase_llc_counter(block_id, false);
                self.increase_llc_counter(evicted_block_id, meta_data.is_instruction);
            },
        }
    }

    unsafe fn on_qemu_exit(&mut self) {
        // now, all the statistically saved.
        let mut llc_counters = fs::File::create("llc_counter.json").unwrap();
        llc_counters
            .write(
                serde_json::to_string_pretty(&self.llc_counter)
                    .unwrap()
                    .as_bytes(),
            )
            .unwrap();
        llc_counters.flush().unwrap();
    }
}
