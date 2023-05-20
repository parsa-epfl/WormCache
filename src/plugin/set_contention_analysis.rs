use crate::cache::single::PrivateCache;
use crate::cache::CacheReturnResult;
use crate::QEMUPlugin;

use std::collections::HashMap;
use std::fs;
use std::io::prelude::*;

use super::PerInstructionInstrumentation;
use super::QEMUMemoryInfo;
use super::QEMUPluginBasicBlock;

pub struct LLCSetAccessDistributionPlugin {
    private_cache: PrivateCache<16, 2048>,
    llc_counter: Vec<HashMap<usize, usize>>,
}

const LLC_SET: usize = 1024 * 64;

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

    pub fn increaseLLCCounter(&mut self, block_id: usize) {

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
                        memory_access: Some(pc_as_param)
                    };
                } else {
                    if (pc % 64) == 0 {
                        // only insert when there it crosses the cache line
                        return PerInstructionInstrumentation {
                            instruction_execution: Some(pc as *mut std::ffi::c_void),
                            memory_access: Some(pc_as_param)
                        };
                    } else {
                        return PerInstructionInstrumentation {
                            instruction_execution: None,
                            memory_access: Some(pc_as_param)
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

        match self.private_cache.update(block_id, false) {
            CacheReturnResult::Miss => {
                self.increaseLLCCounter(block_id);
            }
            CacheReturnResult::Hit => {}
            CacheReturnResult::MissWithEviction(eviction_block_id) => {
                self.increaseLLCCounter(block_id);
                self.increaseLLCCounter(eviction_block_id);
            }
            CacheReturnResult::MissWithWriteBack(write_back_block_id) => {
                self.increaseLLCCounter(block_id);
                self.increaseLLCCounter(write_back_block_id);
            }
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

        match self.private_cache.update(block_id, false) {
            CacheReturnResult::Miss => {
                self.increaseLLCCounter(block_id);
            }
            CacheReturnResult::Hit => {}
            CacheReturnResult::MissWithEviction(eviction_block_id) => {
                self.increaseLLCCounter(block_id);
                self.increaseLLCCounter(eviction_block_id);
            }
            CacheReturnResult::MissWithWriteBack(write_back_block_id) => {
                self.increaseLLCCounter(block_id);
                self.increaseLLCCounter(write_back_block_id);
            }
        }
    }

    unsafe fn on_qemu_exit(&mut self) {
        // now, all the statistically saved.
        let mut llc_counters = fs::File::create("llc_counter.log").unwrap();
        for set_contention in self.llc_counter.iter() {
        }
        llc_counters.flush().unwrap();
    }
}
