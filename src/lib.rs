
pub mod cache;
pub mod checkpoint;
pub mod mh;
pub mod bp;
mod qemu_api;
use mh::ts_model::TimestampMemoryHierarchy;
use mh::ts_model::TimestampSingleCoreMemoryHierarchy;
use qemu_api::*;
mod plugin;
use plugin::{
    QEMUPlugin,
    QEMUPluginPerCoreActor
};
use std::{ffi, cell::RefCell};

type PluginType = TimestampMemoryHierarchy<8, 512, 16, 1024>;
static mut PLUGIN: Option<PluginType> = None;
type PerCorePlyginType = <PluginType as QEMUPlugin>::PerCorePlugin;

// There might be a centralized data structure and a thread local data structure.
thread_local! {
    pub static per_core_records: Option<*mut PerCorePlyginType> = None;
}

#[no_mangle]
pub static qemu_plugin_version: u32 = QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn vcpu_mem_access(
    vcpu_index: u32,
    info: qemu_plugin_meminfo_t,
    vaddr: u64,
    user_data: *mut ffi::c_void, // should be NULL.
) {
    per_core_records.with(|x|{
        match x {
            Some(x) => {
                let x = &mut *x.clone();
                x.on_memory_access(vcpu_index, &plugin::QEMUMemoryInfo(info), vaddr, user_data);
            },
            None => {},
        };
    })
}

#[no_mangle]
unsafe extern "C" fn vcpu_insn_exec(
    vcpu_index: u32,
    user_data: *mut ffi::c_void, // it is basically its physical address.
) {
    per_core_records.with(|x|{
        match x {
            Some(x) => {
                let x = &mut *x.clone();
                x.on_instruction_execution(vcpu_index, user_data);
            },
            None => {},
        }
    })
}

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    id: qemu_api::qemu_plugin_id_t,
    tb: *mut qemu_api::qemu_plugin_tb,
) {
    let wrapped_tb = plugin::QEMUPluginBasicBlock(tb);
    let metadata = PLUGIN.as_mut().unwrap().on_translation(&wrapped_tb);
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
    PLUGIN.as_mut().unwrap().on_qemu_exit();
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
    PLUGIN.replace(PluginType::new());
    return 0;
}

/*
 * Some other callbacks (e.g., qemu_plugin_register_vcpu_init_cb) can be utilized to create the 
 */
