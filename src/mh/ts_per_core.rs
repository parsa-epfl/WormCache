use std::sync::Barrier;

use crate::{cache::TimestampCache, mh::pbb_metadata::PBBMetadata, plugin::QEMUPluginPerCoreActor};
use crossbeam_channel::{Receiver, Sender};

use super::{quantum::QuantumManager, ts_model::TimestampMemoryHierarchy};

#[derive(Debug)]
pub struct TimestampSingleCoreMemoryHierarchy<
    const P_A: usize, // associativity of the private cache
    const P_S: usize, // set number of the private cache
    const S_A: usize, // associativity of the shared cache
    const S_S: usize, // set number of the shared cache
> {
    pub private_cache: TimestampCache<P_A, P_S>,
    pub local_shared_cache: TimestampCache<S_A, S_S>,

    // icount for each core
    pub i_count: usize,

    // This variable will be added to i_count so that the instructions from last pBB increases the i_count.
    // By doing this, we don't have to instrument every instruction to increase the i_count.
    pub i_count_from_last_pbb: usize,

    pub quantum_budget: usize,
}

impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    pub fn new() -> Self {
        return Self {
            private_cache: TimestampCache::new(),
            local_shared_cache: TimestampCache::new(),
            i_count: 0,
            i_count_from_last_pbb: 0,
            quantum_budget: crate::QUAMTUM,
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
            // TODO: We don't know the permission of evicted caches.
            crate::cache::CacheReturnResult::MissWithEviction(blk, is_instruction) => {
                self.local_shared_cache
                    .record(blk, is_instruction, false, ts);
            }
            crate::cache::CacheReturnResult::MissWithWriteBack(blk) => {
                self.local_shared_cache
                    .record(blk, false, true, ts);
            }
        }
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
        quantum_manager: &QuantumManager,
    ) {
        // First of all, update the i_count due to the advancement of the last instruction.
        self.i_count += self.i_count_from_last_pbb;

        if self.quantum_budget >= self.i_count_from_last_pbb {
            self.quantum_budget -= self.i_count_from_last_pbb;
        } else {
            // DMN, we have to wait, and then update the quantum.
            // Set this flag so that other thread know you are doing some synchronization.
            crate::qemu_api::qemu_plugin_set_running_flag(false);
            quantum_manager.vcpu_wait();

            // Release this flag now.
            crate::qemu_api::qemu_plugin_set_running_flag(true);


            // After the barrier, we then update the quantum.
            self.quantum_budget += crate::QUAMTUM;
            self.quantum_budget -= self.i_count_from_last_pbb;
        }

        // Now, get the metadata of this turn.
        let metadata = PBBMetadata::from(metadata as usize);
        self.i_count_from_last_pbb = metadata.instruction_count as usize;

        // Update the cache by instruction access.
        self.access_memory(self.i_count, metadata.physical_addr, true, false);
    }

    unsafe fn on_memory_access(
        &mut self,
        cpu_idx: u32,
        info: &crate::plugin::QEMUMemoryInfo,
        vaddr: u64,
        user_data: *mut std::ffi::c_void,
        quantum_manager: &QuantumManager,
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
            }
        }
    }
}
