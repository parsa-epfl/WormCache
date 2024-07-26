use crate::qemu_api;
use rustc_hash::FxHashMap; // this is exposed to the crate so that executable binary can use it.

pub trait Plugin: Send + Sync {
    fn init(plugin_id: u64, options: &FxHashMap<String, String>);

    unsafe fn on_translation(tb: *mut qemu_api::qemu_plugin_tb);

    fn dump_snapshot(name: &str);

    fn serialize(name: &str);
    fn deserialize(name: &str);
}

pub trait FlexusCompatibleSerializer {
    type HelperType: Serialize; // the helper type must be directly serializable.

    fn get_serialize_helper(&self) -> Self::HelperType;
}

mod mmu; // this is only used by other components, not exposed to the crate.
pub use mmu::NoMMU;
use serde::Serialize;
pub mod bp;
pub mod cache_hierarchy;
pub mod debug;
pub mod marker;
pub mod pw_log;
pub mod touch_once;
pub mod trace;
pub mod virtual_time;
