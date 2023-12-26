use super::Plugin;
use crate::qemu_api;
use std::ffi::{self, c_void};

unsafe extern "C" fn vcpu_mem_access(
    cpu_idx: u32,
    info: qemu_api::qemu_plugin_meminfo_t,
    vaddr: u64,
    _: *mut ffi::c_void, // should be NULL.
) {
    let hw_handler = qemu_api::qemu_plugin_get_hwaddr(info, vaddr);
    let is_device = qemu_api::qemu_plugin_hwaddr_is_io(hw_handler);

    if !is_device {
        let is_store = qemu_api::qemu_plugin_mem_is_store(info);
        let paddr = qemu_api::qemu_plugin_hwaddr_phys_addr(hw_handler) as usize;

        let traces = std::slice::from_raw_parts(qemu_api::qemu_plugin_hwaddr_translate_walk_trace(hw_handler), 4);

        println!("Walk trace for {:x}: ", paddr);

        for t in traces.iter() {
            if *t == u64::MAX {
                break;
            }
            let mut buf : u64 = 0;
            qemu_api::qemu_plugin_read_physical_memory(*t, 8, &mut buf as *mut u64 as *mut c_void);
            println!("- {:x} -> {:x}", *t, buf);
        }

    } else {
        // TODO: check the I/O event
    }
}

unsafe extern "C" fn vcpu_insn_exec(
    vcpu_idx: u32,
    paddr: *mut ffi::c_void, // it is basically its physical address.
) {
    // TODO: this part of virtual address is ignored. Should be more careful. 
}

pub struct PageWalkLoggerPlugin {}

impl Plugin for PageWalkLoggerPlugin {
    fn init() {
        println!("PageWalkLoggerPlugin initialized.");
    }

    unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
        let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

        if n_instruction == 0 {
            return;
        }

        let mut block_id = vec![];
        for i in 0..n_instruction {
            let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
            block_id.push(
                qemu_api::qemu_plugin_insn_haddr(inst) as usize
                    >> crate::parameter::CACHE_LINE_SIZE.trailing_zeros(),
            );
        }

        let fb_info = crate::util::find_fetch_block_from_block_id_sequence(block_id);

        // bind the instruction call back.
        for (idx, _) in fb_info.into_iter() {
            let i = qemu_api::qemu_plugin_tb_get_insn(tb, idx);
            qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                i,
                Some(vcpu_insn_exec),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                qemu_api::qemu_plugin_insn_haddr(i) as *mut ffi::c_void,
            );
        }

        // bind the memory callback.
        for i in 0..n_instruction {
            let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
            qemu_api::qemu_plugin_register_vcpu_mem_cb(
                inst,
                Some(vcpu_mem_access),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                qemu_api::qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
                std::ptr::null_mut(),
            );
        }
    }

    fn dump_snapshot() {
        
    }
}