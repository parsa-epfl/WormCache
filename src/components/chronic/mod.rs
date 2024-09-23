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

use crate::qemu_api::qemu_plugin_is_icount_mode;
use rustc_hash::FxHashMap;

mod dump_statistics;
mod snapshot;
mod statistics;

pub fn chronic_behavior_init(options: &FxHashMap<String, String>) {
    let normal = "normal".to_string();
    let mode = options.get("mode").unwrap_or(&normal);

    // - mode=normal|warm|measure
    // - init_threshold=N
    // - interval=N
    // - count=N
    // - prefix="name"

    if mode == "warm" {
        println!("Periodical snapshot (warm) is enabled.");

        assert!(unsafe { qemu_plugin_is_icount_mode() });

        let init_threshold = options
            .get("init_threshold")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let interval = options
            .get("interval")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let count = options
            .get("count")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let prefix = options
            .get("prefix")
            .unwrap_or(&"snapshot".to_string())
            .clone();

        unsafe {
            snapshot::init(init_threshold, interval, count, prefix);
        }
    } else if mode == "measure" {
        assert!(unsafe { qemu_plugin_is_icount_mode() });

        let init_threshold = options
            .get("init_threshold")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let interval = options
            .get("interval")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let count = options
            .get("count")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let prefix = options
            .get("prefix")
            .unwrap_or(&"icount_statistics".to_string())
            .clone();

        println!("Measurement is ON.");
        println!(
            "Interval: {}, Initial threshold: {}, Count: {}",
            interval, init_threshold, count
        );

        unsafe {
            dump_statistics::init(init_threshold, interval, count, prefix);
        }
    }

    // Add more chronic behaviors here.
    statistics::init();
}
