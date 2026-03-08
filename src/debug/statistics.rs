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

use strum::{EnumCount, IntoEnumIterator};
use strum_macros::{Display, EnumCount, EnumIter};

use std::sync::LazyLock;
use std::{cell::UnsafeCell, io::Write};
use std::{ffi, thread};

use crate::parameter::{CORE_COUNT, ENABLE_STATISTICS};
use crate::qemu_api::{self, qemu_plugin_exposed_statistics, qemu_plugin_get_exposed_statistics};
use crate::util::get_monotonic_ts;

/// Global array of pointers to QEMU exposed statistics (one per core)
/// These are populated during plugin initialization and allow direct
/// counter updates without function call overhead.
static mut QEMU_STAT_PTRS: [*mut qemu_plugin_exposed_statistics; CORE_COUNT] =
    [std::ptr::null_mut(); CORE_COUNT];

/// Initialize the QEMU statistics pointers for a specific core.
/// This should be called during vCPU initialization.
pub fn init_qemu_stat_ptr(core_id: u32) {
    if (core_id as usize) < CORE_COUNT {
        unsafe {
            QEMU_STAT_PTRS[core_id as usize] = qemu_plugin_get_exposed_statistics(core_id);
        }
    }
}

/// Inline function to record a statistic to QEMU's exposed statistics structure.
/// This has minimal overhead (~3-5 cycles) due to direct pointer access.
#[inline(always)]
pub fn record_qemu_stat(core_id: u32, event: EventType, increments: u64) {
    if let Some(offset) = event.to_qemu_offset() {
        unsafe {
            let stat_ptr = QEMU_STAT_PTRS[core_id as usize];
            if !stat_ptr.is_null() {
                let field_ptr = (stat_ptr as *mut u8).add(offset) as *mut u64;
                // *field_ptr += 1;
                let value = field_ptr.read_volatile();
                field_ptr.write_volatile(value + increments);
            }
        }
    }
}

#[derive(EnumCount, EnumIter, Display, Debug, Clone, Copy)]
pub enum EventType {
    Instruction,
    TargetLocalCycle,

    MemoryAccess,
    InstructionAccess,
    DataAccess,

    PrivateICacheMiss,
    PrivateDCacheMiss,
    PrivateCacheMiss,
    PrivateCacheMissDueToPTW,

    PrivateCacheMissTriggerCoherenceDueToFetch, // All misses that involve the coherence activity (GetS, GetX)
    PrivateCacheMissTriggerCoherenceDueToRead, // All misses that involve the coherence activity (GetS, GetX)
    PrivateCacheMissTriggerCoherenceDueToWrite, // All misses that involve the coherence activity (GetS, GetX)
    PrivateCacheMissTriggerInvalidation,        // All misses that invalid other copies (GetX)
    PrivateCacheInvalidation, // All invalidations that invalidate other copies (GetX)

    SharedCacheAccess,
    SharedCacheMiss,
    SharedCacheMissDueToPTW,
    SharedCacheMissDueToInstructionFetch,
    SharedCacheMissDueToDataRead,
    SharedCacheMissDueToDataWrite,

    SharedCacheColdMiss, // The cache miss is caused due to the cold start of the shared cache.

    PrivateCacheInvalidationCausailityViolation, // The cache line is invalidated by a access with a smaller timestamp.
    PrivateCacheDowngradeCausalityViolation, // The cache line is downgraded by a access with a smaller timestamp.
    SharedCacheAccessCausalityViolation, // The cache line (in LLC) is accessed by a access with a smaller timestamp.
    SharedCacheEvictionCausalityViolation, // The cache line (in LLC) is evicted by a access with a smaller timestamp.

    // the key problem is still how I convert the previous two counters' value into the miss rate impact.
    TLBMiss,
    TLBMissDueToInstruction,
    TLBMissDueToData,

    TLBAccess,
    TLBAccessDueToInstruction,
    TLBAccessDueToData,

    HugeTLBHit,
    HugeTLBHitDueToInstruction,
    HugeTLBHitDueToData,

    BranchCount,
    BTBMiss,
    RASMiss,
    TageMiss,
    BPMiss, // this is different from summing the previous one.
    // It includes the following logic to judge:
    // - For directional branch, it is a miss if the direction prediction is wrong, or
    // - For directional branch, it is a miss if the direction prediction is right but the target prediction is wrong.
    // - For indirect branch, it is a miss if the target prediction is wrong.
    // - For return, it is a miss if the target prediction (provided by the RAS) is wrong.
    WaitForInterrupt,
    PfL1,
    PfL2,
    PfMem,
    PfUnk,
    PfN,
    Pf0,
    Pf1,
    Pf2,
    Pf3,
    Pf4,
    Pf5,
    Pf6,
    Pf7,
    Pf8,
    Pf9,
    Pf10,
    Pf11,
    Pf12,
    Pf13,
    Pf14,
    Pf15,
    Pf16,
    Pf17,
    Pf18,
    Pf19,
    Pf20,
    Pf21,
    Pf22,
    Pf23,
    Pf24,
    Pf25,
    Pf26,
    Pf27,
    Pf28,
    Pf29,
    Pf30,
    Pf31,
    Prefetches,
    UselessPrefetches,
    UnknownPrefetches,
    WaitForEvent,
    CompareAndSwap,
}

impl EventType {
    /// Returns the byte offset into `qemu_plugin_exposed_statistics` for this event type.
    /// Returns `None` if the event does not map to a QEMU-exposed statistic.
    ///
    /// This uses a match statement which the compiler optimizes into a jump table,
    /// providing O(1) lookup without branching.
    #[inline(always)]
    pub fn to_qemu_offset(self) -> Option<usize> {
        match self {
            EventType::Instruction => Some(0),
            EventType::InstructionAccess => Some(8),
            EventType::DataAccess => Some(16),
            EventType::PrivateICacheMiss => Some(24),
            EventType::PrivateDCacheMiss => Some(32),
            EventType::SharedCacheMiss => Some(40),
            EventType::BranchCount => Some(48),
            EventType::BPMiss => Some(56),
            EventType::TLBMiss => Some(64),
            _ => None,
        }
    }
}

#[repr(align(64))]
struct PerCoreStatistics {
    counters: [u64; EventType::COUNT * 2],
}

impl PerCoreStatistics {
    pub fn new() -> Self {
        Self {
            counters: [0; EventType::COUNT * 2],
        }
    }

    #[inline]
    pub fn record(&mut self, event: EventType, is_os: bool) {
        self.record_by(event, is_os, 1);
    }

    #[inline]
    pub fn record_by(&mut self, event: EventType, is_os: bool, increment: u64) {
        if ENABLE_STATISTICS {
            let index = (event as usize) * 2;
            if is_os {
                self.counters[index + 1] += increment;
            } else {
                self.counters[index] += increment;
            }
        }
    }

    #[inline]
    pub fn decrease_by(&mut self, event: EventType, is_os: bool, decrement: u64) {
        if ENABLE_STATISTICS {
            let index = (event as usize) * 2;
            if is_os {
                self.counters[index + 1] = self.counters[index + 1].saturating_sub(decrement);
            } else {
                self.counters[index] = self.counters[index].saturating_sub(decrement);
            }
        }
    }

    #[inline]
    pub fn set(&mut self, event: EventType, is_os: bool, value: u64) {
        if ENABLE_STATISTICS {
            let index = (event as usize) * 2;
            if is_os {
                self.counters[index + 1] = value;
            } else {
                self.counters[index] = value;
            }
        }
    }

    #[inline]
    pub fn get_line(&self, ts: u64, core_id: u32) -> String {
        let mut line = format!("{},{}", ts, core_id);
        for event in 0..EventType::COUNT {
            let u = self.counters[2 * event];
            let k = self.counters[2 * event + 1];
            line.push_str(&format!(",{}", u + k));
            line.push_str(&format!(",{}", u));
            line.push_str(&format!(",{}", k));
        }
        line
    }
}

pub struct Statistics {
    per_core: [UnsafeCell<PerCoreStatistics>; CORE_COUNT],
}

unsafe impl Sync for Statistics {}

impl Default for Statistics {
    fn default() -> Self {
        Self::new()
    }
}

impl Statistics {
    pub fn new() -> Self {
        Self {
            per_core: std::array::from_fn(|_| UnsafeCell::new(PerCoreStatistics::new())),
        }
    }

    #[inline]
    pub fn record(&self, core_id: u32, event: EventType, is_os: bool) {
        if ENABLE_STATISTICS {
            unsafe {
                (*self.per_core[core_id as usize].get()).record(event, is_os);
            }
        }
    }

    #[inline]
    pub fn record_by(&self, core_id: u32, event: EventType, is_os: bool, increment: u64) {
        if ENABLE_STATISTICS {
            unsafe {
                (*self.per_core[core_id as usize].get()).record_by(event, is_os, increment);
            }
        }
    }

    #[inline]
    pub fn decrease_by(&self, core_id: u32, event: EventType, is_os: bool, decrement: u64) {
        if ENABLE_STATISTICS {
            unsafe {
                (*self.per_core[core_id as usize].get()).decrease_by(event, is_os, decrement);
            }
        }
    }

    #[inline]
    pub fn set(&self, core_id: u32, event: EventType, is_os: bool, value: u64) {
        if ENABLE_STATISTICS {
            unsafe {
                (*self.per_core[core_id as usize].get()).set(event, is_os, value);
            }
        }
    }

    pub fn get_header() -> String {
        // generate all event names.
        let headers = EventType::iter()
            .map(|event| {
                let event_name = event.to_string();
                format!("{},{}:u,{}:k", event_name, event_name, event_name)
            })
            .collect::<Vec<String>>()
            .join(",");

        format!("ts,core_id,{}", headers)
    }

    pub fn get_line_for_all_cores(&self, ts: u64) -> Vec<String> {
        let mut lines = Vec::new();
        for core_id in 0..CORE_COUNT as u32 {
            unsafe {
                lines.push((*self.per_core[core_id as usize].get()).get_line(ts, core_id));
            }
        }
        lines
    }
}

static GLOBAL_STATISTICS: LazyLock<Statistics> = LazyLock::new(Statistics::new);

impl Statistics {
    #[inline]
    pub fn global_record(core_id: u32, event: EventType, is_os: bool) {
        GLOBAL_STATISTICS.record(core_id, event, is_os);
        // Also record to QEMU's exposed statistics for performance modeling
        record_qemu_stat(core_id, event, 1);
    }

    #[inline]
    pub fn global_record_by(core_id: u32, event: EventType, is_os: bool, increment: u64) {
        GLOBAL_STATISTICS.record_by(core_id, event, is_os, increment);
        // For increment > 1, we record once to QEMU stats (it's an approximation)
        if increment > 0 {
            record_qemu_stat(core_id, event, increment);
        }
    }

    #[inline]
    pub fn global_decrease_by(core_id: u32, event: EventType, is_os: bool, decrement: u64) {
        GLOBAL_STATISTICS.decrease_by(core_id, event, is_os, decrement);
    }

    #[inline]
    pub fn global_set(core_id: u32, event: EventType, is_os: bool, value: u64) {
        GLOBAL_STATISTICS.set(core_id, event, is_os, value);
    }

    pub fn global_query_record(core_id: u32, event: EventType) -> (u64, u64, u64) {
        unsafe {
            let cnt = (*GLOBAL_STATISTICS.per_core[core_id as usize].get()).counters;
            let index = (event as usize) * 2;
            let u = cnt[index];
            let k = cnt[index + 1];
            (u + k, u, k)
        }
    }

    pub fn global_get_line_for_all_cores(ts: u64) -> Vec<String> {
        GLOBAL_STATISTICS.get_line_for_all_cores(ts)
    }

    pub fn global_one_line_statistics() -> String {
        let mut lines = vec![];
        lines.push(Self::get_header());

        for stat in Self::global_get_line_for_all_cores(0) {
            lines.push(stat);
        }

        lines.join("\n")
    }

    #[inline]
    pub fn save_to_csv(file_name: &str, ts: u64) {
        // save statistics.
        let mut file = std::fs::File::create(file_name).unwrap();
        // write header.
        file.write_fmt(format_args!("{}\n", Statistics::get_header()))
            .unwrap();
        // write content
        for stat in Statistics::global_get_line_for_all_cores(ts) {
            file.write_all(stat.as_bytes()).unwrap();
            file.write_all(b"\n").unwrap();
        }

        file.flush().unwrap();
        drop(file);
    }
}

pub fn create_thread_for_periodic_log() {
    thread::spawn(move || {
        let mut miss_file = std::fs::File::create("statistics.csv").unwrap();

        miss_file
            .write_fmt(format_args!("{}\n", Statistics::get_header()))
            .unwrap();

        loop {
            // update the local target time before writing the statistics
            for core_id in 0..CORE_COUNT {
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

            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });
}

pub unsafe extern "C" fn save_statistics_to_certain_file(file_path: *const ffi::c_char) {
    unsafe {
        let file_path = std::ffi::CStr::from_ptr(file_path).to_str();
        if file_path.is_err() {
            // report the error.
            println!("Failed to convert file path to str: {:?}", file_path);
            return;
        }
        let file_path = file_path.unwrap();
        Statistics::save_to_csv(file_path, 0);
    }
}

unsafe extern "C" fn user_vcpu_insn_exec(
    vcpu_idx: u32,
    size: *mut ffi::c_void, // the size of the basic block
) {
    Statistics::global_record_by(vcpu_idx, EventType::Instruction, false, size as u64);
}

unsafe extern "C" fn kernel_vcpu_insn_exec(
    vcpu_idx: u32,
    size: *mut ffi::c_void, // the size of the basic block
) {
    Statistics::global_record_by(vcpu_idx, EventType::Instruction, true, size as u64);
}

pub unsafe extern "C" fn on_translation_instructions(tb: *mut qemu_api::qemu_plugin_tb) {
    unsafe {
        let first_instruction = qemu_api::qemu_plugin_tb_get_insn(tb, 0);
        let size = qemu_api::qemu_plugin_tb_n_insns(tb);
        // I need to get the first instruction's PC to see if it is a user or kernel space.
        let pc = qemu_api::qemu_plugin_insn_vaddr(first_instruction);
        if pc & 0x8000_0000_0000_0000 == 0 {
            // user space
            qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                first_instruction,
                Some(user_vcpu_insn_exec),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                size as *mut ffi::c_void,
            );
        } else {
            qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                first_instruction,
                Some(kernel_vcpu_insn_exec),
                qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                size as *mut ffi::c_void,
            );
        }
    }
}
