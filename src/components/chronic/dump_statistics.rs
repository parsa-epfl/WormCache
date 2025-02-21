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

use std::io::Write;

use crate::{
    components::debug::statistics::{EventType, Statistics},
    parameter as param, qemu_api,
    util::get_monotonic_ts,
};

use serde_json::json;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static mut MEASURE_MAX_TURN: u64 = 0xffff_ffff_ffff_ffff;
static mut MEASURE_INTERVAL: u64 = 0;

static MEASURE_TURN: AtomicU64 = AtomicU64::new(0);
static mut CURRENT_CYCLE_COUNT: u64 = 0;
static mut MEASURE_NEXT_THRESHOLD: u64 = 0;
static MEASURE_PREFIX: OnceLock<String> = OnceLock::new();

unsafe extern "C" fn on_icount_periodic_checking(diff: u64) -> bool {
    CURRENT_CYCLE_COUNT += diff;
    // read user icount.
    if CURRENT_CYCLE_COUNT >= MEASURE_NEXT_THRESHOLD {
        const MEASURED_CORE_COUNT: usize = if param::MEASURE_HALF_OF_CORES {
            param::CORE_COUNT / 2
        } else {
            param::CORE_COUNT
        };

        let mut statistics_u = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut statistics_k = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut l2_miss_u = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut l2_miss_k = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut coherence_miss_u = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut coherence_miss_k = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut coherence_inv_u = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut coherence_inv_k = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut llc_miss_u = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut llc_miss_k = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut bp_miss_u = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut bp_miss_k = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut tlb_miss_u = Vec::with_capacity(MEASURED_CORE_COUNT);
        let mut tlb_miss_k = Vec::with_capacity(MEASURED_CORE_COUNT);

        for core_id in 0..MEASURED_CORE_COUNT {
            // Query the instruction.
            let (_, i_u, i_k) =
                Statistics::global_query_record(core_id as u32, EventType::Instruction);
            statistics_u.push(i_u);
            statistics_k.push(i_k);

            // Query the L2 miss.
            let (_, l2_u, l2_k) =
                Statistics::global_query_record(core_id as u32, EventType::PrivateCacheMiss);

            l2_miss_u.push(l2_u);
            l2_miss_k.push(l2_k);

            // Query the coherence miss.
            for t in [
                EventType::PrivateCacheMissTriggerCoherenceDueToFetch,
                EventType::PrivateCacheMissTriggerCoherenceDueToWrite,
                EventType::PrivateCacheMissTriggerCoherenceDueToRead,
            ] {
                let (_, coherence_u, coherence_k) =
                    Statistics::global_query_record(core_id as u32, t);

                coherence_miss_u.push(coherence_u);
                coherence_miss_k.push(coherence_k);
            }

            // Query the coherence invalidation.
            let (_, inv_u, inv_k) = Statistics::global_query_record(
                core_id as u32,
                EventType::PrivateCacheMissTriggerInvalidation,
            );

            coherence_inv_u.push(inv_u);
            coherence_inv_k.push(inv_k);

            // Query the LLC miss.
            let (_, llc_u, llc_k) =
                Statistics::global_query_record(core_id as u32, EventType::SharedCacheMiss);

            llc_miss_u.push(llc_u);
            llc_miss_k.push(llc_k);

            // Query the branch predictor miss.
            let (_, bp_u, bp_k) =
                Statistics::global_query_record(core_id as u32, EventType::BPMiss);

            bp_miss_u.push(bp_u);
            bp_miss_k.push(bp_k);

            // Query the TLB miss.
            let (_, tlb_u, tlb_k) =
                Statistics::global_query_record(core_id as u32, EventType::TLBMiss);

            tlb_miss_u.push(tlb_u);
            tlb_miss_k.push(tlb_k);
        }

        // You should stop the simulation.
        // report statistics.
        let result_json = json!({
            // "icount": i,
            "icount:u": statistics_u,
            "icount:k": statistics_k,

            "l2_miss:u": l2_miss_u,
            "l2_miss:k": l2_miss_k,

            "coherence_miss:u": coherence_miss_u,
            "coherence_miss:k": coherence_miss_k,

            "coherence_inv:u": coherence_inv_u,
            "coherence_inv:k": coherence_inv_k,

            "llc_miss:u": llc_miss_u,
            "llc_miss:k": llc_miss_k,

            "bp:u": bp_miss_u,
            "bp:k": bp_miss_k,

            "tlb:u": tlb_miss_u,
            "tlb:k": tlb_miss_k,
        });

        let mut turn = MEASURE_TURN.fetch_add(1, Ordering::Relaxed);

        // write the result_json to a file.
        let file =
            std::fs::File::create(format!("{}_{}.json", MEASURE_PREFIX.get().unwrap(), turn))
                .unwrap();
        serde_json::to_writer(&file, &result_json).unwrap();

        turn += 1;

        if turn >= MEASURE_MAX_TURN {
            println!("The maximum statistics turn is reached. Quit.");

            let mut miss_file = std::fs::File::create("statistics.final.csv").unwrap();
            miss_file
                .write_fmt(format_args!("{}\n", Statistics::get_header()))
                .unwrap();

            // update the local target time before writing the statistics
            for core_id in 0..param::CORE_COUNT {
                Statistics::global_set(
                    core_id as u32,
                    EventType::TargetLocalCycle,
                    false,
                    unsafe { qemu_api::qemu_plugin_get_vcpu_vtime(core_id as u32) },
                );
            }

            for stat in Statistics::global_get_line_for_all_cores(get_monotonic_ts()) {
                miss_file.write_all(stat.as_bytes()).unwrap();
                miss_file.write_all(b"\n").unwrap();
            }
            std::process::exit(0);
        }

        MEASURE_NEXT_THRESHOLD += MEASURE_INTERVAL;
    }

    false
}

pub unsafe fn init(init_threshold: u64, interval: u64, count: u64, prefix: String) {
    MEASURE_NEXT_THRESHOLD = init_threshold;
    MEASURE_MAX_TURN = count;
    MEASURE_INTERVAL = interval;
    MEASURE_PREFIX.set(prefix).unwrap();

    assert!(qemu_api::qemu_plugin_register_periodic_check_cb(Some(
        on_icount_periodic_checking
    )));
}
