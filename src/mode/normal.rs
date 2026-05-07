use std::io::Write;

use crate::{
    debug::statistics::{EventType, Statistics},
    parameter,
    qemu_api,
    util::get_monotonic_ts,
};

static mut QUIT_THRESHOLD: u64 = u64::MAX;
static mut QUIT_ACCUMULATED_CYCLES: u64 = 0;

unsafe extern "C" fn quit_checking_callback(diff: u64) -> bool {
    unsafe {
        QUIT_ACCUMULATED_CYCLES += diff;

        if QUIT_ACCUMULATED_CYCLES >= QUIT_THRESHOLD {
            let threshold = QUIT_THRESHOLD;
            let accumulated = QUIT_ACCUMULATED_CYCLES;
            println!(
                "Quit mode: Reached quit threshold {} cycles at {} cycles. Dumping and quitting...",
                threshold, accumulated
            );

            let mut miss_file = std::fs::File::create("statistics.final.csv").unwrap();
            miss_file
                .write_fmt(format_args!("{}\n", Statistics::get_header()))
                .unwrap();

            for core_id in 0..parameter::CORE_COUNT {
                Statistics::global_set(
                    core_id as u32,
                    EventType::TargetLocalCycle,
                    false,
                    qemu_api::qemu_plugin_get_vcpu_vtime(core_id as u32),
                );
            }

            for stat in Statistics::global_get_line_for_all_cores(get_monotonic_ts()) {
                miss_file.write_all(stat.as_bytes()).unwrap();
                miss_file.write_all(b"\n").unwrap();
            }

            crate::debug::timing::print_time_breakdown("simulation_ckpt_time.json");
            crate::debug::noc_traffic::NocTraffic::save_to_csv("noc_traffic.final.csv");
            std::process::exit(0);
        }
    }
    false
}

pub unsafe fn init(threshold: u64) {
    unsafe {
        QUIT_THRESHOLD = threshold;
        assert!(qemu_api::qemu_plugin_register_periodic_check_cb(Some(
            quit_checking_callback
        )));
        println!(
            "Quit mode: Quit threshold set to {} cycles. Auto-quit enabled.",
            threshold
        );
    }
}
