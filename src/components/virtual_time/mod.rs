mod icount;
mod vtime;

use once_cell::sync::Lazy;
use std::{io::Write, sync::Mutex};

use crate::qemu_api;

static TIME_PLUGIN: Lazy<Mutex<vtime::VirtualTimeContext>> =
    Lazy::new(|| Mutex::new(vtime::VirtualTimeContext::new()));

static ICOUNT_PLUGIN: Lazy<icount::ICountPlugin> = Lazy::new(|| icount::ICountPlugin::new());

#[no_mangle]
unsafe extern "C" fn calculate_virtual_time() -> i64 {
    return TIME_PLUGIN
        .lock()
        .unwrap()
        .calculate_virtual_time(&ICOUNT_PLUGIN);
}

// memory ts for print the log.
fn get_memory_ts() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

// For each component, you should provide the following functions.
// 1. init function.
#[inline]
pub unsafe fn init() {
    qemu_api::qemu_plugin_register_virtual_time_cb(Some(calculate_virtual_time));
    std::thread::spawn(|| {
        // open a csv file to store the icounts.
        let mut file = std::fs::File::create("cache-icount.csv").unwrap();
        // write the header.
        // file.write_fmt(format_args!("ts")).unwrap();
        // for i in 0..CORE_COUNT {
        //     file.write_fmt(format_args!(",core{}", i)).unwrap();
        // }
        // file.write_fmt(format_args!("\n")).unwrap();
        let mut head = vec!["ts".to_string()];
        for i in 0..crate::CORE_COUNT {
            head.push(format!("core{}", i));
        }

        file.write_fmt(format_args!("{}\n", head.join(",")))
            .unwrap();

        loop {
            let icounts = ICOUNT_PLUGIN.get_icounts();
            let mut lines = vec![];
            lines.push(format!("{}", get_memory_ts()));
            for i in 0..crate::CORE_COUNT {
                lines.push(format!("{}", icounts[i]));
            }
            file.write_fmt(format_args!("{}\n", lines.join(",")))
                .unwrap();
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });
}

// 2. translation function.
#[inline]
pub fn on_instruction_cacheline_touched(vcpu_idx: u32, context: &crate::PluginFetchBlockContext) {
    ICOUNT_PLUGIN.increase_icount(vcpu_idx as u8, context.size);
}

// 3. on memory access
#[inline]
pub fn on_data_cacheline_touched(_: u32, _: usize, _: usize, _: bool) {}

// 3. the function to export checkpoint. This function is guaranteed to be serial.
#[inline]
pub fn dump_snapshot() {
    // clean the icount, because the baseline is changed.
    TIME_PLUGIN.lock().unwrap().reset();
    ICOUNT_PLUGIN.reset();
}
