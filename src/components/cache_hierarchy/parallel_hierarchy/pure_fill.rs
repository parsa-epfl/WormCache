use std::sync::OnceLock;

use crate::{parameter, qemu_api};
use spin::mutex::SpinMutex;

static SNAPSHOT_INFO: SpinMutex<Option<(String, u64)>> = SpinMutex::new(None);
static mut RAW_CKPT_FMT: bool = false;

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

        let format = if RAW_CKPT_FMT {
            qemu_api::qemu_plugin_snapshot_format_t_QEMU_PLUGIN_SNAPSHOT_FORMAT_EXTERNAL_INCREMENTAL_BASE_NO_BXDB
        } else {
            qemu_api::qemu_plugin_snapshot_format_t_QEMU_PLUGIN_SNAPSHOT_FORMAT_EXTERNAL_INCREMENTAL_BASE
        };

        qemu_api::qemu_plugin_savevm(c_snapshot_name.as_ptr(), format);

        std::process::exit(0);
    }
}

static SNAPSHOT_NAME: OnceLock<String> = OnceLock::new();
static WARM_RATIO: OnceLock<f64> = OnceLock::new();

unsafe extern "C" fn quantum_checking_callback(_: u64) -> bool {
    let warmed_set = unsafe { (*super::PLUGIN).get_scache_warmed_set_count() };
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

pub unsafe fn init(name: &str, warm_ratio: f64, raw_ckpt_fmt: bool) {
    unsafe {
        RAW_CKPT_FMT = raw_ckpt_fmt;

        assert!(qemu_api::qemu_plugin_register_event_loop_poll_cb(Some(
            event_loop_callback
        )));

        assert!(qemu_api::qemu_plugin_register_periodic_check_cb(Some(
            quantum_checking_callback
        )));
    }

    SNAPSHOT_NAME.set(format!("{}_{}", name, "warmed")).unwrap();

    assert!(warm_ratio >= 0.0 && warm_ratio <= 1.0);
    WARM_RATIO.set(warm_ratio).unwrap();
}
