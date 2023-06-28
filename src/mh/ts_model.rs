use std::collections::HashMap;

use crate::cache::TimestampCache;
use crate::checkpoint::CacheBlockState;
use crate::checkpoint::DirectoryBlock;
use crate::checkpoint::MemoryHierarchyCheckPoint;
use crate::checkpoint::SerializedCache;
use crate::checkpoint::SerializedDirectory;

use crate::cache::ts_cache::TimestampCacheMetaData;
use crate::checkpoint::CacheBlock;
use crate::plugin::PerInstructionInstrumentation;
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

    pub fn access_memory(&mut self, ts: usize, paddr: usize, is_instruction: bool, is_store: bool) {
        let block_id = paddr >> 6;
        let res = self
            .private_cache
            .record(block_id, is_instruction, is_store, ts);
        match res {
            crate::cache::CacheReturnResult::Miss => {
                self.local_shared_cache
                    .peek(block_id, is_instruction, is_store, ts);
            }
            crate::cache::CacheReturnResult::Hit => {}
            crate::cache::CacheReturnResult::MissWithEviction(blk) => {
                self.local_shared_cache
                    .record(blk, is_instruction, false, ts);
            }
            crate::cache::CacheReturnResult::MissWithWriteBack(blk) => {
                self.local_shared_cache
                    .record(blk, is_instruction, true, ts);
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

#[derive(Clone)]
struct CacheLineSharingInfo {
    replicas: Vec<(usize, u8)>,
    last_writer: Option<(usize, u8)>,
    ts: usize,
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
}

unsafe impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize> QEMUPlugin
    for TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    unsafe fn on_translation(
        &mut self,
        tb: &crate::plugin::QEMUPluginBasicBlock,
    ) -> Vec<PerInstructionInstrumentation> {
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
