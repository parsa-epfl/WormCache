// This module defines the basic memory hierarchies using fine-grained locks.
// It contains the same logical memory hierarchy as the `memory`, but uses locks for shared communication.
// - TLB, which is private.
// - Private caches, with set locks.
// - Directory, with set locks.
// - Shared caches, with set locks.

use l0i::L0InstructionCache;

use crate::util::get_monotonic_ts;
use std::io::prelude::*;

use crate::{
    parameter::{self, ENABLE_STATISTICS},
    qemu_api,
};
use std::ffi;

use std::io::prelude::*;
use zstd::Encoder;

use super::debug::statistics::Statistics;

mod util;

pub mod directory;
pub mod hierarchy;
pub mod icount; // TODO: there should be a centralized icount system.
pub mod l0i;
pub mod private_cache;
pub mod shared_cache;

mod parser;

static mut ICOUNT_PLUGIN: *mut icount::ICountPlugin = std::ptr::null_mut();

unsafe extern "C" fn vcpu_increase_icount(vcpu_idx: u32, count: *mut ffi::c_void) {
    (*ICOUNT_PLUGIN).increase_icount(vcpu_idx as u8, count as u64);
}

type HierarchyForPlugin = parser::HierarchyForPlugin;

static mut PLUGIN: *mut HierarchyForPlugin = std::ptr::null_mut();

// TODO: The QEMU side has to make load-link to get exclusive permission so that the plugin can handle it properly.
unsafe extern "C" fn vcpu_mem_access(
    vcpu_idx: u32,
    info: qemu_api::qemu_plugin_meminfo_t,
    vaddr: u64,
    offset: *mut ffi::c_void,
) {
    if parameter::CACHE_HIERARCHY_FOR_HALF_OF_CORES && vcpu_idx >= parameter::CORE_COUNT as u32 / 2
    {
        return;
    }

    // let offset: u64 = offset as u64;
    // let current_icount = (*ICOUNT_PLUGIN).get_icount(vcpu_idx as u8);

    let memory_instruction_pc = offset as u64;

    let hw_handler = qemu_api::qemu_plugin_get_hwaddr(info, vaddr);
    let is_device = qemu_api::qemu_plugin_hwaddr_is_io(hw_handler);

    if !is_device {
        let is_store = qemu_api::qemu_plugin_mem_is_store(info);

        // let walk_trace = qemu_api::qemu_plugin_hwaddr_translate_walk_trace(hw_handler);
        // let walk_trace: [u64; 4] = std::slice::from_raw_parts(walk_trace, 4).try_into().unwrap();
        let pa = qemu_api::qemu_plugin_hwaddr_phys_addr(hw_handler);

        // Currently, this is experimental.
        // PLUGIN.access_memory_with_va_and_hint(vcpu_idx, vaddr, get_monotonic_ts(), is_store, false, walk_trace, pa);
        (*PLUGIN).access_memory_with_va_and_pa(
            vcpu_idx,
            vaddr,
            pa,
            get_monotonic_ts(),
            is_store,
            false,
            get_monotonic_ts(), // v_ts, not used, but instead, the monotonic timestamp is used.
            memory_instruction_pc,
        );
    } else {
        // TODO: check the I/O event
    }
}

static mut L0_CACHE: *mut L0InstructionCache<{ parser::ALLOCATED_CORE_COUNT }> =
    std::ptr::null_mut();

static mut C0_TRACE_FILE: *mut Encoder<'_, std::fs::File> = std::ptr::null_mut();

unsafe extern "C" fn vcpu_insn_exec(
    vcpu_idx: u32,
    inst_host_addr: *mut ffi::c_void, // it is basically its physical address.
) {
    if parameter::CACHE_HIERARCHY_FOR_HALF_OF_CORES && vcpu_idx >= parameter::CORE_COUNT as u32 / 2
    {
        return;
    }

    let vpn = unsafe { qemu_api::qemu_plugin_read_pc_vpn() };
    let vaddr = vpn << 12 | (inst_host_addr as u64 & 0xfff);

    let current_icount = (*ICOUNT_PLUGIN).get_icount(vcpu_idx as u8);

    if vcpu_idx == 0 {
        unsafe {
            (*C0_TRACE_FILE).write_all(&vaddr.to_le_bytes()).unwrap();
        }

        if current_icount == 100000000 {
            std::process::exit(0);
        }
    }

    let instruction_offset = inst_host_addr as u64 >> 48;
    let inst_host_addr = inst_host_addr as u64 & 0xffff_ffff_ffff;

    if (*L0_CACHE).check_and_update(vcpu_idx, vaddr) {
        // this is very necessary. It avoids the slow lookup of the basic blocks.
        // It also guarantees the traffic to the cache is similar when the tb size is forced to be 1.
        return;
    }

    if parameter::USE_QEMU_HW_ADDR_AS_PHYSICAL_PC {
        (*PLUGIN).access_memory_with_va_and_pa(
            vcpu_idx,
            vaddr,
            inst_host_addr as u64,
            get_monotonic_ts(),
            false,
            true,
            current_icount + instruction_offset,
            vaddr,
        );
    } else {
        (*PLUGIN).access_memory_with_va(
            vcpu_idx,
            vaddr,
            get_monotonic_ts(),
            false,
            true,
            current_icount + instruction_offset,
            vaddr,
        );
    }
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
            PLUGIN = Box::into_raw(Box::new(HierarchyForPlugin::new()));
            L0_CACHE = Box::into_raw(Box::new(L0InstructionCache::new()));
            ICOUNT_PLUGIN = Box::into_raw(Box::new(icount::ICountPlugin::new()));

            C0_TRACE_FILE = Box::into_raw(Box::new(
                Encoder::new(std::fs::File::create("c0_trace.zstd").unwrap(), 3).unwrap(),
            ));
        }

        if parameter::USE_UNIFIED_CACHE {
            assert!(HierarchyForPlugin::information().contains("UnifiedPrivateCache"))
        } else {
            assert!(HierarchyForPlugin::information().contains("HarvardPrivateCache"))
        }

        println!("Memory plugin initialized.");
        println!("{}", HierarchyForPlugin::information());

        // this thread peridocally dumps the statistics.
        std::thread::spawn(|| {
            if !ENABLE_STATISTICS {
                return;
            }

            // open a csv file.
            let mut miss_file = std::fs::File::create("cache-misses.csv").unwrap();
            let mut warmed_rate = std::fs::File::create("shared_cache_warm_count.csv").unwrap();

            miss_file
                .write_fmt(format_args!("{}\n", Statistics::get_header()))
                .unwrap();

            warmed_rate
                .write_all(b"ts,warm_set_count,warm_slot_count\n")
                .unwrap();

            loop {
                for stat in Statistics::global_get_line_for_all_cores(get_monotonic_ts()) {
                    miss_file.write_all(stat.as_bytes()).unwrap();
                    miss_file.write_all(b"\n").unwrap();
                }

                // get the duration of the following function.

                let now = std::time::Instant::now();

                warmed_rate
                    .write_all(
                        format!(
                            "{},{},{}\n",
                            get_monotonic_ts(),
                            unsafe { (*PLUGIN).get_scache_warmed_set_count() },
                            unsafe { (*PLUGIN).get_scache_warmed_slots_count() }
                        )
                        .as_bytes(),
                    )
                    .unwrap();

                let elapsed = now.elapsed();

                std::thread::sleep(std::time::Duration::from_secs(10) - elapsed);
            }
        });
    }

    #[inline]
    fn dump_snapshot(name: &str) {
        // if ENABLE_STATISTICS {
        //     // open a csv file and dump each cores' statistics.
        //     let mut file =
        //         std::fs::File::create(format!("{}/memory_locked_missrate.csv", name)).unwrap();

        //     file.write_fmt(format_args!("{}\n", Statistics::get_header()))
        //         .unwrap();

        //     for stat in Statistics::global_get_line_for_all_cores(get_monotonic_ts()) {
        //         file.write_all(stat.as_bytes()).unwrap();
        //         file.write_all(b"\n").unwrap();
        //     }
        // }

        // // dump the access counter of each set in the shared cache.
        // unsafe {
        //     (*PLUGIN).dump_access_counter();
        // }

        // // dump the cache state.
        // unsafe {
        //     (*PLUGIN).dump_snapshot(name);
        // }

        unsafe {
            (*PLUGIN).dump_diagnose_information();
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

            // The target virtual address should have the same page offset as the host virtual address.
            assert_eq!(
                (qemu_api::qemu_plugin_insn_vaddr(i) as u64) & 0xfff,
                (qemu_api::qemu_plugin_insn_haddr(i) as u64) & 0xfff
            );

            let insn_addr = (qemu_api::qemu_plugin_insn_haddr(i) as u64) & 0xffff_ffff_ffff;
            let offset = idx as u64;
            let combined = insn_addr | (offset << 48);

            qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                i,
                Some(vcpu_insn_exec),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                combined as *mut ffi::c_void,
            );
        }

        // bind the memory callback.
        for i in 0..n_instruction {
            let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
            let vpc = qemu_api::qemu_plugin_insn_vaddr(inst);
            qemu_api::qemu_plugin_register_vcpu_mem_cb(
                inst,
                Some(vcpu_mem_access),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                qemu_api::qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
                vpc as *mut ffi::c_void,
            );
        }

        // add the icount callback for the whole block.
        qemu_api::qemu_plugin_register_vcpu_tb_exec_cb(
            tb,
            Some(vcpu_increase_icount),
            qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
            n_instruction as *mut ffi::c_void,
        );
    }
}
