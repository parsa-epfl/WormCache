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

use core::ffi;

use crate::{parameter::CORE_COUNT, qemu_api};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct InstructionFrequency {
    #[serde_as(as = "[_; CORE_COUNT]")]
    pub frequencies: [FxHashMap<u64, u64>; CORE_COUNT], // PC -> frequency
}

static mut PLUGIN: *mut InstructionFrequency = std::ptr::null_mut();

unsafe extern "C" fn vcpu_insn_exec(vcpu_idx: u32, inst_virtual_addr: *mut ffi::c_void) {
    let vpn = unsafe { qemu_api::qemu_plugin_read_pc_vpn() };
    let vaddr = vpn << 12 | (inst_virtual_addr as u64 & 0xfff);

    let plugin = unsafe { &mut *PLUGIN };
    let freq = plugin.frequencies[vcpu_idx as usize]
        .entry(vaddr)
        .or_insert(0);
    *freq += 1;
}

pub struct InstructionFrequencyPlugin {}

impl super::super::Plugin for InstructionFrequencyPlugin {
    fn init(_plugin_id: u64, _options: &FxHashMap<String, String>) {
        let plugin = InstructionFrequency {
            frequencies: std::array::from_fn(|_| FxHashMap::default()),
        };

        unsafe {
            PLUGIN = Box::into_raw(Box::new(plugin));
        }

        println!("InstructionFrequencyPlugin initialized");
    }

    unsafe fn on_translation(tb: *mut qemu_api::qemu_plugin_tb) {
        unsafe {
            let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

            if n_instruction == 0 {
                return;
            }

            for i in 0..n_instruction {
                let insn = qemu_api::qemu_plugin_tb_get_insn(tb, i);
                let insn_addr = qemu_api::qemu_plugin_insn_vaddr(insn);

                qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                    insn,
                    Some(vcpu_insn_exec),
                    qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                    insn_addr as *mut ffi::c_void,
                );
            }
        }
    }

    fn serialize(name: &str) {
        let file = std::fs::File::create(format!("{}/inst_frequency.json.zstd", name)).unwrap();
        let mut file = zstd::Encoder::new(file, 0).unwrap();

        let plugin = unsafe { &*PLUGIN };

        serde_json::to_writer(&mut file, plugin).unwrap();

        file.finish().unwrap();
    }

    fn deserialize(_name: &str) {}
}
