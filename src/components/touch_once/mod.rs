use std::ffi;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

use crate::qemu_api;
use crate::CORE_COUNT;

use once_cell::sync::Lazy;

mod touched_cache;
use touched_cache::TouchedCache;

const CONFIGURATION: [usize; 8] = [
    8 * 1024,
    16 * 1024,
    32 * 1024,
    64 * 1024,
    128 * 1024,
    256 * 1024,
    512 * 1024,
    1024 * 1024,
];

static PLUGIN: Lazy<Mutex<Vec<(TouchedCache, File)>>> = Lazy::new(|| {
    Mutex::new(Vec::from_iter(
        CONFIGURATION.iter().map(|&set| {
            return (TouchedCache::new(set, 16), File::create(format!("./{}MB_touched.csv", set / 1024)).unwrap());
        }),
    ))
});

fn get_memory_ts() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
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

        PLUGIN.lock().unwrap().iter_mut().for_each(|(cache, file)| {
            if cache.access(paddr) {
                file.write_fmt(format_args!("{},{}\n", get_memory_ts(), cache.get_fully_touched_set_count())).unwrap();
            }
        });
    } else {
        // TODO: check the I/O event
    }
}

unsafe extern "C" fn vcpu_insn_exec(
    vcpu_idx: u32,
    paddr: *mut ffi::c_void, // it is basically its physical address.
) {
    PLUGIN.lock().unwrap().iter_mut().for_each(|(cache, file)| {
        file.write_fmt(format_args!("{},{}\n", get_memory_ts(), cache.get_fully_touched_set_count())).unwrap();
    });
}

pub struct TouchOnePlugin {}

impl super::Plugin for TouchOnePlugin {
    #[inline]
    fn init() {
        println!("Touch once plugin initialized.");
        unsafe {
            assert!(qemu_api::qemu_plugin_n_vcpus() == 1, "Currently this plugin only works for single vCPU.");
        }

        // all files should be initialized and write the first line.
        PLUGIN.lock().unwrap().iter_mut().for_each(|(_, file)| {
            file.write_fmt(format_args!("timestamp,fully_touched_set_count\n")).unwrap();
        });
    }

    #[inline]
    fn dump_snapshot() {
        
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
}
