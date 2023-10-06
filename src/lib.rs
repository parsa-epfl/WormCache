pub mod bp;
pub mod cache;
pub mod checkpoint;
pub mod mh;
pub mod vtime;

mod qemu_api;
use mh::ts_model::TimestampMemoryHierarchy;
use plugin::{QEMUMemoryInfo, QEMUPluginPerCoreActor};
use qemu_api::*;
mod plugin;
use once_cell::sync::Lazy;
use plugin::QEMUPlugin;
use std::ffi;
use std::io::Write;
use std::sync::Mutex;
use vtime::VirtualTimeContext;

/// TODO: Store the following variable inside the PluginType.
/// TODO: Make this data strcture as a constant incicating its length.
pub const CORE_COUNT: usize = 8;

// Parameter for the memory hierarchy.
pub type PluginType = TimestampMemoryHierarchy<8, 16, 16, 32>;
static PLUGIN: Lazy<PluginType> = Lazy::new(|| PluginType::new());
static TIME_PLUGIN: Lazy<Mutex<vtime::VirtualTimeContext>> = Lazy::new(|| Mutex::new(VirtualTimeContext::new()));


// convenient time function.

pub fn get_real_time() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

#[no_mangle]
pub static qemu_plugin_version: u32 = QEMU_PLUGIN_VERSION;

#[no_mangle]
unsafe extern "C" fn vcpu_mem_access(
    cpu_idx: u32,
    info: qemu_plugin_meminfo_t,
    vaddr: u64,
    user_data: *mut ffi::c_void, // should be NULL.
) {
    let core_id = cpu_idx as u8;
    let mut x = PLUGIN.hierarchies(core_id);
    x.on_memory_access(cpu_idx, &QEMUMemoryInfo(info), vaddr, user_data);
}

#[no_mangle]
unsafe extern "C" fn vcpu_insn_exec(
    vcpu_index: u32,
    user_data: *mut ffi::c_void, // it is basically its physical address.
) {
    let core_id = vcpu_index as u8;
    let mut x = PLUGIN.hierarchies(core_id);
    x.on_instruction_execution(vcpu_index, user_data);
}

#[no_mangle]
unsafe extern "C" fn vcpu_tb_trans(
    id: qemu_api::qemu_plugin_id_t,
    tb: *mut qemu_api::qemu_plugin_tb,
) {
    let wrapped_tb = plugin::QEMUPluginBasicBlock(tb);
    let metadata = PLUGIN.on_translation(&wrapped_tb);

    wrapped_tb
        .into_iter()
        .zip(metadata.into_iter())
        .for_each(|i| {
            match i.1.memory_access {
                Some(userdata) => {
                    qemu_plugin_register_vcpu_mem_cb(
                        i.0 .0,
                        Some(vcpu_mem_access),
                        qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
                        userdata,
                    );
                }
                None => {}
            };
            match i.1.instruction_execution {
                Some(userdata) => {
                    qemu_plugin_register_vcpu_insn_exec_cb(
                        i.0 .0,
                        Some(vcpu_insn_exec),
                        qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        userdata,
                    );
                }
                None => {}
            }
        });
}

#[no_mangle]
unsafe extern "C" fn plugin_exit(id: qemu_api::qemu_plugin_id_t, p: *mut ffi::c_void) {
    PLUGIN.on_qemu_exit();
}

pub unsafe extern "C" fn calculate_virtual_time() -> i64 {
    return TIME_PLUGIN.lock().unwrap().calculate_virtual_time(&PLUGIN);
}

#[no_mangle]
unsafe extern "C" fn qemu_plugin_install(
    id: qemu_api::qemu_plugin_id_t,
    _: *const qemu_api::qemu_info_t,
    _: i32,
    _: *const *const u8,
) -> i32 {
    qemu_plugin_register_vcpu_tb_trans_cb(id, Some(vcpu_tb_trans));
    qemu_plugin_register_atexit_cb(id, Some(plugin_exit), std::ptr::null_mut());

    qemu_plugin_register_virtual_time_cb(Some(calculate_virtual_time));

    // set up a thread to periodically print the icount of each core.
    std::thread::spawn(||{
        // open a csv file to store the icounts.
        let mut file = std::fs::File::create("icount.csv").unwrap();
        // write the header.
        file.write_fmt(format_args!("ts")).unwrap();
        for i in 0..CORE_COUNT {
            file.write_fmt(format_args!(",core{}", i)).unwrap();
        }
        file.write_fmt(format_args!("\n")).unwrap();

        loop {
            let icounts = PLUGIN.get_icounts();
            let mut lines = vec![];
            lines.push(format!("{}", get_real_time()));
            for i in 0..CORE_COUNT {
                lines.push(format!("{}", icounts[i]));
            }
            file.write_fmt(format_args!("{}\n", lines.join(","))).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });

    return 0;
}

