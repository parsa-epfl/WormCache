pub mod arch;
pub mod parameter;

pub mod components;
mod qemu_api;
mod util;

// Plugin
#[allow(unused_imports)]
use components::bp::BranchPredictorPlugin;
#[allow(unused_imports)]
use components::cache_hierarchy::ParallelCacheHierarchyPlugin;
#[allow(unused_imports)]
use components::marker::MarkerPlugin;
#[allow(unused_imports)]
use components::pw_log::PageWalkLoggerPlugin;
#[allow(unused_imports)]
use components::touch_once::TouchOnePlugin;
#[allow(unused_imports)]
use components::trace::TracePlugin;
#[allow(unused_imports)]
use components::virtual_time::VirtualTimePlugin;

use parameter::PluginList;

use std::ffi;

#[no_mangle]
pub static qemu_plugin_version: u32 = qemu_api::QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    _: qemu_api::qemu_plugin_id_t,
    tb: *mut qemu_api::qemu_plugin_tb,
) {
    PluginList::on_translation(tb);
}

#[no_mangle]
unsafe extern "C" fn savevm_cb(name: *const ffi::c_char) {
    // let name = ffi::CStr::from_ptr(name).to_str().unwrap();
    // create a folder for the name.
    // std::fs::create_dir_all(name).unwrap();
    // PluginList::dump_snapshot(name);
}

#[no_mangle]
unsafe extern "C" fn qemu_plugin_exit(_: qemu_api::qemu_plugin_id_t, _: *mut ffi::c_void) {
    std::fs::create_dir_all("unsaved").unwrap();
    PluginList::dump_snapshot("unsaved");
}

unsafe extern "C" fn qemu_deplete_quantum_cb() {
    std::fs::create_dir_all("unsaved").unwrap();
    PluginList::dump_snapshot("unsaved");
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
    assert!(
        qemu_info.as_ref().unwrap().system_emulation,
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
    qemu_api::qemu_plugin_register_atexit_cb(id, Some(qemu_plugin_exit), std::ptr::null_mut());
    qemu_api::qemu_plugin_register_savevm_cb(Some(savevm_cb));
    qemu_api::qemu_plugin_register_quantum_deplete_cb(Some(qemu_deplete_quantum_cb));

    PluginList::init();

    0
}
