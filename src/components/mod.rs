use crate::qemu_api;

pub trait Plugin: Send + Sync {
    fn init();

    unsafe fn on_translation(tb: *mut qemu_api::qemu_plugin_tb);

    fn dump_snapshot(name: &str);
}

mod mmu; // this is only used by other components, not exposed to the crate.
pub use mmu::NoMMU; // this is exposed to the crate so that executable binary can use it.
pub mod bp;
pub mod cache_hierarchy;
pub mod debug;
pub mod marker;
pub mod pw_log;
mod ref_cache_hierarchy;
pub mod touch_once;
pub mod trace;
pub mod virtual_time;
