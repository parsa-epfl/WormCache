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

use std::ffi;

use crate::qemu_api;

use crate::debug::statistics::*;

unsafe extern "C" fn vcpu_exec_wfi(vcpu_idx: u32, _: *mut ffi::c_void) {
    Statistics::global_record(vcpu_idx, EventType::WaitForInterrupt, true);
}

pub struct WaitForInterruptCounterPlugin {}

impl super::Plugin for WaitForInterruptCounterPlugin {
    fn init(_plugin_id: u64, _options: &rustc_hash::FxHashMap<String, String>) {}

    unsafe fn on_translation(tb: *mut qemu_api::qemu_plugin_tb) {
        unsafe {
            let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

            // go over all instructions and check which one is WFI.

            for i in 0..n_instruction {
                let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
                let literal = qemu_api::qemu_plugin_insn_data(inst) as *const u32;
                let literal = *literal;

                if literal == 0b_1101_0101_0000_0011_0010_0000_0111_1111 {
                    qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                        inst,
                        Some(vcpu_exec_wfi),
                        qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        std::ptr::null_mut(),
                    );
                }
            }
        }
    }

    fn serialize(_name: &str) {}

    fn deserialize(_name: &str) {}
}
