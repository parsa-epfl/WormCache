use std::ffi;

pub mod single_core_cache;
pub mod first_touch;
pub mod qemu_wrapper;

// Re-export the QEMU API
pub use qemu_wrapper::QEMUPluginBasicBlock;
pub use qemu_wrapper::QEMUPluginBasicBlockIterator;
pub use qemu_wrapper::QEMUPluginInstruction;
pub use qemu_wrapper::QEMUMemoryInfo;

pub unsafe trait QEMUPlugin {
    unsafe fn on_translation(&mut self, tb: &QEMUPluginBasicBlock)
        -> Vec<*mut ffi::c_void>;
    unsafe fn on_instruction_execution(&mut self, cpu_idx: u32, user_data: *mut ffi::c_void);
    unsafe fn on_memory_access(
        &mut self,
        cpu_idx: u32,
        info: &QEMUMemoryInfo,
        vaddr: u64,
        user_data: *mut ffi::c_void,
    );
    unsafe fn on_qemu_exit(&mut self);
}
