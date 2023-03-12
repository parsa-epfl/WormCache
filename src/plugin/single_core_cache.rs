use crate::QEMUPlugin;
use crate::qemu_api;
use crate::cache::single::PrivateCache;
use std::collections::HashMap;

use super::QEMUMemoryInfo;
use super::QEMUPluginBasicBlock;

struct SingleCoreCachePlugin {
    l1i: PrivateCache,
    l1d: PrivateCache,
    llc_counter: HashMap<usize, usize>
}

unsafe impl QEMUPlugin for SingleCoreCachePlugin {
    unsafe fn on_translation(&mut self, tb: &QEMUPluginBasicBlock) -> Vec<*mut std::ffi::c_void> {
        todo!()
    }

    unsafe fn on_instruction_execution(&mut self, cpu_idx: u32, user_data: *mut std::ffi::c_void) {
        todo!()
    }

    unsafe fn on_memory_access(&mut self, cpu_idx: u32, info: &QEMUMemoryInfo, vaddr: u64, user_data: *mut std::ffi::c_void) {
        todo!()
    }

    unsafe fn on_qemu_exit(&mut self) {
        todo!()
    }
}