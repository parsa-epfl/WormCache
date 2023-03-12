use crate::cache::single::PrivateCache;
use crate::qemu_api;
use crate::QEMUPlugin;
use std::collections::HashMap;
use std::ffi::c_void;

use super::QEMUMemoryInfo;
use super::QEMUPluginBasicBlock;

#[derive(Clone, Debug)]
pub struct SingleCoreCacheStatistics {
    pub instructions: usize,
    pub l1i_miss: usize,
    pub l1d_miss: usize,
    pub l1d_wb: usize,
}

pub struct SingleCoreCachePlugin {
    l1i: PrivateCache,
    l1d: PrivateCache,
    llc_counter: Vec<usize>,

    // counters
    c: SingleCoreCacheStatistics,
}

const L1Associativity: usize = 8;
const L1Set: usize = 128;
const LLCSet: usize = 1024 * 64;

impl SingleCoreCachePlugin {
    pub fn new() -> Self {
        return SingleCoreCachePlugin {
            l1i: PrivateCache::new(L1Set, L1Associativity),
            l1d: PrivateCache::new(L1Set, L1Associativity),
            llc_counter: (0..LLCSet)
                .map(|_| {
                    return 0;
                })
                .collect(),
            c: SingleCoreCacheStatistics {
                instructions: 0,
                l1i_miss: 0,
                l1d_miss: 0,
                l1d_wb: 0,
            },
        };
    }

    pub fn statistics(&self) -> SingleCoreCacheStatistics {
        return self.c.clone();
    }
}

unsafe impl QEMUPlugin for SingleCoreCachePlugin {
    unsafe fn on_translation(&mut self, tb: &QEMUPluginBasicBlock) -> Vec<*mut std::ffi::c_void> {
        return tb
            .iter()
            .map(|i| i.physical_address() as *mut std::ffi::c_void)
            .collect();
    }

    unsafe fn on_instruction_execution(&mut self, cpu_idx: u32, user_data: *mut std::ffi::c_void) {
        if cpu_idx != 1 {
            return;
        }

        self.c.instructions += 1;

        let addr = user_data as usize;
        let block_id = addr >> 6;

        match self.l1i.update(block_id, false) {
            crate::cache::CacheReturnResult::Miss => {
                self.llc_counter[block_id % LLCSet] += 1;
                self.c.l1i_miss += 1;
            }
            crate::cache::CacheReturnResult::Hit => {}
            crate::cache::CacheReturnResult::MissWithEviction(_) => {
                self.llc_counter[block_id % LLCSet] += 1;
                self.c.l1i_miss += 1;
            }
            crate::cache::CacheReturnResult::MissWithDirtyEviction(_) => {
                panic!("This case should not happen!");
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

        match self.l1d.update(addr, info.is_store_operation()) {
            crate::cache::CacheReturnResult::Miss => {
                self.llc_counter[block_id % LLCSet] += 1;
                self.c.l1d_miss += 1;
            }
            crate::cache::CacheReturnResult::Hit => {}
            crate::cache::CacheReturnResult::MissWithEviction(_) => {
                self.llc_counter[block_id % LLCSet] += 1;
                self.c.l1d_miss += 1;
            }
            crate::cache::CacheReturnResult::MissWithDirtyEviction(write_back_block_id) => {
                self.llc_counter[block_id % LLCSet] += 1;
                self.llc_counter[write_back_block_id % LLCSet] += 1;
                self.c.l1d_wb += 1;
            }
        }
    }

    unsafe fn on_qemu_exit(&mut self) {}
}
