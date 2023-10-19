pub trait Plugin: Send + Sync {
    fn instance() -> Self;
    fn init();
    fn on_instruction_cacheline_touched(vcpu_idx: u32, context: &crate::PluginFetchBlockContext);
    fn on_data_cacheline_touched(vcpu_idx: u32, va: usize, pa: usize, is_write: bool);
    fn dump_snapshot();
}

pub mod virtual_time;
pub mod memory;
pub mod trace;