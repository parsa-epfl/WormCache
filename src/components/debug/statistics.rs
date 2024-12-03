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

use std::cell::UnsafeCell;
use once_cell::sync::Lazy;

use crate::parameter::{CORE_COUNT, ENABLE_STATISTICS};

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

    SharedCacheAccessTsViolation,

    UnknownPrivateCacheMisses,
    UnknownSharedCacheMisses,

    // PrivateCacheColdMiss
    SharedCacheColdMiss, // The cache miss is caused due to the cold start of the shared cache.

    ShadowSharedCacheHit,
    ShadowSharedCacheMiss, // this is for debugging purpose of the SharedCacheVTsOrderViolation.

    SpecialMemoryInstructionAccess,
    SpecialMemoryInstructionPrivateCacheMiss,
    SpecialMemoryInstructionSharedCacheMiss,

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

static mut GLOBAL_STATISTICS: Lazy<Statistics> = Lazy::new(Statistics::new);

impl Statistics {
    #[inline]
    pub fn global_record(core_id: u32, event: EventType, is_os: bool) {
        unsafe {
            GLOBAL_STATISTICS.record(core_id, event, is_os);
        }
    }

    #[inline]
    pub fn global_record_by(core_id: u32, event: EventType, is_os: bool, increment: u64) {
        unsafe {
            GLOBAL_STATISTICS.record_by(core_id, event, is_os, increment);
        }
    }

    #[inline]
    pub fn global_set(core_id: u32, event: EventType, is_os: bool, value: u64) {
        unsafe {
            GLOBAL_STATISTICS.set(core_id, event, is_os, value);
        }
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
        unsafe { GLOBAL_STATISTICS.get_line_for_all_cores(ts) }
    }

    pub fn global_one_line_statistics() -> String {
        let mut lines = vec![];
        lines.push(Self::get_header());

        for stat in Self::global_get_line_for_all_cores(0) {
            lines.push(stat);
        }

        lines.join("\n")
    }
}
