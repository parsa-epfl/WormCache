mod tage;

mod aarch64;
mod callbacks;
use super::Plugin;
use crate::qemu_api;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::ffi;
use std::sync::Mutex;

struct BranchMetaData {
    pc: u64,
    instruction: u32,
}

static mut BRANCH_METADATA: Lazy<Mutex<HashMap<usize, Box<BranchMetaData>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub struct BranchPredictorPlugin {}

impl Plugin for BranchPredictorPlugin {
    fn init() {
        println!("PageWalkLoggerPlugin initialized.");
        unsafe {
            BRANCH_METADATA.lock().unwrap().clear();
        }
    }

    unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
        let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

        if n_instruction == 0 {
            return;
        }

        for i in 0..n_instruction {
            let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
            let data_ptr = qemu_api::qemu_plugin_insn_data(inst) as *const u8;
            let data_size = qemu_api::qemu_plugin_insn_size(inst);
            assert!(data_size == 4);
            let instruction_bits = std::slice::from_raw_parts(data_ptr, data_size);
            let instruction = u32::from_le_bytes(instruction_bits.try_into().unwrap());
            let branch_type = aarch64::branch_type(instruction);
            if let Some(branch_type) = branch_type {
                let branch_metadata = Box::new(BranchMetaData {
                    pc: qemu_api::qemu_plugin_insn_vaddr(inst),
                    instruction,
                });
                let branch_metadata_ptr = Box::into_raw(branch_metadata);
                let branch_metadata_ptr = branch_metadata_ptr as usize;
                BRANCH_METADATA.lock().unwrap().insert(
                    branch_metadata_ptr,
                    Box::from_raw(branch_metadata_ptr as *mut BranchMetaData),
                );
                qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                    inst,
                    Some(callbacks::get_callback(branch_type)),
                    qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                    branch_metadata_ptr as *mut ffi::c_void,
                );

                let instruction_disassembly = qemu_api::qemu_plugin_insn_disas(inst);
                let instruction_disassembly = ffi::CStr::from_ptr(instruction_disassembly);
                let instruction_disassembly = instruction_disassembly.to_str().unwrap();

                let opcode = instruction_disassembly.split(' ').next().unwrap();

                if aarch64::check_opcode_match(opcode, branch_type) != true {
                    panic!(
                        "Opcode mismatch: opcode: {}, branch_type: {:?}",
                        opcode, branch_type
                    );
                }
            }
        }
    }

    fn dump_snapshot() {}
}
