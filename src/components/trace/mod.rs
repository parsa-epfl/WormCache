use std::{
    collections::HashMap,
    ffi,
    fs::File,
    io::{BufWriter, Write},
    sync::Mutex,
};

use once_cell::sync::Lazy;

use crate::qemu_api;

fn get_memory_ts() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

static TRACE_FILE: Lazy<Mutex<BufWriter<File>>> = Lazy::new(|| {
    // open a file to store the trace.
    let mut file = std::fs::File::create("trace.trace").unwrap();
    let mut b = BufWriter::with_capacity(64 * 1024 * 1024, file);
    return Mutex::new(b);
});

#[cfg(target_pointer_width = "64")]
#[derive(Debug)]
pub struct PluginFetchBlockContext {
    /// The virtual address of the first instruction.
    pub va: usize,
    /// The physical address of the first instruction.
    pub pa: usize,
    /// The number of instructions in this fetch block.
    pub size: usize,
}

static FETCH_BLOCK_CONTEXT_MAP: Lazy<Mutex<HashMap<usize, Box<PluginFetchBlockContext>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

unsafe extern "C" fn vcpu_insn_exec(
    vcpu_idx: u32,
    size: *mut ffi::c_void,
) {
    let context = &*(size as *mut PluginFetchBlockContext);

    let mut buffer = [0u8; 18];
    buffer[0..8].copy_from_slice(&context.pa.to_le_bytes());
    buffer[8..16].copy_from_slice(&(get_memory_ts() as u64).to_le_bytes());
    buffer[16] = 0;
    buffer[17] = vcpu_idx as u8;
    TRACE_FILE.lock().unwrap().write(&buffer).unwrap();
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

        let mut buffer = [0u8; 18];
        buffer[0..8].copy_from_slice(&paddr.to_le_bytes());
        buffer[8..16].copy_from_slice(&(get_memory_ts() as u64).to_le_bytes());
        buffer[16] = if is_store { 2 } else { 1 };
        buffer[17] = cpu_idx as u8;
        TRACE_FILE.lock().unwrap().write(&buffer).unwrap();
    } else {
        // TODO: check the I/O event
    }
}

// This structure is just a wrapper of for the plugin system to register. Plugin is believed to be globally singleton.
pub struct TracePlugin {}

impl super::Plugin for TracePlugin {
    #[inline]
    fn init() {
        // make sure the file is initialized.
        TRACE_FILE.lock().unwrap().flush().unwrap();
        println!("Trace plugin initialized.");
    }

    #[inline]
    fn dump_snapshot() {
        TRACE_FILE.lock().unwrap().flush().unwrap();
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
        for (idx, size) in fb_info.into_iter() {
            let i = qemu_api::qemu_plugin_tb_get_insn(tb, idx);
            // register the metadata first.
            let mut map = FETCH_BLOCK_CONTEXT_MAP.lock().unwrap();
            let ctx_ptr: *const PluginFetchBlockContext = match map.get_mut(&idx) {
                Some(old_ctx) => {
                    old_ctx.va = qemu_api::qemu_plugin_insn_vaddr(i) as usize;
                    old_ctx.pa = qemu_api::qemu_plugin_insn_haddr(i) as usize;
                    old_ctx.size = size;
                    let res: *const PluginFetchBlockContext = &**old_ctx;
                    res
                }
                None => {
                    let ctx = Box::new(PluginFetchBlockContext {
                        va: qemu_api::qemu_plugin_insn_vaddr(i) as usize,
                        pa: qemu_api::qemu_plugin_insn_haddr(i) as usize,
                        size,
                    });
                    let res: *const PluginFetchBlockContext = &*ctx;
                    map.insert(idx, ctx);
                    res
                }
            };
            qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                i,
                Some(vcpu_insn_exec),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                ctx_ptr as *mut ffi::c_void,
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
