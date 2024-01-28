pub mod parameter;
pub mod arch;

pub mod components;
mod qemu_api;
mod util;

// Plugin
use components::memory_mtr::TimeStampedMemoryPlugin;
use components::trace::TracePlugin;
use components::virtual_time::VirtualTimePlugin;
use components::marker::MarkerPlugin;
use components::touch_once::TouchOnePlugin;
use components::pw_log::PageWalkLoggerPlugin;
use components::bp::BranchPredictorPlugin;
use components::Plugin;

use std::ffi;

#[no_mangle]
pub static qemu_plugin_version: u32 = qemu_api::QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn plugin_exit(_: qemu_api::qemu_plugin_id_t, _: *mut ffi::c_void) {
    TimeStampedMemoryPlugin::dump_snapshot();
    VirtualTimePlugin::dump_snapshot();
    // TracePlugin::dump_snapshot();
    MarkerPlugin::dump_snapshot();
    // TouchOnePlugin::dump_snapshot();
    // PageWalkLoggerPlugin::dump_snapshot();
    BranchPredictorPlugin::dump_snapshot();
}

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    _: qemu_api::qemu_plugin_id_t,
    tb: *mut qemu_api::qemu_plugin_tb,
) {

    TimeStampedMemoryPlugin::on_translation(tb);
    VirtualTimePlugin::on_translation(tb);
    // TracePlugin::on_translation(tb);
    MarkerPlugin::on_translation(tb);
    //TouchOnePlugin::on_translation(tb);
    // PageWalkLoggerPlugin::on_translation(tb);
    BranchPredictorPlugin::on_translation(tb);
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

    TimeStampedMemoryPlugin::init();
    VirtualTimePlugin::init();
    // TracePlugin::init();
    MarkerPlugin::init();
    // TouchOnePlugin::init();
    // PageWalkLoggerPlugin::init();
    BranchPredictorPlugin::init();

    return 0;
}
