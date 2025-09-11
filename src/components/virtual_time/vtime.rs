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

use crate::parameter as param;
use crate::qemu_api::qemu_plugin_cpu_is_tick_enabled;
use crate::qemu_api::qemu_plugin_get_snapshot_cpu_clock;

pub struct VirtualTimeContext {
    last_real_time: u64,
    advanced_vclock: i64,
    time_scaling_factor: f64,
}

impl VirtualTimeContext {
    pub fn new() -> Self {
        Self {
            last_real_time: 0,
            advanced_vclock: 0,
            time_scaling_factor: param::INIT_HOST_TIME_SCALE as f64,
        }
    }

    pub fn update_scaling_factor(&mut self, scaling_factor: f64) {
        if scaling_factor == 0.0 {
            panic!("The scaling factor should not be zero.");
        }
        self.time_scaling_factor = scaling_factor;
    }

    pub fn shift_time(&mut self, shift: u64) {
        /*
        if unsafe { qemu_plugin_cpu_is_tick_enabled() } {
            self.advanced_vclock += shift as i64;
        }
        */
    }

    pub fn get_scaling_factor(&self) -> f64 {
        self.time_scaling_factor
    }

    pub fn calculate_cpu_clock(&mut self) -> i64 {
        // 1. get real timestamp in nanosecond
        // let real_time = SystemTime::now()
        //     .duration_since(SystemTime::UNIX_EPOCH)
        //     .unwrap()
        //     .as_nanos() as i128;

        let real_time = crate::util::get_monotonic_ts();

        // 2. calculate the potential update
        /*
        unsafe {
            if qemu_plugin_cpu_is_tick_enabled() && self.last_real_time != 0 {
                // 3.2 if the maximum is zero, we use the difference of the real time.
                let advanced_vtime =
                    ((real_time - self.last_real_time) as f64 / self.time_scaling_factor) as i64;

                // 3.3 update the advanced vclock
                self.advanced_vclock += advanced_vtime;
            }
        }
        */

        // 4. update the context with the new icounts and the real time.
        self.last_real_time = real_time;

        // 5. return the calculated virtual time
        // TODO: this way to calculate the time has bug when exporting multiple checkpoints
        // Because qb.qemu_plugin_get_snapshoted_vm_clock() is updated a checkpoint is exported.
        // I didn't see a better solution. Maybe storing this value inside this plugin?

        unsafe { self.advanced_vclock + qemu_plugin_get_snapshot_cpu_clock() }
    }

    #[allow(dead_code)]
    pub fn calculate_cpu_clock_with_10x_slowdown_from_realtime(&mut self) -> i64 {
        let real_time = crate::util::get_monotonic_ts();

        /*
        unsafe {
            if qemu_plugin_cpu_is_tick_enabled() && self.last_real_time != 0 {
                let advanced_vtime = (real_time - self.last_real_time) as i64;
                self.advanced_vclock += advanced_vtime / 10;
            }
        }
        */

        self.last_real_time = real_time;

        // 5. return the calculated virtual time
        unsafe { self.advanced_vclock + qemu_plugin_get_snapshot_cpu_clock() }
    }

    pub fn reset(&mut self) {
        self.last_real_time = 0;
        self.advanced_vclock = 0;
    }
}
