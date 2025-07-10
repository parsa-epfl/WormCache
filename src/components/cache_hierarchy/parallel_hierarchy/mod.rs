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
use std::{io::Write, sync::OnceLock};

use rustc_hash::FxHashMap;
use spin::mutex::SpinMutex;

use crate::{
    parameter::{self, ENABLE_STATISTICS},
    qemu_api,
    timestamp::get_ts, util::get_monotonic_ts
};

use super::{MemoryAccessRequest, MemoryHierarchy, common::CacheAccessType};
use super::{
    common::L0InstructionCache,
    mmu::{MMUFlushMode, tlb::AddressSpaceID},
};

pub mod hierarchy;
pub mod parser;

type HierarchyForPlugin = parser::HierarchyForPlugin;

static mut PLUGIN: *mut HierarchyForPlugin = std::ptr::null_mut();
// static mut DUMMY_PLUGIN: *mut HierarchyForPlugin = std::ptr::null_mut();

// TODO: The QEMU side has to make load-link to get exclusive permission so that the plugin can handle it properly.
unsafe extern "C" fn vcpu_mem_access(
    vcpu_idx: u32,
    info: qemu_api::qemu_plugin_meminfo_t,
    vaddr: u64,
    inst_virtual_addr: *mut ffi::c_void,
) {
    unsafe {
        let hw_handler = qemu_api::qemu_plugin_get_hwaddr(info, vaddr);
        let is_device = qemu_api::qemu_plugin_hwaddr_is_io(hw_handler);

        if !is_device {
            let is_store = qemu_api::qemu_plugin_mem_is_store(info);

            let pa = qemu_api::qemu_plugin_hwaddr_phys_addr(hw_handler);

            let inst_virtual_addr = inst_virtual_addr as u64;
            let is_os = (inst_virtual_addr >> 48) & 1 == 1;
            let offset = inst_virtual_addr >> 49;

            let ts = if parameter::USE_TARGET_TIME_FOR_CACHE_STATE_CONSTRUCTION {
                let ip10ps = qemu_api::qemu_plugin_get_vcpu_ip10ps(vcpu_idx);
                ((offset * 10000) / ip10ps) + qemu_api::qemu_plugin_get_vcpu_vtime(vcpu_idx) + 1
            } else {
                get_ts()
            };

            if parameter::MEASURE_HALF_OF_CORES && vcpu_idx >= parameter::CORE_COUNT as u32 / 2 {
                // (*DUMMY_PLUGIN).access_memory_with_va_and_pa(
                //     &MemoryAccessRequest {
                //         core_id: vcpu_idx - parameter::CORE_COUNT as u32 / 2,
                //         va: vaddr,
                //         access_type: if is_store {
                //             CacheAccessType::DataWrite
                //         } else {
                //             CacheAccessType::DataRead
                //         },
                //         is_os,
                //     },
                //     Some(pa),
                //     ts,
                // )
            } else {
                (*PLUGIN).access_memory_with_va_and_pa(
                    &MemoryAccessRequest {
                        core_id: vcpu_idx,
                        va: vaddr,
                        access_type: if is_store {
                            CacheAccessType::DataWrite
                        } else {
                            CacheAccessType::DataRead
                        },
                        is_os,
                    },
                    Some(pa),
                    ts,
                );
            };
        } else {
            // TODO: check the I/O event
        }
    }
}

static mut L0_CACHE: *mut L0InstructionCache<{ parameter::CORE_COUNT }> = std::ptr::null_mut();

unsafe extern "C" fn vcpu_insn_exec(
    vcpu_idx: u32,
    inst_virtual_addr: *mut ffi::c_void, // it is basically its physical address.
) {
    unsafe {
        let vpn = qemu_api::qemu_plugin_read_pc_vpn();
        let vaddr = vpn << 12 | (inst_virtual_addr as u64 & 0xfff);

        if (*L0_CACHE).check_and_update(vcpu_idx, vaddr) {
            return;
        }

        let offset = (inst_virtual_addr as u64) >> 49;
        let ts = if parameter::USE_TARGET_TIME_FOR_CACHE_STATE_CONSTRUCTION {
            let ip10ps = qemu_api::qemu_plugin_get_vcpu_ip10ps(vcpu_idx);
            ((offset * 10000) / ip10ps) + qemu_api::qemu_plugin_get_vcpu_vtime(vcpu_idx) + 1
        } else {
            get_ts()
        };

        if parameter::MEASURE_HALF_OF_CORES && vcpu_idx >= parameter::CORE_COUNT as u32 / 2 {
            // (*DUMMY_PLUGIN).access_memory_with_va(
            //     &MemoryAccessRequest {
            //         core_id: vcpu_idx - parameter::CORE_COUNT as u32 / 2,
            //         va: vaddr,
            //         access_type: CacheAccessType::InstructionFetch,
            //         is_os: vaddr >> 63 == 1,
            //     },
            //     ts,
            // );
        } else {
            (*PLUGIN).access_memory_with_va(
                &MemoryAccessRequest {
                    core_id: vcpu_idx,
                    va: vaddr,
                    access_type: CacheAccessType::InstructionFetch,
                    is_os: vaddr >> 63 == 1,
                },
                ts,
            );
        }
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

unsafe extern "C" fn vcpu_invalid_tlb(
    vcpu_idx: u32,
    mode: u32,
    asid: u64,
    vpn: u64,
    page_count: u64,
) {
    unsafe {
        let info = if mode == 0 {
            MMUFlushMode::All
        } else if mode == 1 {
            MMUFlushMode::ByASID(AddressSpaceID::NonGlobal(asid as u16))
        } else if mode == 2 {
            MMUFlushMode::ByVPN(vpn, page_count)
        } else if mode == 3 {
            MMUFlushMode::ByVPNAndASID(vpn, page_count, AddressSpaceID::NonGlobal(asid as u16))
        } else {
            unreachable!()
        };

        if parameter::MEASURE_HALF_OF_CORES && vcpu_idx >= parameter::CORE_COUNT as u32 / 2 {
            // (*DUMMY_PLUGIN).flush_mmu(vcpu_idx - parameter::CORE_COUNT as u32 / 2, info);
        } else {
            (*PLUGIN).flush_mmu(vcpu_idx, info);
        }
    }
}

static SNAPSHOT_INFO: SpinMutex<Option<(String, u64)>> = SpinMutex::new(None);

unsafe extern "C" fn event_loop_callback() {
    unsafe {
        let snapshot_info_guard = SNAPSHOT_INFO.try_lock();
        if snapshot_info_guard.is_none() {
            return;
        }

        let mut snapshot_info_guard = snapshot_info_guard.unwrap();

        if snapshot_info_guard.is_none() {
            return;
        }

        let snapshot_info = snapshot_info_guard.take().unwrap();

        println!("Snapshot request: {}", &snapshot_info.0);

        let c_snapshot_name = std::ffi::CString::new(snapshot_info.0.clone()).unwrap();

        qemu_api::qemu_plugin_savevm(
            c_snapshot_name.as_ptr(),
            qemu_api::qemu_plugin_snapshot_format_t_QEMU_PLUGIN_SNAPSHOT_FORMAT_EXTERNAL_INCREMENTAL_BASE,
        );

        std::process::exit(0);
    }
}

static SNAPSHOT_NAME: OnceLock<String> = OnceLock::new();
static WARM_RATIO: OnceLock<f64> = OnceLock::new();

unsafe extern "C" fn quantum_checking_callback(_: u64) -> bool {
    let warmed_set = unsafe { (*PLUGIN).get_scache_warmed_set_count() };
    let warm_ratio = *WARM_RATIO.get().unwrap();

    if warmed_set >= (parameter::SHARED_CACHE_SET as f64 * warm_ratio) as usize {
        let snapshot_info = (SNAPSHOT_NAME.get().unwrap().clone(), 0);

        let snapshot_info_guard = SNAPSHOT_INFO.try_lock();
        if snapshot_info_guard.is_none() {
            return false;
        }

        let mut snapshot_info_guard = snapshot_info_guard.unwrap();

        if snapshot_info_guard.is_none() {
            *snapshot_info_guard = Some(snapshot_info);
            println!("All the sets are warmed up. Create a snapshot.");
            return true; // suggest a interrupt.
        }
    }
    return false;
}

pub struct ParallelCacheHierarchyPlugin {}

impl super::super::Plugin for ParallelCacheHierarchyPlugin {
    #[inline]
    fn init(_plugin_id: u64, options: &FxHashMap<String, String>) {
        let mode = String::new();
        let mode = options.get("mode").unwrap_or(&mode);
        assert_ne!(
            mode, "vtime",
            "Pure vtime is enabled. Memory Hierarchy should be disabled."
        );

        let pure_fill_mode = mode == "pure_fill";

        if pure_fill_mode {
            unsafe {
                assert!(qemu_api::qemu_plugin_register_event_loop_poll_cb(Some(
                    event_loop_callback
                )));

                assert!(qemu_api::qemu_plugin_register_periodic_check_cb(Some(
                    quantum_checking_callback
                )));
            }
        }

        let prefix = options.get("prefix").unwrap_or(&"".to_string()).clone();
        SNAPSHOT_NAME
            .set(format!("{}_{}", prefix, "warmed"))
            .unwrap();

        unsafe {
            let quantum_size = qemu_api::qemu_plugin_get_quantum_size();
            let is_icount_mode = qemu_api::qemu_plugin_is_icount_mode();

            PLUGIN = Box::into_raw(Box::new(HierarchyForPlugin::new(
                true,
                quantum_size,
                is_icount_mode,
            )));
            L0_CACHE = Box::into_raw(Box::new(L0InstructionCache::new()));

            // if parameter::MEASURE_HALF_OF_CORES {
            // DUMMY_PLUGIN =
            // Box::into_raw(Box::new(HierarchyForPlugin::new(false, 0, is_icount_mode)));
            // }

            qemu_api::qemu_plugin_register_flushing_local_tlb_cb(Some(vcpu_invalid_tlb));

            // qemu_api::qemu_plugin_register_periodic_check_cb(Some(dump_statistics));
        }

        if parameter::USE_UNIFIED_CACHE {
            assert!(HierarchyForPlugin::information().contains("UnifiedPrivateCache"))
        } else {
            assert!(HierarchyForPlugin::information().contains("HarvardPrivateCache"))
        }

        println!("Memory plugin initialized.");
        println!("{}", HierarchyForPlugin::information());

        // this thread peridocally dumps the statistics.
        std::thread::spawn(move || {
            if !ENABLE_STATISTICS {
                return;
            }

            // open a csv file.
            let mut warmed_rate = std::fs::File::create("shared_cache_warm_count.csv").unwrap();

            warmed_rate
                .write_all(b"ts,warm_set_count,warm_slot_count\n")
                .unwrap();

            loop {
                let warmed_set = unsafe { (*PLUGIN).get_scache_warmed_set_count() };

                warmed_rate
                    .write_all(
                        format!("{},{},{}\n", get_monotonic_ts(), warmed_set, unsafe {
                            (*PLUGIN).get_scache_warmed_slots_count()
                        })
                        .as_bytes(),
                    )
                    .unwrap();

                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        });
    }

    #[inline]
    unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
        unsafe {
            let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

            if n_instruction == 0 {
                return;
            }

            assert!(n_instruction < 32768);

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

                let insn_addr = (qemu_api::qemu_plugin_insn_vaddr(i) as u64) & 0x1_ffff_ffff_ffff;
                let offset = idx as u64;
                let combined = insn_addr | (offset << 49);

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

                let insn_addr =
                    (qemu_api::qemu_plugin_insn_vaddr(inst) as u64) & 0x1_ffff_ffff_ffff;
                let offset = i as u64;
                let combined = insn_addr | (offset << 49);

                qemu_api::qemu_plugin_register_vcpu_mem_cb(
                    inst,
                    Some(vcpu_mem_access),
                    qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                    qemu_api::qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
                    combined as *mut ffi::c_void,
                );
            }
        }
    }

    fn serialize(name: &str) {
        unsafe {
            (*PLUGIN).serialize(name, 0);
            // if parameter::MEASURE_HALF_OF_CORES {
            // (*DUMMY_PLUGIN).serialize(name, 1);
            // }
        }
    }

    fn deserialize(name: &str) {
        unsafe {
            (*PLUGIN).deserialize(name, 0);
            // if parameter::MEASURE_HALF_OF_CORES {
            // (*DUMMY_PLUGIN).deserialize(name, 1);
            // }
        }
    }
}
