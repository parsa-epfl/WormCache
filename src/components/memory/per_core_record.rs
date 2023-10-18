use super::ts_cache::TimestampCache;

#[derive(Debug)]
#[repr(align(64))]
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
        let block_id = paddr >> crate::CACHE_LINE_SIZE.trailing_zeros();
        let res = self
            .private_cache
            .record(block_id, is_instruction, is_store, ts);
        match res {
            super::CacheReturnResult::Miss => {
                self.local_shared_cache
                    .peek(block_id, is_instruction, is_store, ts);
            }
            super::CacheReturnResult::Hit => {}
            super::CacheReturnResult::MissWithEviction(blk, is_instruction) => {
                self.local_shared_cache
                    .record(blk, is_instruction, false, ts);
            }
            super::CacheReturnResult::MissWithWriteBack(blk) => {
                self.local_shared_cache.record(blk, false, true, ts);
            }
        }
    }
}

// unsafe impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
//     QEMUPluginPerCoreActor for TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>
// {
//     type PluginType = TimestampMemoryHierarchy<P_A, P_S, S_A, S_S>;

//     unsafe fn on_instruction_execution(
//         &mut self,
//         cpu_idx: u32,
//         metadata: *mut std::ffi::c_void,
//     ) {
//         // Update the icount for the time calculation.
//         let metadata = PBBMetadata::from(metadata as usize);
//         self.i_count += metadata.instruction_count as usize;

//         // Update the cache by instruction access.
//         self.access_memory(crate::get_real_time() as usize, metadata.physical_addr, true, false);
//     }

//     unsafe fn on_memory_access(
//         &mut self,
//         cpu_idx: u32,
//         info: &crate::plugin::QEMUMemoryInfo,
//         vaddr: u64,
//         user_data: *mut std::ffi::c_void,
//     ) {
//         match info.translate(vaddr) {
//             Some(pa) => {
//                 self.access_memory(
//                     crate::get_real_time() as usize,
//                     pa as usize,
//                     false,
//                     info.is_store_operation(),
//                 );
//             }
//             None => {
//                 // This is a device access, thus ignored.
//                 // TODO: We should handle this case if the device is accessing the cache.
//             }
//         }
//     }
// }
