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

mod icount;
mod vtime;

use core::ffi;
use once_cell::sync::Lazy;
use rustc_hash::FxHashMap;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::parameter as param;
use crate::qemu_api;

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

pub struct VirtualTimePlugin {}

impl super::Plugin for VirtualTimePlugin {
    #[inline]
    fn init(_: u64, options: &FxHashMap<String, String>) {
        // check the following options:
        // - vtime=on|off
        // - mode=vtime forces vtime=on.
        // - vtime_mode=adaptive|fixed
        // - vtime_scaling_factor=1000

        let mut vtime_is_on = options.get("vtime").map(|x| x == "on").unwrap_or(false);
        let mode = String::new();
        let mode = options.get("mode").unwrap_or(&mode);
        if *mode == "vtime" {
            vtime_is_on = true;
            println!("Mode is set to vtime.");
        } else {
            // then, we need to record the instruction count.
            unsafe {
                ICOUNT_PLUGIN = Box::into_raw(Box::new(icount::ICountPlugin::new()));
            }
        }

        if vtime_is_on && !unsafe { qemu_api::qemu_plugin_is_icount_mode() } {
            assert!(unsafe {
                qemu_api::qemu_plugin_register_cpu_clock_cb(Some(calculate_cpu_clock))
            });

            assert!(unsafe {
                qemu_api::qemu_plugin_register_snapshot_cpu_clock_update_cb(Some(
                    on_snapshot_cpu_clock_update,
                ))
            });

            // if the vtime_mode is fixed, we just set the scaling factor. Otherwise, we need to profile it.
            let vtime_mode = String::from("adaptive");
            let vtime_mode = options.get("vtime_mode").unwrap_or(&vtime_mode);

            if vtime_mode == "fixed" {
                let scaling_factor = options
                    .get("vtime_scaling_factor")
                    .unwrap_or(&String::from("1000"))
                    .parse::<f64>()
                    .unwrap();

                TIME_PLUGIN
                    .lock()
                    .unwrap()
                    .update_scaling_factor(scaling_factor);
            } else {
                // register threads to profile the icount and calculate the host time scaling factor.
                thread::spawn(|| {
                    // let mut history_icount = [(0, 0); param::CORE_COUNT];
                    let mut historical_local_time = [0; param::CORE_COUNT];
                    let mut last_scaling_factor = param::INIT_HOST_TIME_SCALE as f64;

                    let mut loop_count = 0;
                    let mut full_system_continuous_idle_turn = 0;
                    loop {
                        // read the current icount.
                        let local_vtime: [u64; param::CORE_COUNT] =
                            std::array::from_fn(|idx| unsafe {
                                qemu_api::qemu_plugin_get_vcpu_vtime(idx as u32)
                            });

                        let mut accumulated_local_vtime_diff = 0;
                        let mut active_core_count = 0;
                        // check the icount difference for cores that are not sleeping.
                        for i in 0..param::CORE_COUNT {
                            let diff = local_vtime[i] - historical_local_time[i];

                            if diff != 0 {
                                accumulated_local_vtime_diff += diff;
                                active_core_count += 1;
                            }

                            historical_local_time[i] = local_vtime[i];
                        }

                        if active_core_count != 0 {
                            // set the time scaling factor.
                            let average_centi_nanosecond =
                                accumulated_local_vtime_diff as f64 / active_core_count as f64;
                            
                            let scaling_factor = (param::HOST_TIME_SCALING_PROFILING_PERIOD as f64
                                * 1e8) 
                                / average_centi_nanosecond; // (delta host time in nano) / (delta vtime in centi-nano)

                            assert!(scaling_factor.is_finite());

                            TIME_PLUGIN
                                .lock()
                                .unwrap()
                                .update_scaling_factor(scaling_factor);

                            last_scaling_factor = scaling_factor;
                        } else {
                            full_system_continuous_idle_turn += 1;

                            if full_system_continuous_idle_turn == 2 {
                                // get all core's deadlines.
                                let mut next_deadline = std::u64::MAX;
                                for i in 0..param::CORE_COUNT {
                                    let deadline = unsafe {
                                        qemu_api::qemu_plugin_cpu_get_next_deadline(i as u32)
                                    };

                                    if deadline < next_deadline {
                                        next_deadline = deadline;
                                    }
                                }

                                if next_deadline < std::i64::MAX as u64 {
                                    // shift the time to the next deadline.
                                    TIME_PLUGIN.lock().unwrap().shift_time(next_deadline);
                                }

                                full_system_continuous_idle_turn = 0;
                            }
                        }

                        loop_count += 1;

                        if loop_count % 100 == 0 {
                            println!(
                                "Scaling factor: {}", last_scaling_factor
                            );
                        }

                        // wait for a period.
                        thread::sleep(Duration::from_millis(
                            param::HOST_TIME_SCALING_PROFILING_PERIOD as u64,
                        ));
                    }
                });
            }

            println!("Virtual time calculation is on. Mode: {}", vtime_mode);
            if vtime_mode == "fixed" {
                println!(
                    "Scaling factor: {}",
                    TIME_PLUGIN.lock().unwrap().get_scaling_factor()
                );
            }
        } else if unsafe { qemu_api::qemu_plugin_is_icount_mode() } {
            println!("Virtual time calculation is off because icount mode is on.");
        }
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

    fn serialize(folder_name: &str) {
        // for all cores, dump virtual times.
        let file = std::fs::File::create(format!("{}/vtime.json.zstd", folder_name)).unwrap();

        let mut file = zstd::Encoder::new(file, 0).unwrap();

        let vtime = (0..256)
            .map(|idx| unsafe { qemu_api::qemu_plugin_get_vcpu_vtime(idx) })
            .collect::<Vec<_>>();

        serde_json::to_writer(&mut file, &vtime).unwrap();

        file.finish().unwrap();
    }

    fn deserialize(folder_name: &str) {
        let file = std::fs::File::open(format!("{}/vtime.json.zstd", folder_name));

        if file.is_err() {
            println!("Cannot load the vtime state. Error: {:?}", file.err());
            return;
        }

        let file = file.unwrap();

        let mut file = zstd::Decoder::new(file).unwrap();

        let vtime: Vec<u64> = serde_json::from_reader(&mut file).unwrap();

        for (i, v) in vtime.into_iter().enumerate() {
            unsafe {
                qemu_api::qemu_plugin_set_vcpu_vtime(i as u32, v);
            }
        }
    }
}
