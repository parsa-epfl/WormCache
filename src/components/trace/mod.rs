use std::{
    ffi,
    fs::File,
    io::{BufWriter, Write},
    process::exit,
};

use zstd::Encoder;

static mut TRACE_FILE: *mut Encoder<BufWriter<File>> = std::ptr::null_mut();

static mut C0_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

use crate::qemu_api;

unsafe extern "C" fn vcpu_insn_exec(vcpu_idx: u32, host_va: *mut ffi::c_void) {
    if !vcpu_idx == 0 {
        return;
    }

    let host_va_u64 = host_va as u64;
    let pc = qemu_api::qemu_plugin_read_pc_vpn() << 12 | host_va_u64 & 0xfff;
    let instruction_literal = unsafe { *(host_va as *mut u32) };

    // write the instruction to the trace file.
    unsafe { writeln!(*TRACE_FILE, "{:x},{:x}", pc, instruction_literal).unwrap() };

    // increment the counter.
    unsafe {
        if C0_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 20000000 {
            exit(0);
        }
    };
}

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
    } else {
        // TODO: check the I/O event
    }
}

// This structure is just a wrapper of for the plugin system to register. Plugin is believed to be globally singleton.
pub struct TracePlugin {}

impl super::Plugin for TracePlugin {
    #[inline]
    fn init() {
        unsafe {
            TRACE_FILE = Box::into_raw(Box::new(
                Encoder::new(
                    BufWriter::new(File::create("worm_cache.c0.trace.zst").unwrap()),
                    4,
                )
                .unwrap(),
            ));
        }
    }

    #[inline]
    fn dump_snapshot(_: &str) {}

    unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
        let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

        if n_instruction == 0 {
            return;
        }

        // bind the memory callback.
        for i in 0..n_instruction {
            let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
            let host_va_instruction = qemu_api::qemu_plugin_insn_haddr(inst);
            qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                inst,
                Some(vcpu_insn_exec),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                host_va_instruction,
            );
            // qemu_api::qemu_plugin_register_vcpu_mem_cb(
            //     inst,
            //     Some(vcpu_mem_access),
            //     qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
            //     qemu_api::qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
            //     std::ptr::null_mut(),
            // );
        }
    }
}
