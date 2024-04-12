// This module defines the basic memory hierarchies using fine-grained locks.
// It contains the same logical memory hierarchy as the `memory`, but uses locks for shared communication.
// - TLB, which is private.
// - Private caches, with set locks.
// - Directory, with set locks.
// - Shared caches, with set locks.

use once_cell::sync::Lazy;
use std::io::prelude::*;

use crate::{
    arch::AArch64,
    parameter::{self, ENABLE_STATISTICS},
    qemu_api,
};
use std::ffi;

use self::private_cache::{HarvardPrivateCaches};

use super::debug::statistics::Statistics;

pub mod directory;
mod hierarchy;
mod private_cache;
pub mod shared_cache;

type AArch64MMU = crate::components::mmu::MemoryManagementUnit<
    AArch64,
    { parameter::TLB_ASSO },
    { parameter::TLB_SET },
>;

type PluginMemoryHierarchyHarvard = hierarchy::LockedMemoryHierarchy<
    AArch64MMU,
    HarvardPrivateCaches<
        { parameter::CORE_COUNT },
        { parameter::HARVARD_PRI_I_CACHE_SET },
        { parameter::HARVARD_PRI_I_CACHE_ASSO },
        { parameter::HARVARD_PRI_D_CACHE_SET },
        { parameter::HARVARD_PRI_D_CACHE_ASSO },
    >,
>;

static mut PLUGIN: Lazy<PluginMemoryHierarchyHarvard> =
    Lazy::new(|| PluginMemoryHierarchyHarvard::new());

pub fn get_memory_ts() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

// TODO: The QEMU side has to make load-link to get exclusive permission so that the plugin can handle it properly.
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

        // let walk_trace = qemu_api::qemu_plugin_hwaddr_translate_walk_trace(hw_handler);
        // let walk_trace: [u64; 4] = std::slice::from_raw_parts(walk_trace, 4).try_into().unwrap();
        let pa = qemu_api::qemu_plugin_hwaddr_phys_addr(hw_handler);

        // Currently, this is experimental.
        // PLUGIN.access_memory_with_va_and_hint(vcpu_idx, vaddr, get_memory_ts() as u64, is_store, false, walk_trace, pa);
        PLUGIN.access_memory_with_va_and_pa(
            vcpu_idx,
            vaddr,
            pa,
            get_memory_ts() as u64,
            is_store,
            false,
        );
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

    PLUGIN.access_memory_with_va(vcpu_idx, vaddr, get_memory_ts() as u64, false, true);
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

pub struct ParallelCacheHierarchyPlugin {}

impl super::Plugin for ParallelCacheHierarchyPlugin {
    #[inline]
    fn init() {
        unsafe {
            Lazy::force(&PLUGIN);
        }

        println!("Memory[Locked] plugin initialized.");
    }

    #[inline]
    fn dump_snapshot(name: &str) {
        if ENABLE_STATISTICS {
            // open a csv file and dump each cores' statistics.
            let mut file =
                std::fs::File::create(format!("{}/memory_locked_missrate.csv", name)).unwrap();

            file.write_fmt(format_args!("{}\n", Statistics::get_header()))
                .unwrap();

            for stat in Statistics::global_get_line_for_all_cores(get_memory_ts() as u64) {
                file.write(stat.as_bytes()).unwrap();
                file.write(b"\n").unwrap();
            }
        }

        // dump the access counter of each set in the shared cache.
        unsafe {
            PLUGIN.dump_access_counter();
        }

        // dump the cache state.
        unsafe {
            PLUGIN.dump_snapshot(name);
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
