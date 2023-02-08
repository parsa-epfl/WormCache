mod qemu_plugin;
mod warmup;
use qemu_plugin::*;
use std::ffi;
use warmup::WarmupLatencyCache;

use once_cell::sync::Lazy;
use std::collections::HashMap;

static mut TRANSLATIONS: Lazy<HashMap<usize, usize>> = Lazy::new(|| return HashMap::new());

const SET_COUNT: usize = 16 * 1024;

static mut WARM_UP_CACHE: Lazy<WarmupLatencyCache<16, SET_COUNT>> = Lazy::new(|| {
    return warmup::WarmupLatencyCache::new();
});

const WORKLOAD_CPU: u32 = 1;

#[no_mangle]
pub static qemu_plugin_version: u32 = QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn vcpu_mem_access(
    vcpu_index: u32,
    info: qemu_plugin_meminfo_t,
    vaddr: u64,
    user_data: *mut ffi::c_void, // should be NULL.
) {
    if vcpu_index == WORKLOAD_CPU {
        let hva = qemu_plugin_get_hwaddr(info, vaddr) as usize;
        WARM_UP_CACHE.update(hva, false);
    }
}

#[no_mangle]
unsafe extern "C" fn vcpu_insn_exec(
    vcpu_index: u32,
    user_data: *mut ffi::c_void, // it is basically its physical address.
) {
    let hva = user_data as usize;
    if vcpu_index == WORKLOAD_CPU {
        WARM_UP_CACHE.update(hva, true);
    }
}

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    id: qemu_plugin::qemu_plugin_id_t,
    tb: *mut qemu_plugin::qemu_plugin_tb,
) {
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
            std::ptr::null_mut(),
        );
        qemu_plugin_register_vcpu_insn_exec_cb(
            instruction, 
            Some(vcpu_insn_exec), 
            qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS, 
            hva as *mut ffi::c_void
        );
    }
}

#[no_mangle]
unsafe extern "C" fn plugin_exit(id: qemu_plugin::qemu_plugin_id_t, p: *mut ffi::c_void) {}

#[no_mangle]
unsafe extern "C" fn qemu_plugin_install(
    id: qemu_plugin::qemu_plugin_id_t,
    info: *const qemu_plugin::qemu_info_t,
    argc: i32,
    argv: *const *const u8,
) -> i32 {
    qemu_plugin_register_vcpu_tb_trans_cb(id, Some(vcpu_tb_trans));
    qemu_plugin_register_atexit_cb(id, Some(plugin_exit), std::ptr::null_mut());

    return 0;
}
