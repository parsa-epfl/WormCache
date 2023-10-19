use std::{
    fs::File,
    io::{BufWriter, Write},
    sync::Mutex,
};

use once_cell::sync::Lazy;

fn get_memory_ts() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

static TRACE_FILE: Lazy<Mutex<BufWriter<File>>> = Lazy::new(|| {
    // open a file to store the trace.
    let mut file = std::fs::File::create("trace.trace").unwrap();
    let mut b = BufWriter::with_capacity(64 * 1024 * 1024, file);
    return Mutex::new(b);
});

// This structure is just a wrapper of for the plugin system to register. Plugin is believed to be globally singleton.
pub struct TracePlugin {}

impl super::Plugin for TracePlugin {
    #[inline]
    fn instance() -> Self {
        return Self {};
    }

    #[inline]
    fn init() {
        // make sure the file is initialized.
        TRACE_FILE.lock().unwrap().flush().unwrap();
        println!("Trace plugin initialized.");
    }

    #[inline]
    fn on_instruction_cacheline_touched(
        vcpu_idx: u32,
        context: &crate::PluginFetchBlockContext,
    ) {
        let mut buffer = [0u8; 18];
        buffer[0..8].copy_from_slice(&context.pa.to_le_bytes());
        buffer[8..16].copy_from_slice(&(get_memory_ts() as u64).to_le_bytes());
        buffer[16] = 0;
        buffer[17] = vcpu_idx as u8;
        TRACE_FILE.lock().unwrap().write(&buffer).unwrap();
    }

    fn on_data_cacheline_touched(vcpu_idx: u32, va: usize, pa: usize, is_write: bool) {
        let mut buffer = [0u8; 18];
        buffer[0..8].copy_from_slice(&pa.to_le_bytes());
        buffer[8..16].copy_from_slice(&(get_memory_ts() as u64).to_le_bytes());
        buffer[16] = if is_write { 2 } else { 1 };
        buffer[17] = vcpu_idx as u8;
        TRACE_FILE.lock().unwrap().write(&buffer).unwrap();
    }

    fn dump_snapshot() {
        TRACE_FILE.lock().unwrap().flush().unwrap();
    }
}

