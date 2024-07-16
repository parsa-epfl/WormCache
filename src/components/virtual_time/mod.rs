mod icount;
mod vtime;

use core::ffi;
use once_cell::sync::Lazy;
use spin::mutex::SpinMutex;
use std::{io::Write, sync::Mutex};

use crate::parameter as param;
use crate::qemu_api;

use crate::util::get_monotonic_ts;

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
}

unsafe extern "C" fn kernel_vcpu_insn_exec(
    vcpu_idx: u32,
    size: *mut ffi::c_void, // the size of the basic block
) {
    (*ICOUNT_PLUGIN).increase_kernel_icount(vcpu_idx as u8, size as u64);
}

static SNAPSHOT_INFO: SpinMutex<Option<(String, u64)>> = SpinMutex::new(None);
static mut PERIODIC_SNAPSHOT_COUNT: u64 = 0;

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

    if param::PERIODICAL_SNAPSHOT_QUIT_THRESHOLD.is_some()
        && PERIODIC_SNAPSHOT_COUNT >= param::PERIODICAL_SNAPSHOT_QUIT_THRESHOLD.unwrap()
    {
        println!("Generate {} snapshots. Quit.", PERIODIC_SNAPSHOT_COUNT);
        std::process::exit(0);
    }
}

pub struct VirtualTimePlugin {}

impl super::Plugin for VirtualTimePlugin {
    #[inline]
    fn init() {
        unsafe {
            ICOUNT_PLUGIN = Box::into_raw(Box::new(icount::ICountPlugin::new()));
        }

        if !param::USE_ICOUNT_MODE {
            assert!(unsafe {
                qemu_api::qemu_plugin_register_cpu_clock_cb(Some(calculate_cpu_clock))
            });

            assert!(unsafe {
                qemu_api::qemu_plugin_register_snapshot_cpu_clock_update_cb(Some(
                    on_snapshot_cpu_clock_update,
                ))
            });
        }

        if param::PERIODICAL_SNAPSHOT_ENABLED {
            println!("Periodical snapshot is enabled.");
            println!(
                "Interval: {}, Initial threshold: {}",
                param::PERIODICAL_SNAPSHOT_INTERVAL,
                param::PERIODICAL_SNAPSHOT_INITIAL_THRESHOLD
            );
            assert!(unsafe {
                qemu_api::qemu_plugin_register_event_loop_poll_cb(Some(event_loop_callback))
            });

            // This thread monitors the icounts and triggers the snapshot.
            std::thread::spawn(|| {
                let mut current_threshold = param::PERIODICAL_SNAPSHOT_INITIAL_THRESHOLD;
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

                        current_threshold += param::PERIODICAL_SNAPSHOT_INTERVAL;
                        snapshot_id += 1;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            });
        }

        std::thread::spawn(|| {
            // open a csv file to store the icounts.
            let mut file = std::fs::File::create("icount.csv").unwrap();
            // write the header.
            // file.write_fmt(format_args!("ts")).unwrap();
            // for i in 0..CORE_COUNT {
            //     file.write_fmt(format_args!(",core{}", i)).unwrap();
            // }
            // file.write_fmt(format_args!("\n")).unwrap();
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
        println!("Virtual time plugin initialized.");
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
