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

// This plugin capture the hint instruction in ARM and print debug information.

use rustc_hash::FxHashMap;

use crate::qemu_api;
use std::ffi;

pub struct MarkerPlugin {}

unsafe extern "C" fn on_hint_executed(vcpu_index: u32, hint_value: *mut ffi::c_void) {
    let hint_value = hint_value as u32;

    if hint_value == 110 {
        // print the current timestamp, in us.
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_micros();
        println!("{}: vcpu {} executed hint #91.", ts, vcpu_index);
    }
}

impl super::Plugin for MarkerPlugin {
    fn init(_plugin: u64, _options: &FxHashMap<String, String>) {
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
            let hint_opcode = 0b1101_0101_0000_0011_0010_0000_0001_1111_u32;
            let hint_mask = 0b1111_1111_1111_1111_1111_0000_0001_1111_u32;
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

    fn serialize(_: &str) {}

    fn deserialize(_: &str) {}
}
