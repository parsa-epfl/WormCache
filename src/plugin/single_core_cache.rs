use crate::cache::single::CacheMetaData;
use crate::cache::single::PrivateCache;
use crate::cache::single::SingleCacheResult;
use crate::cache::CacheReturnResult;
use crate::QEMUPlugin;

use std::fs;
use std::io::prelude::*;

use super::PerInstructionInstrumentation;
use super::QEMUMemoryInfo;
use super::QEMUPluginBasicBlock;

#[derive(Clone, Debug)]
pub struct SingleCoreCacheStatistics {
    pub instructions: usize,
    pub l1i_miss: usize,
    pub l1d_miss: usize,
    pub l1d_wb: usize,
}

#[derive(Clone, Copy)]
struct NormalCacheMetadata {
    dirty: bool,
}

impl CacheMetaData for NormalCacheMetadata {
    fn is_dirty(&self) -> bool {
        return self.dirty;
    }

    fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty
    }
}

pub struct SingleCoreCachePlugin {
    last_iblock: usize,
    l1i: PrivateCache<8, 1024, NormalCacheMetadata>,
    l1d: PrivateCache<8, 1024, NormalCacheMetadata>,
    llc_counter: Vec<usize>,

    // counters
    c: SingleCoreCacheStatistics,
}

const LLC_SET: usize = 1024 * 64;

impl SingleCoreCachePlugin {
    pub fn new() -> Self {
        return SingleCoreCachePlugin {
            last_iblock: 0,
            l1i: PrivateCache::new(),
            l1d: PrivateCache::new(),
            llc_counter: (0..LLC_SET)
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
    unsafe fn on_translation(
        &mut self,
        tb: &QEMUPluginBasicBlock,
    ) -> Vec<PerInstructionInstrumentation> {
        return tb
            .iter()
            .map(|x| {
                return PerInstructionInstrumentation {
                    instruction_execution: Some(x.physical_address() as *mut std::ffi::c_void),
                    memory_access: Some(x.physical_address() as *mut std::ffi::c_void),
                };
            })
            .collect();
    }

    unsafe fn on_instruction_execution(&mut self, cpu_idx: u32, user_data: *mut std::ffi::c_void) {
        if cpu_idx != 1 {
            return;
        }

        self.c.instructions += 1;

        let addr = user_data as usize;
        let block_id = addr >> 6;

        if self.last_iblock == block_id {
            // filter some process
            return;
        }

        self.last_iblock = block_id;

        match self
            .l1i
            .update(block_id, NormalCacheMetadata { dirty: false })
        {
            SingleCacheResult::Miss => {
                self.llc_counter[block_id % LLC_SET] += 1;
                self.c.l1i_miss += 1;
            }
            SingleCacheResult::Hit => {}
            SingleCacheResult::MissAndEvicted(evicted_block_id, medadata) => {
                self.llc_counter[block_id % LLC_SET] += 1;
                self.c.l1i_miss += 1;
                assert!(!medadata.is_dirty());
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

        match self.l1d.update(
            addr,
            NormalCacheMetadata {
                dirty: info.is_store_operation(),
            },
        ) {
            SingleCacheResult::Miss => {
                self.llc_counter[block_id % LLC_SET] += 1;
                self.c.l1d_miss += 1;
            }
            SingleCacheResult::Hit => {}
            SingleCacheResult::MissAndEvicted(evicted_block_id, meta_data) => {
                if meta_data.is_dirty() {
                    self.llc_counter[block_id % LLC_SET] += 1;
                    self.llc_counter[evicted_block_id % LLC_SET] += 1;
                    self.c.l1d_wb += 1;
                } else {
                    self.llc_counter[block_id % LLC_SET] += 1;
                    self.llc_counter[evicted_block_id % LLC_SET] += 1;
                    self.c.l1d_miss += 1;
                }
            }
        }
    }

    unsafe fn on_qemu_exit(&mut self) {
        // now, all the statistically saved.
        let mut llc_counters = fs::File::create("llc_counter.log").unwrap();
        for cnt in self.llc_counter.iter() {
            llc_counters.write_fmt(format_args!("{} ", cnt)).unwrap();
        }
        llc_counters.flush().unwrap();
    }
}
