use crate::qemu_api;

pub trait Plugin: Send + Sync {
    fn init();

    unsafe fn on_translation(tb: *mut qemu_api::qemu_plugin_tb);
    
    fn dump_snapshot();
}


pub mod virtual_time;
pub mod memory;
pub mod trace;
pub mod marker;