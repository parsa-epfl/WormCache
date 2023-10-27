pub mod bp;
pub mod parameter;
pub use parameter::*;

pub mod components;
mod qemu_api;
mod util;

// Plugin
use components::memory::MemoryPlugin;
use components::trace::TracePlugin;
use components::virtual_time::VirtualTimePlugin;
use components::Plugin;

use std::ffi;

#[no_mangle]
pub static qemu_plugin_version: u32 = qemu_api::QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn plugin_exit(_: qemu_api::qemu_plugin_id_t, _: *mut ffi::c_void) {
    MemoryPlugin::dump_snapshot();
    VirtualTimePlugin::dump_snapshot();
    MarkerPlugin::dump_snapshot();
}

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    _: qemu_api::qemu_plugin_id_t,
    tb: *mut qemu_api::qemu_plugin_tb,
) {

    MemoryPlugin::on_translation(tb);
    VirtualTimePlugin::on_translation(tb);
    MarkerPlugin::on_translation(tb);
}

#[no_mangle]
unsafe extern "C" fn qemu_plugin_install(
    id: qemu_api::qemu_plugin_id_t,
    _: *const qemu_api::qemu_info_t,
    _: i32,
    _: *const *const u8,
) -> i32 {
    // make sure that the number of vCPUs is equal to the core count.
    assert_eq!(
        qemu_api::qemu_plugin_n_vcpus(),
        CORE_COUNT as i32,
        "Unmatched core count, thus exit."
    );

    qemu_api::qemu_plugin_register_vcpu_tb_trans_cb(id, Some(vcpu_tb_trans));

    MemoryPlugin::init();
    VirtualTimePlugin::init();
    MarkerPlugin::init();

    return 0;
}
