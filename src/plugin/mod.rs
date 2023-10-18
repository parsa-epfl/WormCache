use std::ffi;

pub mod qemu_wrapper;

// Re-export the QEMU API
pub use qemu_wrapper::QEMUPluginBasicBlock;
pub use qemu_wrapper::QEMUPluginBasicBlockIterator;
pub use qemu_wrapper::QEMUPluginInstruction;
pub use qemu_wrapper::QEMUMemoryInfo;


#[derive(Debug)]
pub struct PerInstructionInstrumentation {
    pub instruction_execution: Option<*mut std::ffi::c_void>,
    pub memory_access: Option<*mut std::ffi::c_void>
}

pub unsafe trait QEMUPluginPerCoreActor {

    type PluginType: QEMUPlugin<PerCorePlugin = Self>;

    unsafe fn on_instruction_execution(
        &mut self, 
        cpu_idx: u32, 
        user_data: *mut ffi::c_void
    );
    
    unsafe fn on_memory_access(
        &mut self,
        cpu_idx: u32,
        info: &QEMUMemoryInfo,
        vaddr: u64,
        user_data: *mut ffi::c_void
    );
}

// This one should have access to the Quantum server for synchronization and private data submission.
pub unsafe trait QEMUPlugin {

    type PerCorePlugin: QEMUPluginPerCoreActor;

    // Please use concurrency hashmap if possible.
    unsafe fn on_translation(&self, tb: &QEMUPluginBasicBlock)
        -> Vec<PerInstructionInstrumentation>;
    
    unsafe fn on_qemu_exit(&self);
}

