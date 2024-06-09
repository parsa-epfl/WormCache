use crate::parameter as param;
use crate::qemu_api::qemu_plugin_cpu_is_tick_enabled;
use crate::qemu_api::qemu_plugin_get_snapshot_cpu_clock;

pub struct VirtualTimeContext {
    last_real_time: u64,
    advanced_vclock: i64,
    time_scaling_factor: u64,
}

impl VirtualTimeContext {
    pub fn new() -> Self {
        Self {
            last_real_time: 0,
            advanced_vclock: 0,
            time_scaling_factor: param::INIT_HOST_TIME_SCALE as u64,
        }
    }

    pub fn update_scaling_factor(&mut self, scaling_factor: u64) {
        if scaling_factor == 0 {
            panic!("The scaling factor should not be zero.");
        }
        self.time_scaling_factor = scaling_factor;
    }

    pub fn calculate_cpu_clock(&mut self) -> i64 {
        // 1. get real timestamp in nanosecond
        // let real_time = SystemTime::now()
        //     .duration_since(SystemTime::UNIX_EPOCH)
        //     .unwrap()
        //     .as_nanos() as i128;

        let real_time = crate::util::get_monotonic_ts();

        // 2. calculate the potential update
        unsafe {
            if qemu_plugin_cpu_is_tick_enabled() && self.last_real_time != 0 {
                // 3.2 if the maximum is zero, we use the difference of the real time.
                let advanced_vtime =
                    ((real_time - self.last_real_time) / self.time_scaling_factor) as i64;

                // 3.3 update the advanced vclock
                self.advanced_vclock += advanced_vtime;
            }
        }

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

        unsafe {
            if qemu_plugin_cpu_is_tick_enabled() && self.last_real_time != 0 {
                let advanced_vtime = (real_time - self.last_real_time) as i64;
                self.advanced_vclock += advanced_vtime / 10;
            }
        }

        self.last_real_time = real_time;

        // 5. return the calculated virtual time
        unsafe { self.advanced_vclock + qemu_plugin_get_snapshot_cpu_clock() }
    }

    pub fn reset(&mut self) {
        self.last_real_time = 0;
        self.advanced_vclock = 0;
    }
}
