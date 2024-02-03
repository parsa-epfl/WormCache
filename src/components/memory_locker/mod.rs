// This module defines the basic memory hierarchies using fine-grained locks.
// It contains the same logical memory hierarchy as the `memory`, but uses locks for shared communication.
// - TLB, which is private.
// - Private caches, with set locks.
// - Directory, with set locks.
// - Shared caches, with set locks.

use once_cell::sync::Lazy;
use std::io::prelude::*;

use crate::{parameter::ENABLE_STATISTICS, qemu_api};
use std::ffi;

pub mod directory;
mod hierarchy;
mod private_cache;
pub mod shared_cache;
pub mod statistics;

static mut PLUGIN: Lazy<hierarchy::LockedMemoryHierarchy> =
    Lazy::new(|| hierarchy::LockedMemoryHierarchy::new());

pub fn get_memory_ts() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

unsafe extern "C" fn vcpu_mem_access(
    vcpu_idx: u32,
    info: qemu_api::qemu_plugin_meminfo_t,
    vaddr: u64,
    _: *mut ffi::c_void, // should be NULL.
) {
    let hw_handler = qemu_api::qemu_plugin_get_hwaddr(info, vaddr);
    let is_device = qemu_api::qemu_plugin_hwaddr_is_io(hw_handler);

    if !is_device {
        let is_store = qemu_api::qemu_plugin_mem_is_store(info);

        // PLUGIN.get_mut().hierarchies(cpu_idx as u8).access_memory(
        //     get_memory_ts() as usize,
        //     vaddr,
        //     false,
        //     is_store,
        // )

        PLUGIN.access_memory_with_va(vcpu_idx, vaddr, get_memory_ts() as u64, is_store, false);
    } else {
        // TODO: check the I/O event
    }
}

unsafe extern "C" fn vcpu_insn_exec(
    vcpu_idx: u32,
    voffset: *mut ffi::c_void, // it is basically its physical address.
) {
    let vpn = unsafe { qemu_api::qemu_plugin_read_pc_vpn() };
    let vaddr = vpn << 12 | (voffset as u64 & 0xfff);

    PLUGIN.access_memory_with_va(vcpu_idx, vaddr, get_memory_ts() as u64, false, false);
}

// TODO: One additional PluginAPI is needed for this instruction. It will be a similar function to the memory access.
unsafe extern "C" fn _vcpu_invalidate_cache(
    _vcpu_idx: u32,
    _paddr: *mut ffi::c_void, // it is basically its physical address.
) {
    // PLUGIN
    //     .hierarchies(vcpu_idx as u8)
    //     .invalidate(paddr as usize, get_memory_ts() as usize);
}

pub struct LockedMemoryPlugin {}

impl super::Plugin for LockedMemoryPlugin {
    #[inline]
    fn init() {
        unsafe {
            Lazy::force(&PLUGIN);
        }

        println!("Memory[Locked] plugin initialized.");
    }

    #[inline]
    fn dump_snapshot() {
        if ENABLE_STATISTICS {
            // open a csv file and dump each cores' statistics.
            let mut file = std::fs::File::create("memory_locked_missrate.csv").unwrap();
            file.write(b"core_id,total_mem,private_cache_miss,shared_cache_access,private_cache_miss_ratio,shared_cache_access_ratio\n")
            .unwrap();
            for i in 0..crate::parameter::CORE_COUNT {
                let stats = unsafe { PLUGIN.get_statistics(i as u32) };
                file.write(stats.as_bytes()).unwrap();
                file.write(b"\n").unwrap();
            }
        }
    }

    #[inline]
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
                (qemu_api::qemu_plugin_insn_vaddr(i) & 0xfff) as *mut ffi::c_void,
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
}
