mod icount;
mod vtime;

use core::ffi;
use once_cell::sync::Lazy;
use rustc_hash::FxHashMap;
use serde_json::json;
use spin::mutex::SpinMutex;
use std::{io::Write, sync::Mutex};

use crate::parameter as param;
use crate::qemu_api;

use crate::util::get_monotonic_ts;

use super::debug::statistics::EventType;
use super::debug::statistics::Statistics;

static TIME_PLUGIN: Lazy<Mutex<vtime::VirtualTimeContext>> =
    Lazy::new(|| Mutex::new(vtime::VirtualTimeContext::new()));

static mut ICOUNT_PLUGIN: *mut icount::ICountPlugin = std::ptr::null_mut();

unsafe extern "C" fn calculate_cpu_clock() -> i64 {
    return TIME_PLUGIN.lock().unwrap().calculate_cpu_clock();
}

unsafe extern "C" fn on_snapshot_cpu_clock_update() {
    TIME_PLUGIN.lock().unwrap().reset();
}

unsafe extern "C" fn user_vcpu_insn_exec(
    vcpu_idx: u32,
    size: *mut ffi::c_void, // the size of the basic block
) {
    (*ICOUNT_PLUGIN).increase_user_icount(vcpu_idx as u8, size as u64);
    Statistics::global_record_by(vcpu_idx, EventType::Instruction, false, size as u64);
}

unsafe extern "C" fn kernel_vcpu_insn_exec(
    vcpu_idx: u32,
    size: *mut ffi::c_void, // the size of the basic block
) {
    (*ICOUNT_PLUGIN).increase_kernel_icount(vcpu_idx as u8, size as u64);
    Statistics::global_record_by(vcpu_idx, EventType::Instruction, true, size as u64);
}

static SNAPSHOT_INFO: SpinMutex<Option<(String, u64)>> = SpinMutex::new(None);
static mut PERIODIC_SNAPSHOT_COUNT: u64 = 0;
static mut PERIODIC_SNAPSHOT_REQUIRED_COUNT: u64 = 0xffff_ffff_ffff_ffff;

unsafe extern "C" fn event_loop_callback() {
    let snapshot_info_guard = SNAPSHOT_INFO.try_lock();
    if snapshot_info_guard.is_none() {
        return;
    }

    let mut snapshot_info_guard = snapshot_info_guard.unwrap();

    if snapshot_info_guard.is_none() {
        return;
    }

    let snapshot_info = snapshot_info_guard.take().unwrap();

    println!(
        "Snapshot request: {}, User ICount: {}",
        &snapshot_info.0, snapshot_info.1
    );

    qemu_api::qemu_plugin_savevm(snapshot_info.0.as_ptr() as *const i8);

    PERIODIC_SNAPSHOT_COUNT += 1;

    if PERIODIC_SNAPSHOT_COUNT >= PERIODIC_SNAPSHOT_REQUIRED_COUNT {
        println!("Generate {} snapshots. Quit.", PERIODIC_SNAPSHOT_COUNT);
        std::process::exit(0);
    }
}

static mut MEASURE_MAX_TURN: u64 = 0xffff_ffff_ffff_ffff;
static mut MEASURE_INTERVAL: u64 = 0;

static mut MEASURE_TURN: u64 = 0;
static mut MEASURE_NEXT_THRESHOLD: u64 = 0;

unsafe extern "C" fn on_icount_periodic_checking() {
    // read user icount.
    let u_total_icount = (*ICOUNT_PLUGIN).total_user_icount();
    if u_total_icount > MEASURE_NEXT_THRESHOLD {
        let (u_icount, k_icount) = (*ICOUNT_PLUGIN).total_icount();

        let mut statistics_u = 0;
        let mut statistics_k = 0;
        let mut l2_miss_u = 0;
        let mut l2_miss_k = 0;
        let mut llc_miss_u = 0;
        let mut llc_miss_k = 0;
        let mut bp_miss_u = 0;
        let mut bp_miss_k = 0;

        const MEASURED_CORE_COUNT: usize = if param::MEASURE_HALF_OF_CORES {
            param::CORE_COUNT / 2
        } else {
            param::CORE_COUNT
        };

        for core_id in 0..MEASURED_CORE_COUNT {
            // Query the instruction.
            let (_, i_u, i_k) =
                Statistics::global_query_record(core_id as u32, EventType::Instruction);
            statistics_u += i_u;
            statistics_k += i_k;

            // Query the L2 miss.
            let (_, l2_u, l2_k) =
                Statistics::global_query_record(core_id as u32, EventType::PrivateCacheMiss);

            l2_miss_u += l2_u;
            l2_miss_k += l2_k;

            // Query the LLC miss.
            let (_, llc_u, llc_k) =
                Statistics::global_query_record(core_id as u32, EventType::SharedCacheMiss);

            llc_miss_u += llc_u;
            llc_miss_k += llc_k;

            // Query the branch predictor miss.
            let (_, bp_u, bp_k) =
                Statistics::global_query_record(core_id as u32, EventType::BPMiss);

            bp_miss_u += bp_u;
            bp_miss_k += bp_k;
        }

        assert!(statistics_u == u_icount);
        assert!(statistics_k == k_icount);

        let i = u_icount + k_icount;
        let l2_miss = l2_miss_u + l2_miss_k;
        let llc_miss = llc_miss_u + llc_miss_k;
        let bp_miss = bp_miss_u + bp_miss_k;

        // You should stop the simulation.
        println!("Total userspace instruction: {}", u_total_icount);
        // report statistics.
        let result_json = json!({
            "icount": i,
            "icount:u": u_icount,
            "icount:k": k_icount,

            "l2_miss": l2_miss,
            "l2_miss:u": l2_miss_u,
            "l2_miss:k": l2_miss_k,

            "llc_miss": llc_miss,
            "llc_miss:u": llc_miss_u,
            "llc_miss:k": llc_miss_k,

            "bp": bp_miss,
            "bp:u": bp_miss_u,
            "bp:k": bp_miss_k,

            "agg": {
                "l2_mpki": l2_miss as f64 / i as f64 * 1000.0,
                "llc_mpki": llc_miss as f64 / i as f64 * 1000.0,
                "bp_mpki": bp_miss as f64 / i as f64 * 1000.0,

                "l2_mpki:u": l2_miss_u as f64 / u_icount as f64 * 1000.0,
                "llc_mpki:u": llc_miss_u as f64 / u_icount as f64 * 1000.0,
                "bp_mpki:u": bp_miss_u as f64 / u_icount as f64 * 1000.0,
            }
        });

        // write the result_json to a file.
        let file =
            std::fs::File::create(format!("icount_statistics_{}.json", MEASURE_TURN)).unwrap();
        serde_json::to_writer_pretty(&file, &result_json).unwrap();

        MEASURE_TURN += 1;
        if MEASURE_TURN >= MEASURE_MAX_TURN {
            println!("The maximum statistics turn is reached. Quit.");
            std::process::exit(0);
        }

        MEASURE_NEXT_THRESHOLD += MEASURE_INTERVAL;
    }
}

pub struct VirtualTimePlugin {}

impl super::Plugin for VirtualTimePlugin {
    #[inline]
    fn init(options: &FxHashMap<String, String>) {
        unsafe {
            ICOUNT_PLUGIN = Box::into_raw(Box::new(icount::ICountPlugin::new()));
        }

        // check the following options:
        // - vtime=on|off
        // - mode=normal|warm|measure
        // - init_threshold=N
        // - interval=N
        // - count=N
        // - check_duration=N

        let vtime_is_on = options.get("vtime").map(|x| x == "on").unwrap_or(false);

        if vtime_is_on && !unsafe { qemu_api::qemu_plugin_is_icount_mode() } {
            assert!(unsafe {
                qemu_api::qemu_plugin_register_cpu_clock_cb(Some(calculate_cpu_clock))
            });

            assert!(unsafe {
                qemu_api::qemu_plugin_register_snapshot_cpu_clock_update_cb(Some(
                    on_snapshot_cpu_clock_update,
                ))
            });

            println!("Virtual time calculation is on.");
        } else if unsafe { qemu_api::qemu_plugin_is_icount_mode() } {
            println!("Virtual time calculation is off because icount mode is on.");
        }

        let normal = "normal".to_string();
        let mode = options.get("mode").unwrap_or(&normal);

        if mode == "warm" {
            println!("Periodical snapshot (warm) is enabled.");
            let init_threshold = options
                .get("init_threshold")
                .map(|x| x.parse::<u64>().unwrap())
                .unwrap();

            let interval = options
                .get("interval")
                .map(|x| x.parse::<u64>().unwrap())
                .unwrap();

            let count = options
                .get("count")
                .map(|x| x.parse::<u64>().unwrap())
                .unwrap();

            let check_duration = options
                .get("check_duration")
                .map(|x| x.parse::<u64>().unwrap())
                .unwrap();

            // update count
            unsafe { PERIODIC_SNAPSHOT_REQUIRED_COUNT = count };

            println!(
                "Interval: {}, Initial threshold: {}, Count: {}, Check duration: {} ms",
                interval, init_threshold, count, check_duration
            );
            assert!(unsafe {
                qemu_api::qemu_plugin_register_event_loop_poll_cb(Some(event_loop_callback))
            });

            // This thread monitors the icounts and triggers the snapshot.
            std::thread::spawn(move || {
                let mut current_threshold = init_threshold;
                let mut snapshot_id = 0;
                loop {
                    let current_user_icount = unsafe { (*ICOUNT_PLUGIN).total_user_icount() };
                    if current_user_icount > current_threshold {
                        let mut snapshot_info = SNAPSHOT_INFO.lock();
                        if snapshot_info.is_none() {
                            // write a snapshot request.
                            *snapshot_info =
                                Some((format!("snapshot_{}", snapshot_id), current_user_icount));
                        }

                        drop(snapshot_info);

                        current_threshold += interval;
                        snapshot_id += 1;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(check_duration));
                }
            });
        } else if mode == "measure" {
            let init_threshold = options
                .get("init_threshold")
                .map(|x| x.parse::<u64>().unwrap())
                .unwrap();

            let interval = options
                .get("interval")
                .map(|x| x.parse::<u64>().unwrap())
                .unwrap();

            let count = options
                .get("count")
                .map(|x| x.parse::<u64>().unwrap())
                .unwrap();

            println!("Measurement is ON.");
            println!(
                "Interval: {}, Initial threshold: {}, Count: {}",
                interval, init_threshold, count
            );

            unsafe {
                MEASURE_MAX_TURN = count;
                MEASURE_INTERVAL = interval;
                MEASURE_NEXT_THRESHOLD = init_threshold;
            }

            assert!(unsafe {
                qemu_api::qemu_plugin_register_icount_periodic_checking_cb(Some(
                    on_icount_periodic_checking,
                ))
            })
        }

        std::thread::spawn(|| {
            // open a csv file to store the icounts.
            let mut file = std::fs::File::create("icount.csv").unwrap(); // TODO: combine this log with another log.
            let mut head = vec!["ts".to_string()];
            for i in 0..param::CORE_COUNT {
                head.push(format!("core{}", i));
                head.push(format!("core{}:u", i));
                head.push(format!("core{}:k", i));
            }

            file.write_fmt(format_args!("{}\n", head.join(",")))
                .unwrap();

            const CORE_RANGE_FOR_TIME_CALCULATION: usize =
                // if param::CACHE_HIERARCHY_FOR_HALF_OF_CORES {
                if false {
                    param::CORE_COUNT / 2
                } else {
                    param::CORE_COUNT
                };

            let mut accumulated_host_time: u64 = 0;
            let mut history_icount = [(0, 0); CORE_RANGE_FOR_TIME_CALCULATION];

            loop {
                let icounts = unsafe { (*ICOUNT_PLUGIN).get_icounts() };
                let mut lines = vec![];
                lines.push(format!("{}", get_monotonic_ts()));
                for i in 0..param::CORE_COUNT {
                    let (u, k) = icounts[i];
                    let all = u + k;
                    lines.push(format!("{}", all));
                    lines.push(format!("{}", u));
                    lines.push(format!("{}", k));
                }
                file.write_fmt(format_args!("{}\n", lines.join(",")))
                    .unwrap();

                // do a copy
                let mut maximum_icount = 0;
                let mut progress = false;
                for i in 0..CORE_RANGE_FOR_TIME_CALCULATION {
                    let (u, k) = icounts[i];
                    let this_core_icount = u + k;
                    if this_core_icount > maximum_icount {
                        maximum_icount = this_core_icount;
                    }

                    if (u, k) != history_icount[i] {
                        progress = true;
                    }

                    history_icount[i] = (u, k);
                }

                // if the maximum icount is not zero, we update the time scaling factor.
                if maximum_icount > 0 && accumulated_host_time > 0 {
                    let mut time_plugin = TIME_PLUGIN.lock().unwrap();
                    let factor =
                        (accumulated_host_time * 1e9 as u64) as f64 / maximum_icount as f64;

                    time_plugin.update_scaling_factor(factor);
                }

                if progress {
                    accumulated_host_time += 10;
                }

                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        });
    }

    unsafe fn on_translation(tb: *mut qemu_api::qemu_plugin_tb) {
        let first_instruction = qemu_api::qemu_plugin_tb_get_insn(tb, 0);
        let size = qemu_api::qemu_plugin_tb_n_insns(tb);
        // I need to get the first instruction's PC to see if it is a user or kernel space.
        let pc = qemu_api::qemu_plugin_insn_vaddr(first_instruction);
        if pc & 0x8000_0000_0000_0000 == 0 {
            // user space
            qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                first_instruction,
                Some(user_vcpu_insn_exec),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                size as *mut ffi::c_void,
            );
        } else {
            qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                first_instruction,
                Some(kernel_vcpu_insn_exec),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                size as *mut ffi::c_void,
            );
        }
    }

    #[inline]
    fn dump_snapshot(_: &str) {}

    fn serialize(_: &str) {}

    fn deserialize(_: &str) {}
}
