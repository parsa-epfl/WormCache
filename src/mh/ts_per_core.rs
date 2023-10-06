use crate::{cache::TimestampCache, mh::pbb_metadata::PBBMetadata, plugin::QEMUPluginPerCoreActor};

use super::ts_model::TimestampMemoryHierarchy;

#[derive(Debug)]
pub struct TimestampSingleCoreMemoryHierarchy<
    const P_A: usize, // associativity of the private cache
    const P_S: usize, // set number of the private cache
    const S_A: usize, // associativity of the shared cache
    const S_S: usize, // set number of the shared cache
> {
    pub private_cache: TimestampCache<P_A, P_S>,
    pub local_shared_cache: TimestampCache<S_A, S_S>,

    // icount for each core, for the time calculation.
    pub i_count: usize,
}

impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    pub fn new() -> Self {
        return Self {
            private_cache: TimestampCache::new(),
            local_shared_cache: TimestampCache::new(),
            i_count: 0,
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
            crate::cache::CacheReturnResult::MissWithEviction(blk, is_instruction) => {
                self.local_shared_cache
                    .record(blk, is_instruction, false, ts);
            }
            crate::cache::CacheReturnResult::MissWithWriteBack(blk) => {
                self.local_shared_cache.record(blk, false, true, ts);
            }
        }
    }

    pub fn get_icount(&self) -> usize {
        return self.i_count;
    }
}

unsafe impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    QEMUPluginPerCoreActor for TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    type PluginType = TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>;

    unsafe fn on_instruction_execution(
        &mut self,
        cpu_idx: u32,
        metadata: *mut std::ffi::c_void,
    ) {
        // First of all, update the i_count due to the advancement of the last instruction.

        // Now, get the metadata of this turn.
        let metadata = PBBMetadata::from(metadata as usize);
        self.i_count += metadata.instruction_count as usize;

        // Update the cache by instruction access.
        self.access_memory(self.i_count, metadata.physical_addr, true, false);
    }

    unsafe fn on_memory_access(
        &mut self,
        cpu_idx: u32,
        info: &crate::plugin::QEMUMemoryInfo,
        vaddr: u64,
        user_data: *mut std::ffi::c_void,
    ) {
        let bias = user_data as usize;
        match info.translate(vaddr) {
            Some(pa) => {
                self.access_memory(
                    self.i_count + bias,
                    pa as usize,
                    false,
                    info.is_store_operation(),
                );
            }
            None => {
                // This is a device access, thus ignored.
                // TODO: We should handle this case if the device is accessing the cache.
            }
        }
    }
}
