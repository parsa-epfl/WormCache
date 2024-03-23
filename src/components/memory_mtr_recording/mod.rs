use once_cell::sync::Lazy;

use crate::arch::AArch64;
use crate::qemu_api;

use super::mmu::{AbstractMMU, MemoryManagementUnit};
use super::Plugin;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi;

#[derive(PartialEq, Eq)]
enum MTR {
    W(u64),        // ts
    RAW(u64, u64), // (read_ts, write_ts)
    R(u64),        // (read_ts)
    I(u64),        // (fetch_ts)
}

impl MTR {
    fn to_string(&self) -> String {
        match self {
            MTR::W(ts) => format!("W(ts = {})", ts),
            MTR::RAW(read_ts, write_ts) => {
                format!("RAW(read_ts = {}, write_ts = {})", read_ts, write_ts)
            }
            MTR::R(ts) => format!("R(ts = {})", ts),
            MTR::I(ts) => format!("I(ts = {})", ts),
        }
    }
}

#[repr(align(64))]
struct MTRPerCoreHierarchy<MMU: AbstractMMU> {
    mtr: HashMap<u64, MTR>, // block_id -> MTR
    mmu: MMU,
}

impl<MMU: AbstractMMU> MTRPerCoreHierarchy<MMU> {
    fn new() -> Self {
        Self {
            mtr: HashMap::new(),
            mmu: MMU::new(),
        }
    }

    pub fn access(&mut self, vaddr: u64, ts: u64, is_write: bool, is_instruction: bool) {
        // let block_id = self.mmu.get_block_id(vaddr);
        // self.access_with_pblock_id(block_id, ts, is_write, is_instruction);
        let translation_result = self.mmu.translate_and_refill(vaddr, ts);
        match translation_result {
            super::mmu::MMUTranslationResult::Hit(pa) => {
                let blocked_id = pa >> crate::parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_with_pblock_id(blocked_id, ts, is_write, is_instruction);
            }
            super::mmu::MMUTranslationResult::Miss(pa, walk_trace) => {
                for walk_trace_pa in walk_trace {
                    let blocked_id =
                        walk_trace_pa >> crate::parameter::CACHE_LINE_SIZE.trailing_zeros();
                    self.access_with_pblock_id(blocked_id, ts, false, false);
                }
                let blocked_id = pa >> crate::parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_with_pblock_id(blocked_id, ts, is_write, is_instruction);
            }
            super::mmu::MMUTranslationResult::MissNotCacheable(pa) => {
                let blocked_id = pa >> crate::parameter::CACHE_LINE_SIZE.trailing_zeros();
                self.access_with_pblock_id(blocked_id, ts, is_write, is_instruction);
            }
        }
    }

    fn access_with_pblock_id(
        &mut self,
        block_id: u64,
        ts: u64,
        is_write: bool,
        is_instruction: bool,
    ) {
        match self.mtr.get_mut(&block_id) {
            Some(mtr) => {
                if is_write {
                    *mtr = MTR::W(ts);
                } else if is_instruction {
                    *mtr = MTR::I(ts);
                } else {
                    match mtr {
                        MTR::W(write_ts) => {
                            *mtr = MTR::RAW(ts, *write_ts);
                        }
                        MTR::RAW(_, write_ts) => {
                            *mtr = MTR::RAW(ts, *write_ts);
                        }
                        _ => {
                            *mtr = MTR::R(ts);
                        }
                    }
                }
            }
            None => {
                self.mtr.insert(
                    block_id,
                    match (is_write, is_instruction) {
                        (true, false) => MTR::W(ts),
                        (false, false) => MTR::R(ts),
                        (true, true) => unreachable!(),
                        (false, true) => MTR::I(ts),
                    },
                );
            }
        }
    }

    fn dump_to_file(&self, dump_file: &mut impl std::io::Write) {
        for (block_id, mtr) in &self.mtr {
            writeln!(
                dump_file,
                "block_id = {}, mtr = {}",
                block_id,
                mtr.to_string()
            )
            .unwrap();
        }
    }
}

struct MTRMemoryHierarchy<MMU: AbstractMMU> {
    hierarchies: [MTRPerCoreHierarchy<MMU>; crate::parameter::CORE_COUNT],
}

impl<MMU: AbstractMMU> MTRMemoryHierarchy<MMU> {
    fn new() -> Self {
        Self {
            hierarchies: std::array::from_fn(|_| MTRPerCoreHierarchy::new()),
        }
    }

    pub fn access(
        &mut self,
        core_id: usize,
        vaddr: u64,
        ts: u64,
        is_write: bool,
        is_instruction: bool,
    ) {
        self.hierarchies[core_id].access(vaddr, ts, is_write, is_instruction);
    }

    pub fn dump(&self, snapshot_name: &str) {
        for (core_id, hierarchy) in self.hierarchies.iter().enumerate() {
            let mut dump_file =
                std::fs::File::create(format!("{}/mtr_core_{}.txt", snapshot_name, core_id))
                    .unwrap();
            hierarchy.dump_to_file(&mut dump_file);
        }
    }
}

pub fn get_memory_ts() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

static mut PLUGIN: Lazy<
    UnsafeCell<
        MTRMemoryHierarchy<
            MemoryManagementUnit<
                AArch64,
                { crate::parameter::TLB_ASSO },
                { crate::parameter::TLB_SET },
            >,
        >,
    >,
> = Lazy::new(|| UnsafeCell::new(MTRMemoryHierarchy::new()));

pub struct MTRMemoryPlugin {}

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

        let h = &mut *PLUGIN.get();
        h.access(
            cpu_idx as usize,
            vaddr,
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
    let h = &mut *PLUGIN.get();
    h.access(
        vcpu_idx as usize,
        vaddr,
        get_memory_ts() as u64,
        false,
        true,
    );
}

impl Plugin for MTRMemoryPlugin {
    #[inline]
    fn init() {
        unsafe {
            Lazy::force(&PLUGIN);
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

    #[inline]
    fn dump_snapshot(name: &str) {
        unsafe {
            (&*PLUGIN.get()).dump(name);
        }
    }
}
