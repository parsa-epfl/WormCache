use crate::parameter as param;
use crate::qemu_api::qemu_plugin_cpu_is_tick_enabled;
use crate::qemu_api::qemu_plugin_get_snapshot_cpu_clock;

use std::time::SystemTime;

use super::icount::ICountPlugin;

pub struct VirtualTimeContext {
    last_real_time: i128,
    last_icount: u64,
    advanced_vclock: i64,
}

impl VirtualTimeContext {
    pub fn new() -> Self {
        Self {
            last_real_time: 0,
            last_icount: 0,
            advanced_vclock: 0,
        }
    }

    pub fn calculate_cpu_clock(&mut self, icount: &ICountPlugin) -> i64 {
        // 1. get real timestamp in nanosecond
        let real_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i128;

        // 2. get the icounts from each core
        let icounts = icount.get_total_icounts_of_core(0);

        // 3. calculate the potential update
        unsafe {
            if qemu_plugin_cpu_is_tick_enabled() {
                // 3.1 calculate the difference icounts and find the maximum.
                let icount_diff = icounts - self.last_icount;

                // 3.2 if the maximum is zero, we use the difference of the real time.
                let advanced_vtime = if icount_diff == 0 {
                    if self.last_real_time == 0 {
                        // the first time this function is called. We should ignore it.
                        0
                    } else {
                        ((real_time - self.last_real_time) / param::HOST_TIME_SCALE as i128) as i64
                    }
                } else {
                    icount_diff as i64
                };

                // 3.3 update the advanced vclock
                self.advanced_vclock += advanced_vtime;
            }
        }

        // 4. update the context with the new icounts and the real time.
        self.last_icount = icounts;
        self.last_real_time = real_time;

        // 5. return the calculated virtual time
        // TODO: this way to calculate the time has bug when exporting multiple checkpoints
        // Because qb.qemu_plugin_get_snapshoted_vm_clock() is updated a checkpoint is exported.
        // I didn't see a better solution. Maybe storing this value inside this plugin?

        unsafe {
            self.advanced_vclock + qemu_plugin_get_snapshot_cpu_clock()
        }
    }

    pub fn calculate_cpu_clock_with_10x_slowdown_from_realtime(&mut self) -> i64 {
        let real_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i128;

        unsafe {
            if qemu_plugin_cpu_is_tick_enabled() && self.last_real_time != 0 {
                let advanced_vtime = (real_time - self.last_real_time) as i64;
                self.advanced_vclock += advanced_vtime / 10;
            }
        }

        self.last_real_time = real_time;

        // 5. return the calculated virtual time
        unsafe {
            self.advanced_vclock + qemu_plugin_get_snapshot_cpu_clock()
        }
    }

    pub fn reset(&mut self) {
        self.last_real_time = 0;
        self.last_icount = 0;
        self.advanced_vclock = 0;
    }
}
