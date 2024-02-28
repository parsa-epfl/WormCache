use crate::qemu_api;

pub trait Plugin: Send + Sync {
    fn init();

    unsafe fn on_translation(tb: *mut qemu_api::qemu_plugin_tb);

    fn dump_snapshot();
}

mod mmu; // this is only used by other components, not exposed to the crate.
pub use mmu::NoMMU; // this is exposed to the crate so that executable binary can use it.
pub mod bp;
pub mod marker;
pub mod memory_delayed;
pub mod memory_locker;
pub mod memory_mtr_recording;
pub mod memory_ts;
pub mod pw_log;
pub mod touch_once;
pub mod trace;
pub mod virtual_time;
