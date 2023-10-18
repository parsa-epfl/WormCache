mod icount;
mod vtime;

use once_cell::sync::Lazy;
use std::sync::Mutex;

use crate::qemu_api;

static TIME_PLUGIN: Lazy<Mutex<vtime::VirtualTimeContext>> =
    Lazy::new(|| Mutex::new(vtime::VirtualTimeContext::new()));

static ICOUNT_PLUGIN: Lazy<icount::ICountPlugin> = Lazy::new(|| icount::ICountPlugin::new());


#[no_mangle]
unsafe extern "C" fn calculate_virtual_time() -> i64 {
    return TIME_PLUGIN.lock().unwrap().calculate_virtual_time(&ICOUNT_PLUGIN);
}

#[no_mangle]
unsafe extern "C" fn icount_incrementing(
    vcpu_idx: u32,
    user_data: *mut std::ffi::c_void,
) {
    let core_id = vcpu_idx as u8;
    let count = user_data as usize;
    ICOUNT_PLUGIN.increase_icount(core_id, count);
}

// For each component, you should provide the following functions.
// 1. init function. 
#[inline]
pub unsafe fn init() {
    qemu_api::qemu_plugin_register_virtual_time_cb(Some(calculate_virtual_time));
}


// 2. translation function. 
#[inline]
pub fn on_instruction_cacheline_touched(
    vcpu_idx: u32,
    context: &crate::PluginFetchBlockContext
) {
    ICOUNT_PLUGIN.increase_icount(vcpu_idx as u8, context.size);
}

// 3. on memory access
#[inline]
pub fn on_data_cacheline_touched(
    _: u32,
    _: usize,
    _: usize,
    _: bool
) {
    
}

// 3. the function to export checkpoint. This function is guaranteed to be serial.
#[inline]
pub fn dump_snapshot() {
    // clean the icount, because the baseline is changed.
    TIME_PLUGIN.lock().unwrap().reset();
    ICOUNT_PLUGIN.reset();
}
