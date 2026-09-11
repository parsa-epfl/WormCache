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
