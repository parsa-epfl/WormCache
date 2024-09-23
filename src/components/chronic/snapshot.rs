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

use crate::qemu_api;
use spin::Mutex as SpinMutex;

static SNAPSHOT_INFO: SpinMutex<Option<(String, u64)>> = SpinMutex::new(None);

static mut PERIODIC_SNAPSHOT_COUNT: u64 = 0;
static mut PERIODIC_SNAPSHOT_REQUIRED_COUNT: u64 = 0xffff_ffff_ffff_ffff;

static mut PERIODIC_SNAPSHOT_THRESHOLD: u64 = 0xffff_ffff_ffff_ffff;
static mut PERIODIC_SNAPSHOT_INTERVAL: u64 = 0xffff_ffff_ffff_ffff;
static mut PERIODIC_SNAPSHOT_CURRENT_CYCLES: u64 = 0;

static mut SNAPSHOT_PREFIX: String = String::new();

unsafe extern "C" fn event_loop_callback() {
    let snapshot_info_guard = SNAPSHOT_INFO.try_lock();
    if snapshot_info_guard.is_none() {
        return;
    }

    let mut snapshot_info_guard = snapshot_info_guard.unwrap();

    if snapshot_info_guard.is_none() {
        return;
    }

    let snapshot_info = snapshot_info_guard.take().unwrap();

    println!(
        "Snapshot request: {}, At Cycle: {}",
        &snapshot_info.0, snapshot_info.1
    );

    let c_snapshot_name = std::ffi::CString::new(snapshot_info.0.clone()).unwrap();

    qemu_api::qemu_plugin_savevm(c_snapshot_name.as_ptr());

    PERIODIC_SNAPSHOT_COUNT += 1;

    if PERIODIC_SNAPSHOT_COUNT >= PERIODIC_SNAPSHOT_REQUIRED_COUNT {
        println!("Generate {} snapshots. Quit.", PERIODIC_SNAPSHOT_COUNT);
        std::process::exit(0);
    }
}

// Remember, this function will be used as a quantum callback.
unsafe extern "C" fn quantum_checking_callback(diff: u64) {
    PERIODIC_SNAPSHOT_CURRENT_CYCLES += diff;

    if PERIODIC_SNAPSHOT_CURRENT_CYCLES >= PERIODIC_SNAPSHOT_THRESHOLD {
        let snapshot_name = format!("{}_{}", SNAPSHOT_PREFIX.clone(), PERIODIC_SNAPSHOT_COUNT);
        let snapshot_info = (snapshot_name, PERIODIC_SNAPSHOT_CURRENT_CYCLES);

        let snapshot_info_guard = SNAPSHOT_INFO.try_lock();
        if snapshot_info_guard.is_none() {
            return;
        }

        let mut snapshot_info_guard = snapshot_info_guard.unwrap();

        if snapshot_info_guard.is_none() {
            *snapshot_info_guard = Some(snapshot_info);
        }

        PERIODIC_SNAPSHOT_THRESHOLD += PERIODIC_SNAPSHOT_INTERVAL;
    }
}

pub unsafe fn init(init_threshold: u64, interval: u64, required_count: u64, prefix: String) {
    PERIODIC_SNAPSHOT_THRESHOLD = init_threshold;
    PERIODIC_SNAPSHOT_REQUIRED_COUNT = required_count;
    PERIODIC_SNAPSHOT_INTERVAL = interval;
    SNAPSHOT_PREFIX = prefix;

    unsafe {
        assert!(qemu_api::qemu_plugin_register_periodic_check_cb(Some(
            quantum_checking_callback
        )));

        assert!(qemu_api::qemu_plugin_register_event_loop_poll_cb(Some(
            event_loop_callback
        )));
    }
}
