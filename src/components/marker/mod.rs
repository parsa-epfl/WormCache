// This plugin capture the hint instruction in ARM and print debug information.

use crate::qemu_api;
use std::ffi;

pub struct MarkerPlugin {}

unsafe extern "C" fn on_hint_executed(vcpu_index: u32, hint_value: *mut ffi::c_void) {
    let hint_value = hint_value as u32;

    if hint_value == 91 {
        // print the current timestamp, in us.
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_micros();
        println!("{}: vcpu {} executed hint #91.", ts, vcpu_index);
    }
}

impl super::Plugin for MarkerPlugin {
    fn init() {
        println!("MarkerPlugin init. This plugin targets the hint instruction in aarch64.");
    }

    unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
        // go over the instruction and check its type.
        let n_inst = qemu_api::qemu_plugin_tb_n_insns(tb);
        for i in 0..n_inst {
            let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
            let literal = qemu_api::qemu_plugin_insn_data(inst) as *const u32;
            let literal: u32 = *literal;
            // decode the instruction
            let hint_opcode = 0b1101_0101_0000_0011_0010_0000_0001_1111 as u32;
            let hint_mask = 0b1111_1111_1111_1111_1111_0000_0001_1111 as u32;
            if (literal & hint_mask) == hint_opcode {
                // OK, this is an hint instruction.
                let hint_value = (literal >> 5) & 0b1111111;
                if hint_value > 90 {
                    // Well, we only instrument the hint instruction with value larger than 90.
                    qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                        inst,
                        Some(on_hint_executed),
                        qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        hint_value as *mut ffi::c_void,
                    );
                }
            }
        }
    }

    fn dump_snapshot() {
        
    }
}
