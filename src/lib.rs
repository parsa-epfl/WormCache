pub mod bp;
pub mod parameter;
pub use parameter::*;

mod components;
mod qemu_api;
mod util;

// Plugin
use components::memory;
use components::virtual_time;

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::ffi;
use std::sync::Mutex;

pub fn get_real_time() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

#[no_mangle]
pub static qemu_plugin_version: u32 = qemu_api::QEMU_PLUGIN_VERSION;

#[no_mangle]
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
        memory::on_data_cacheline_touched(cpu_idx, vaddr as usize, paddr, is_store);
        virtual_time::on_data_cacheline_touched(cpu_idx, vaddr as usize, paddr, is_store);
    } else {
        // TODO: check the I/O event
    }

}

#[no_mangle]
unsafe extern "C" fn vcpu_insn_exec(
    vcpu_idx: u32,
    user_data: *mut ffi::c_void, // it is basically its physical address.
) {
    let ctx = &*(user_data as *const PluginFetchBlockContext);
    memory::on_instruction_cacheline_touched(vcpu_idx, ctx);
    virtual_time::on_instruction_cacheline_touched(vcpu_idx, ctx);
}

#[cfg(target_pointer_width = "64")]
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

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    _: qemu_api::qemu_plugin_id_t,
    tb: *mut qemu_api::qemu_plugin_tb,
) {
    let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

    if n_instruction == 0 {
        return;
    }

    let mut block_id = vec![];
    for i in 0..n_instruction {
        let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
        block_id.push(qemu_api::qemu_plugin_insn_haddr(inst) as usize);
    }

    let fb_info = util::find_fetch_block_from_block_id_sequence(block_id);

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

#[no_mangle]
unsafe extern "C" fn plugin_exit(_: qemu_api::qemu_plugin_id_t, _: *mut ffi::c_void) {
    virtual_time::dump_snapshot();
    memory::dump_snapshot();
}

#[no_mangle]
unsafe extern "C" fn qemu_plugin_install(
    id: qemu_api::qemu_plugin_id_t,
    _: *const qemu_api::qemu_info_t,
    _: i32,
    _: *const *const u8,
) -> i32 {
    // make sure that the number of vCPUs is equal to the core count.
    assert_eq!(
        qemu_api::qemu_plugin_n_vcpus(),
        CORE_COUNT as i32,
        "Unmatched core count, thus exit."
    );

    qemu_api::qemu_plugin_register_vcpu_tb_trans_cb(id, Some(vcpu_tb_trans));

    memory::init();
    virtual_time::init();

    // // set up a thread to periodically print the icount of each core.
    // std::thread::spawn(|| {
    //     // open a csv file to store the icounts.
    //     let mut file = std::fs::File::create("cache-icount.csv").unwrap();
    //     // write the header.
    //     // file.write_fmt(format_args!("ts")).unwrap();
    //     // for i in 0..CORE_COUNT {
    //     //     file.write_fmt(format_args!(",core{}", i)).unwrap();
    //     // }
    //     // file.write_fmt(format_args!("\n")).unwrap();
    //     let mut head = vec!["ts".to_string()];
    //     for i in 0..CORE_COUNT {
    //         head.push(format!("core{}", i));
    //     }

    //     file.write_fmt(format_args!("{}\n", head.join(",")))
    //         .unwrap();

    //     loop {
    //         let icounts = PLUGIN.get_icounts();
    //         let mut lines = vec![];
    //         lines.push(format!("{}", get_real_time()));
    //         for i in 0..CORE_COUNT {
    //             lines.push(format!("{}", icounts[i]));
    //         }
    //         file.write_fmt(format_args!("{}\n", lines.join(",")))
    //             .unwrap();
    //         std::thread::sleep(std::time::Duration::from_secs(1));
    //     }
    // });

    return 0;
}
