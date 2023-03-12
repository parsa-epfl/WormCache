mod cache;
mod qemu_api;
use qemu_api::*;
mod plugin;
use std::ffi;

pub unsafe trait QEMUPlugin {
    unsafe fn on_translation(&mut self, tb: *mut qemu_api::qemu_plugin_tb)
        -> Vec<*mut ffi::c_void>;
    unsafe fn on_instruction_execution(&mut self, cpu_idx: u32, user_data: *mut ffi::c_void);
    unsafe fn on_memory_access(
        &mut self,
        cpu_idx: u32,
        info: qemu_api::qemu_plugin_meminfo_t,
        vaddr: u64,
        user_data: *mut ffi::c_void,
    );
    unsafe fn on_qemu_exit(&mut self);
}

use once_cell::sync::Lazy;

static mut PLUGIN: Lazy<plugin::first_touch::FirstTouchCounterPlugin> = Lazy::new(plugin::first_touch::FirstTouchCounterPlugin::new);

#[no_mangle]
pub static qemu_plugin_version: u32 = QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn vcpu_mem_access(
    vcpu_index: u32,
    info: qemu_plugin_meminfo_t,
    vaddr: u64,
    user_data: *mut ffi::c_void, // should be NULL.
) {
    PLUGIN.on_memory_access(vcpu_index, info, vaddr, user_data);
}

#[no_mangle]
unsafe extern "C" fn vcpu_insn_exec(
    vcpu_index: u32,
    user_data: *mut ffi::c_void, // it is basically its physical address.
) {
    PLUGIN.on_instruction_execution(vcpu_index, user_data);
}

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    id: qemu_api::qemu_plugin_id_t,
    tb: *mut qemu_api::qemu_plugin_tb,
) {
    let metadata = PLUGIN.on_translation(tb);
    let n_instructions = qemu_plugin_tb_n_insns(tb);

    for i in 0..n_instructions {
        let instruction = qemu_plugin_tb_get_insn(tb, i);
        let hva = qemu_plugin_insn_haddr(instruction);
        // insert the normal instruction
        qemu_plugin_register_vcpu_mem_cb(
            instruction,
            Some(vcpu_mem_access),
            qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
            qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
            metadata[i],
        );
        qemu_plugin_register_vcpu_insn_exec_cb(
            instruction,
            Some(vcpu_insn_exec),
            qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
            metadata[i],
        );
    }
}

#[no_mangle]
unsafe extern "C" fn plugin_exit(id: qemu_api::qemu_plugin_id_t, p: *mut ffi::c_void) {
    PLUGIN.on_qemu_exit();
}

#[no_mangle]
unsafe extern "C" fn qemu_plugin_install(
    id: qemu_api::qemu_plugin_id_t,
    info: *const qemu_api::qemu_info_t,
    argc: i32,
    argv: *const *const u8,
) -> i32 {
    qemu_plugin_register_vcpu_tb_trans_cb(id, Some(vcpu_tb_trans));
    qemu_plugin_register_atexit_cb(id, Some(plugin_exit), std::ptr::null_mut());

    std::thread::spawn(||{
        loop {
            println!("utilization: {}", PLUGIN.current_usage());
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });

    return 0;
}
