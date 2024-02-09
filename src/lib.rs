pub mod parameter;
pub mod arch;

pub mod components;
mod qemu_api;
mod util;

// Plugin
use components::memory_ts::TimeStampedMemoryPlugin;
use components::trace::TracePlugin;
use components::virtual_time::VirtualTimePlugin;
use components::marker::MarkerPlugin;
use components::touch_once::TouchOnePlugin;
use components::pw_log::PageWalkLoggerPlugin;
use components::bp::BranchPredictorPlugin;
use components::memory_locker::LockedMemoryPlugin;
use components::memory_mtr_recording::MTRMemoryPlugin;
use components::memory_delayed::DelayedMemoryPlugin;

use parameter::PluginList;

use std::ffi;

#[no_mangle]
pub static qemu_plugin_version: u32 = qemu_api::QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn plugin_exit(_: qemu_api::qemu_plugin_id_t, _: *mut ffi::c_void) {
    PluginList::dump_snapshot();
}

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    _: qemu_api::qemu_plugin_id_t,
    tb: *mut qemu_api::qemu_plugin_tb,
) {
    PluginList::on_translation(tb);
}

#[no_mangle]
unsafe extern "C" fn qemu_plugin_install(
    id: qemu_api::qemu_plugin_id_t,
    qemu_info: *const qemu_api::qemu_info_t,
    _: i32,
    _: *const *const u8,
) -> i32 {
    // make sure that the number of vCPUs is equal to the core count.
    assert_eq!(
        qemu_api::qemu_plugin_n_vcpus(),
        parameter::CORE_COUNT as i32,
        "Unmatched core count, thus exit."
    );

    // check system emulation cost.
    assert_eq!(
        qemu_info.as_ref().unwrap().system_emulation,
        true,
        "Only support system emulation mode, thus exit."
    );

    // check the architectural name
    assert_eq!(
        ffi::CStr::from_ptr(qemu_info.as_ref().unwrap().target_name)
            .to_str()
            .unwrap(),
        "aarch64",
        "Only support aarch64 architecture, thus exit."
    );

    qemu_api::qemu_plugin_register_vcpu_tb_trans_cb(id, Some(vcpu_tb_trans));
    qemu_api::qemu_plugin_register_atexit_cb(id, Some(plugin_exit), std::ptr::null_mut());

    PluginList::init();

    return 0;
}
