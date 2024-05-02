mod icount;
mod vtime;

use core::ffi;
use once_cell::sync::Lazy;
use std::{io::Write, sync::Mutex};

use crate::parameter as param;
use crate::qemu_api;

use crate::util::get_monotonic_ts;

static TIME_PLUGIN: Lazy<Mutex<vtime::VirtualTimeContext>> =
    Lazy::new(|| Mutex::new(vtime::VirtualTimeContext::new()));

static mut ICOUNT_PLUGIN: Lazy<icount::ICountPlugin> = Lazy::new(icount::ICountPlugin::new);

unsafe extern "C" fn calculate_cpu_clock() -> i64 {
    return TIME_PLUGIN
        .lock()
        .unwrap()
        .calculate_cpu_clock(&ICOUNT_PLUGIN);
}

unsafe extern "C" fn on_snapshot_cpu_clock_update() {
    TIME_PLUGIN.lock().unwrap().reset();
    ICOUNT_PLUGIN.reset();
}

unsafe extern "C" fn user_vcpu_insn_exec(
    vcpu_idx: u32,
    size: *mut ffi::c_void, // the size of the basic block
) {
    ICOUNT_PLUGIN.increase_user_icount(vcpu_idx as u8, size as u64);
}

unsafe extern "C" fn kernel_vcpu_insn_exec(
    vcpu_idx: u32,
    size: *mut ffi::c_void, // the size of the basic block
) {
    ICOUNT_PLUGIN.increase_kernel_icount(vcpu_idx as u8, size as u64);
}

pub struct VirtualTimePlugin {}

impl super::Plugin for VirtualTimePlugin {
    #[inline]
    fn init() {
        assert!(unsafe { qemu_api::qemu_plugin_register_cpu_clock_cb(Some(calculate_cpu_clock)) });

        assert!(unsafe {
            qemu_api::qemu_plugin_register_snapshot_cpu_clock_update_cb(Some(
                on_snapshot_cpu_clock_update,
            ))
        });

        std::thread::spawn(|| {
            // open a csv file to store the icounts.
            let mut file = std::fs::File::create("cache-icount.csv").unwrap();
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

            loop {
                let icounts = unsafe { ICOUNT_PLUGIN.get_icounts() };
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
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        });
        println!("Virtual time plugin initialized.");
    }

    #[inline]
    fn dump_snapshot(_: &str) {}

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
}
