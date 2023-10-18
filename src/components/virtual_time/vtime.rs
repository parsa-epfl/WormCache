use crate::qemu_api::qemu_plugin_cpu_is_tick_enabled;
use crate::qemu_api::qemu_plugin_get_snapshoted_vm_clock;
use crate::CORE_COUNT;

use std::time::SystemTime;

use super::icount::ICountPlugin;

pub struct VirtualTimeContext {
    last_real_time: i128,
    last_icounts: [u64; CORE_COUNT],
    advanced_vclock: i64,
}

impl VirtualTimeContext {
    pub fn new() -> Self {
        return Self {
            last_real_time: 0,
            last_icounts: [0; CORE_COUNT],
            advanced_vclock: 0,
        };
    }

    pub fn calculate_virtual_time(&mut self, icount: &ICountPlugin) -> i64 {
        // 1. get real timestamp in nanosecond
        let real_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i128;

        // 2. get the icounts from each core
        let icounts = icount.get_icounts();

        // 3. calculate the potential update
        unsafe {
            if qemu_plugin_cpu_is_tick_enabled() {
                // 3.1 calculate the difference icounts and find the maximum.
                let mut max_icounts = 0;
                for i in 0..CORE_COUNT {
                    let diff = icounts[i] - self.last_icounts[i];
                    if diff > max_icounts {
                        max_icounts = diff;
                    }
                }

                // 3.2 if the maximum is zero, we use the difference of the real time.
                let advanced_vtime = if max_icounts == 0 {
                    (real_time - self.last_real_time) as i64
                } else {
                    max_icounts as i64
                };

                // 3.3 update the advanced vclock
                self.advanced_vclock += advanced_vtime;
            }
        }

        // 4. update the context with the new icounts and the real time.
        self.last_icounts.copy_from_slice(&icounts);
        self.last_real_time = real_time;

        // 5. return the calculated virtual time
        // TODO: this way to calculate the time has bug when exporting multiple checkpoints
        // Because qb.qemu_plugin_get_snapshoted_vm_clock() is updated a checkpoint is exported.
        // I didn't see a better solution. Maybe storing this value inside this plugin?

        unsafe {
            return self.advanced_vclock + qemu_plugin_get_snapshoted_vm_clock();
        }
    }

    pub fn reset(&mut self) {
        self.last_real_time = 0;
        self.last_icounts = [0; CORE_COUNT];
        self.advanced_vclock = 0;
    }
}
