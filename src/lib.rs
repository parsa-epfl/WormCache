pub mod cache;
pub mod memory_model;
mod qemu_api;
use qemu_api::*;
mod plugin;
use chrono::Local;
use plugin::single_core_cache::{SingleCoreCachePlugin, SingleCoreCacheStatistics};
use plugin::QEMUPlugin;
use std::ffi;
use std::io::prelude::*;

use once_cell::sync::Lazy;

static mut PLUGIN: Lazy<SingleCoreCachePlugin> = Lazy::new(SingleCoreCachePlugin::new);

#[no_mangle]
pub static qemu_plugin_version: u32 = QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn vcpu_mem_access(
    vcpu_index: u32,
    info: qemu_plugin_meminfo_t,
    vaddr: u64,
    user_data: *mut ffi::c_void, // should be NULL.
) {
    PLUGIN.on_memory_access(vcpu_index, &plugin::QEMUMemoryInfo(info), vaddr, user_data);
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
    let wrapped_tb = plugin::QEMUPluginBasicBlock(tb);
    let metadata = PLUGIN.on_translation(&wrapped_tb);
    wrapped_tb
        .into_iter()
        .zip(metadata.into_iter())
        .for_each(|i| {
            match i.1.instruction_execution {
                Some(userdata) => {
                    qemu_plugin_register_vcpu_mem_cb(
                        i.0 .0,
                        Some(vcpu_mem_access),
                        qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
                        userdata,
                    );
                }
                None => {}
            };
            match i.1.memory_access {
                Some(userdata) => {
                    qemu_plugin_register_vcpu_insn_exec_cb(
                        i.0 .0,
                        Some(vcpu_insn_exec),
                        qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        userdata,
                    );
                }
                None => {}
            }
        });
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

    std::thread::spawn(|| {
        let mut f = std::fs::File::create("statistics.log").unwrap();
        f.write(b"timestamp,instructions,l1i_miss,l1d_miss,l1d_wb\n")
            .unwrap();
        loop {
            f.write_fmt(format_args!(
                "{},{},{},{},{}\n",
                Local::now().format("%H:%M:%S"),
                PLUGIN.statistics().instructions,
                PLUGIN.statistics().l1i_miss,
                PLUGIN.statistics().l1d_miss,
                PLUGIN.statistics().l1d_wb
            ))
            .unwrap();
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });

    return 0;
}
