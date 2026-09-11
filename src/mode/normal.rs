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
            crate::plugin_on_exit();
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
