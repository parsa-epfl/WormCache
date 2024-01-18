mod checkpoint;
mod mtr;
mod per_core_record;
mod ts_cache;
mod ts_model;
mod ts_set;
mod mmu;

use std::ffi;
use std::fs;
use std::io::Write;
use std::process::exit;

pub use checkpoint::CacheBlockState;
pub use checkpoint::PrivateCacheParameters;
pub use per_core_record::TimestampSingleCoreMemoryHierarchy;
pub use ts_cache::TimestampCache;
pub use ts_cache::TimestampCacheMetaData;
pub use ts_model::TimestampMemoryHierarchy;
pub use ts_set::CacheFlushResult;
pub use ts_set::CacheReturnResult;
pub use ts_set::TimestampCacheLineStatus;
pub use ts_set::TimestampCacheSet;

use crate::qemu_api;
use crate::parameter as param;

use once_cell::sync::Lazy;
use std::cell::UnsafeCell;

static mut PLUGIN: Lazy<
    UnsafeCell<
        TimestampMemoryHierarchy<
            { param::TLB_ASSO },
            { param::TLB_SET },
            { param::PRI_CACHE_ASSO },
            { param::PRI_CACHE_SET },
            { param::SHARED_CACHE_ASSO },
            { param::SHARED_CACHE_SET },
        >,
    >,
> = Lazy::new(|| UnsafeCell::new(TimestampMemoryHierarchy::new(param::CORE_COUNT)));

pub fn get_memory_ts() -> u128 {
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

        PLUGIN.get_mut().hierarchies(cpu_idx as u8).access_memory(
            get_memory_ts() as usize,
            vaddr,
            false,
            is_store,
        )
    } else {
        // TODO: check the I/O event
    }
}

unsafe extern "C" fn vcpu_insn_exec(
    vcpu_idx: u32,
    voffset: *mut ffi::c_void, // it is basically its physical address.
) {
    let vpn = unsafe {
        qemu_api::qemu_plugin_read_pc_vpn()
    };
    let vaddr = vpn << 12 | (voffset as u64 & 0xfff);
    PLUGIN.get_mut().hierarchies(vcpu_idx as u8).access_memory(
        get_memory_ts() as usize,
        vaddr,
        true,
        false,
    )
}

// TODO: One additional PluginAPI is needed for this instruction. It will be a similar function to the memory access.
unsafe extern "C" fn vcpu_invalidate_cache(
    vcpu_idx: u32,
    paddr: *mut ffi::c_void, // it is basically its physical address.
) {
    // PLUGIN
    //     .hierarchies(vcpu_idx as u8)
    //     .invalidate(paddr as usize, get_memory_ts() as usize);
}

pub struct MemoryPlugin {}

impl super::Plugin for MemoryPlugin {
    #[inline]
    fn init() {
        println!("Memory plugin initialized.");

        // I need to start a function to reason about the completion rate of LLC.
        "The following code is for querying LLC warming time.";
        // std::thread::spawn(|| {
        //     const LLC_SET: usize = param::SHARED_CACHE_SET;
        //     // Currently this stuff only works for a fully associative cache.
        //     let mut new_block_count = Vec::from_iter((0..LLC_SET).map(|_| false));
        //     let mut warmed_count = 0;
        //     let mut recorded_count = 0;
        //     // open a file to record completion time.
        //     let mut output = fs::File::create("./completion_time.csv").unwrap();
        //     output.write_fmt(format_args!("timestamp\n")).unwrap();
        //     // open a file to write the update time.
        //     loop {
        //         for set_index in 0..LLC_SET {
        //             if new_block_count[set_index] {
        //                 continue;
        //             }
        //             let mut touched_entry = 0;
        //             for core_id in 0..(param::CORE_COUNT as u8) {
        //                 unsafe {
        //                     let set = PLUGIN
        //                         .get_mut()
        //                         .hierarchies(core_id)
        //                         .local_shared_cache
        //                         .sets
        //                         .get(set_index)
        //                         .unwrap();
        //                     touched_entry += set.warm_chunk_count();
        //                 }
        //             }

        //             if touched_entry >= crate::parameter::SHARED_CACHE_ASSO {
        //                 new_block_count[set_index] = true;
        //                 warmed_count += 1;
        //                 if warmed_count == LLC_SET {
        //                     output
        //                         .write_fmt(format_args!("{}\n", get_memory_ts()))
        //                         .unwrap();
        //                     output.flush().unwrap();
        //                     recorded_count += 1;
        //                     if recorded_count == 40 {
        //                         output.flush().unwrap();
        //                         exit(0);
        //                     }
        //                     for core_id in 0..(param::CORE_COUNT as u8) {
        //                         unsafe {
        //                             PLUGIN
        //                                 .get_mut()
        //                                 .hierarchies(core_id)
        //                                 .clean_local_shared_cache();
        //                         }
        //                     }
        //                     warmed_count = 0;
        //                     for i in 0..LLC_SET {
        //                         new_block_count[i] = false;
        //                     }
        //                 }
        //             }
        //         }

        //         std::thread::sleep(std::time::Duration::from_secs(1));
        //     }
        // });
    }

    #[inline]
    fn dump_snapshot() {
        let private_param = checkpoint::PrivateCacheParameters {
            l1i_sets: 32,
            l1i_associativity: 16,
            l1d_sets: 32,
            l1d_associativity: 16,
            l2_sets: param::PRI_CACHE_SET,
            l2_associativity: param::PRI_CACHE_ASSO,
            directory_associativity: param::PRI_CACHE_ASSO * param::CORE_COUNT,
        };
        unsafe {
            let mtr = PLUGIN.get_mut().render_mtr::<{ param::PRI_CACHE_SET }>();
            let mtr = mtr.prune_by_associativity(private_param.directory_associativity);
            let caches = PLUGIN.get_mut().render_cache_hierarchy(&mtr, &private_param);
            let exported_json = serde_json::to_string_pretty(&caches).unwrap();
            let mut output = fs::File::create("./dumped.json").unwrap();
            output.write_all(exported_json.as_bytes()).unwrap();
            output.flush().unwrap();
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
