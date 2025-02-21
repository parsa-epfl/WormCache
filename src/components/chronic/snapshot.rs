// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this
//    list of conditions and the following disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.
//
// 3. Neither the name of the PARSA, EPFL
//    nor the names of its contributors may be used to endorse or promote
//    products derived from this software without specific prior written
//    permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{fs::File, io::Write};

use crate::{
    components::debug::statistics::{EventType, Statistics},
    parameter::{self, PluginList},
    qemu_api,
    util::get_monotonic_ts,
};
use spin::Mutex as SpinMutex;

use std::sync::OnceLock;

static SNAPSHOT_INFO: SpinMutex<Option<(String, u64)>> = SpinMutex::new(None);

use std::sync::atomic::{AtomicU64, Ordering};

static mut PERIODIC_SNAPSHOT_INIT_INDEX: u64 = 0;
static PERIODIC_SNAPSHOT_COUNT: AtomicU64 = AtomicU64::new(0);
static mut PERIODIC_SNAPSHOT_REQUIRED_COUNT: u64 = 0xffff_ffff_ffff_ffff;

static mut PERIODIC_SNAPSHOT_THRESHOLD: u64 = 0xffff_ffff_ffff_ffff;
static mut PERIODIC_SNAPSHOT_INTERVAL: u64 = 0xffff_ffff_ffff_ffff;
static mut PERIODIC_SNAPSHOT_CURRENT_CYCLES: u64 = 0;
static mut PERIODIC_SNAPSHOT_NO_QEMU_SNAPSHOT: bool = false;

static SNAPSHOT_PREFIX: OnceLock<String> = OnceLock::new();

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
        "Snapshot request: {}, At Cycle: {}",
        &snapshot_info.0, snapshot_info.1
    );

    let c_snapshot_name = std::ffi::CString::new(snapshot_info.0.clone()).unwrap();

    qemu_api::qemu_plugin_savevm(c_snapshot_name.as_ptr(), false);

    let snapshot_count = PERIODIC_SNAPSHOT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;

    if snapshot_count >= PERIODIC_SNAPSHOT_REQUIRED_COUNT {
        println!("Generate {} snapshots. Quit.", snapshot_count);
        let mut miss_file = std::fs::File::create("statistics.final.csv").unwrap();
        miss_file
            .write_fmt(format_args!("{}\n", Statistics::get_header()))
            .unwrap();

        // update the local target time before writing the statistics
        for core_id in 0..parameter::CORE_COUNT {
            Statistics::global_set(core_id as u32, EventType::TargetLocalCycle, false, unsafe {
                qemu_api::qemu_plugin_get_vcpu_vtime(core_id as u32)
            });
        }

        for stat in Statistics::global_get_line_for_all_cores(get_monotonic_ts()) {
            miss_file.write_all(stat.as_bytes()).unwrap();
            miss_file.write_all(b"\n").unwrap();
        }
        std::process::exit(0);
    }
}

// add a global file to record the statistics for each quantum.
static STATISTICS_QUANTUM_FILE: OnceLock<SpinMutex<File>> = OnceLock::new();

// Remember, this function will be used as a quantum callback.
unsafe extern "C" fn quantum_checking_callback(diff: u64) -> bool {
    PERIODIC_SNAPSHOT_CURRENT_CYCLES += diff;

    if PERIODIC_SNAPSHOT_CURRENT_CYCLES >= PERIODIC_SNAPSHOT_THRESHOLD {
        let mut miss_file = STATISTICS_QUANTUM_FILE.get().unwrap().lock();
        // dump the statistics.
        for core_id in 0..parameter::CORE_COUNT {
            Statistics::global_set(core_id as u32, EventType::TargetLocalCycle, false, unsafe {
                qemu_api::qemu_plugin_get_vcpu_vtime(core_id as u32)
            });
        }

        for stat in Statistics::global_get_line_for_all_cores(get_monotonic_ts()) {
            miss_file.write_all(stat.as_bytes()).unwrap();
            miss_file.write_all(b"\n").unwrap();
        }

        drop(miss_file);

        let snapshot_name = format!(
            "{}_{}",
            SNAPSHOT_PREFIX.get().unwrap(),
            PERIODIC_SNAPSHOT_COUNT.load(Ordering::Relaxed) + PERIODIC_SNAPSHOT_INIT_INDEX
        );
        let snapshot_info = (snapshot_name, PERIODIC_SNAPSHOT_CURRENT_CYCLES);

        if !PERIODIC_SNAPSHOT_NO_QEMU_SNAPSHOT {
            let snapshot_info_guard = SNAPSHOT_INFO.try_lock();
            if snapshot_info_guard.is_none() {
                return false;
            }

            let mut snapshot_info_guard = snapshot_info_guard.unwrap();

            if snapshot_info_guard.is_none() {
                *snapshot_info_guard = Some(snapshot_info);
            }
        } else {
            println!(
                "WormCache-only snapshot request: {}, At Cycle: {}",
                &snapshot_info.0, snapshot_info.1
            );

            // manually call serialize function of all plugins.
            std::fs::create_dir_all(&snapshot_info.0).unwrap();
            PluginList::serialize(&snapshot_info.0);
            let snapshot_count = PERIODIC_SNAPSHOT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            1;

            if snapshot_count >= PERIODIC_SNAPSHOT_REQUIRED_COUNT {
                println!("Generate {} snapshots. Quit.", snapshot_count);
                let mut miss_file = std::fs::File::create("statistics.final.csv").unwrap();
                miss_file
                    .write_fmt(format_args!("{}\n", Statistics::get_header()))
                    .unwrap();

                // update the local target time before writing the statistics
                for core_id in 0..parameter::CORE_COUNT {
                    Statistics::global_set(
                        core_id as u32,
                        EventType::TargetLocalCycle,
                        false,
                        unsafe { qemu_api::qemu_plugin_get_vcpu_vtime(core_id as u32) },
                    );
                }

                for stat in Statistics::global_get_line_for_all_cores(get_monotonic_ts()) {
                    miss_file.write_all(stat.as_bytes()).unwrap();
                    miss_file.write_all(b"\n").unwrap();
                }
                std::process::exit(0);
            }
        }

        PERIODIC_SNAPSHOT_THRESHOLD += PERIODIC_SNAPSHOT_INTERVAL;

        return !PERIODIC_SNAPSHOT_NO_QEMU_SNAPSHOT;
    }

    false
}

pub unsafe fn init(
    init_threshold: u64,
    interval: u64,
    required_count: u64,
    prefix: String,
    init_index: u64,
    no_qemu_snapshot: bool,
) {
    PERIODIC_SNAPSHOT_THRESHOLD = init_threshold;
    PERIODIC_SNAPSHOT_REQUIRED_COUNT = required_count;
    PERIODIC_SNAPSHOT_INTERVAL = interval;
    SNAPSHOT_PREFIX.set(prefix).unwrap();
    PERIODIC_SNAPSHOT_INIT_INDEX = init_index;
    PERIODIC_SNAPSHOT_NO_QEMU_SNAPSHOT = no_qemu_snapshot;

    unsafe {
        assert!(qemu_api::qemu_plugin_register_periodic_check_cb(Some(
            quantum_checking_callback
        )));

        if !PERIODIC_SNAPSHOT_NO_QEMU_SNAPSHOT {
            assert!(qemu_api::qemu_plugin_register_event_loop_poll_cb(Some(
                event_loop_callback
            )));
        }
    }

    STATISTICS_QUANTUM_FILE
        .set(SpinMutex::new(
            File::create("statistics.quantum.csv").expect("Failed to create statistics file."),
        ))
        .expect("Failed to set the statistics file.");

    STATISTICS_QUANTUM_FILE
        .get()
        .unwrap()
        .lock()
        .write_fmt(format_args!("{}\n", Statistics::get_header()))
        .unwrap();
}
