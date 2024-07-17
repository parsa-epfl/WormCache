use std::ffi;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use crate::qemu_api;

use once_cell::sync::Lazy;

mod touched_cache;
use rustc_hash::FxHashMap;
use touched_cache::TouchedCache;

use crate::util::get_monotonic_ts;

const CONFIGURATION: [usize; 1] = [1024 * 1024];

static PLUGIN: Lazy<Mutex<Vec<(TouchedCache, File)>>> = Lazy::new(|| {
    Mutex::new(Vec::from_iter(CONFIGURATION.iter().map(|&set| {
        (
            TouchedCache::new(set, 16),
            File::create(format!("./{}MB_touched.csv", set / 1024)).unwrap(),
        )
    })))
});

static ICOUNT: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn vcpu_mem_access(
    _cpu_idx: u32,
    info: qemu_api::qemu_plugin_meminfo_t,
    vaddr: u64,
    _: *mut ffi::c_void, // should be NULL.
) {
    if _cpu_idx != 0 {
        return;
    }

    let hw_handler = qemu_api::qemu_plugin_get_hwaddr(info, vaddr);
    let is_device = qemu_api::qemu_plugin_hwaddr_is_io(hw_handler);

    if !is_device {
        let _is_store = qemu_api::qemu_plugin_mem_is_store(info);
        let paddr = qemu_api::qemu_plugin_hwaddr_phys_addr(hw_handler) as usize;

        PLUGIN.lock().unwrap().iter_mut().for_each(|(cache, file)| {
            if cache.access(paddr) && cache.is_fully_touched() {
                file.write_fmt(format_args!(
                    "{},{},{}\n",
                    get_monotonic_ts(),
                    ICOUNT.load(Ordering::Relaxed),
                    cache.get_fully_touched_set_count()
                ))
                .unwrap();
                cache.reset();
            }
        });
    } else {
        // TODO: check the I/O event
    }
}

unsafe extern "C" fn vcpu_insn_exec(
    _vcpu_idx: u32,
    paddr: *mut ffi::c_void, // it is basically its physical address.
) {
    if _vcpu_idx != 0 {
        return;
    }

    PLUGIN.lock().unwrap().iter_mut().for_each(|(cache, file)| {
        if cache.access(paddr as usize) && cache.is_fully_touched() {
            file.write_fmt(format_args!(
                "{},{},{}\n",
                get_monotonic_ts(),
                ICOUNT.load(Ordering::Relaxed),
                cache.get_fully_touched_set_count()
            ))
            .unwrap();
            cache.reset();
        }
    });
}

unsafe extern "C" fn icount_calcuclation(_vcpu_idx: u32, icount: *mut ffi::c_void) {
    if _vcpu_idx != 0 {
        return;
    }

    ICOUNT.fetch_add(icount as usize, Ordering::Relaxed);
}

pub struct TouchOnePlugin {}

impl super::Plugin for TouchOnePlugin {
    #[inline]
    fn init(_options: &FxHashMap<String, String>) {
        println!("Touch once plugin initialized.");

        // all files should be initialized and write the first line.
        PLUGIN.lock().unwrap().iter_mut().for_each(|(_, file)| {
            file.write_fmt(format_args!("timestamp,icount,fully_touched_set_count\n"))
                .unwrap();
        });
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
                qemu_api::qemu_plugin_insn_haddr(i),
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

        // bind the icount callback.
        qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
            qemu_api::qemu_plugin_tb_get_insn(tb, 0),
            Some(icount_calcuclation),
            qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
            n_instruction as *mut ffi::c_void,
        );
    }

    #[inline]
    fn dump_snapshot(_: &str) {}

    fn serialize(_: &str) {}

    fn deserialize(_: &str) {}
}
