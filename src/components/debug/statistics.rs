use strum::{EnumCount, IntoEnumIterator};
use strum_macros::{Display, EnumCount, EnumIter};

use std::cell::UnsafeCell;

use once_cell::sync::Lazy;

use crate::parameter::{CORE_COUNT, ENABLE_STATISTICS};

#[derive(EnumCount, EnumIter, Display, Debug, Clone, Copy)]
pub enum EventType {
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

    SharedCacheAccess,
    SharedCacheMiss,
    SharedCacheMissDueToPTW,
    SharedCacheMissDueToInstructionFetch,
    SharedCacheMissDueToData,

    UnknownPrivateCacheMisses,
    UnknownSharedCacheMisses,

    PrivateCacheVTsOrderViolation, // The violation of the order suggested by VTs, for the coherence state information.
    SharedCacheVTsOrderViolation, // The violation of the order suggested by VTs, for the LRU information in the shared cache.

    // the key problem is still how I convert the previous two counters' value into the miss rate impact.
    TLBMiss,
    TLBMissDueToInstruction,
    TLBMissDueToData,

    BranchCount,
    BTBMiss,
    RASMiss,
    TageMiss,
}

#[repr(align(64))]
struct PerCoreStatistics {
    counters: [u64; EventType::COUNT * 3],
}

impl PerCoreStatistics {
    pub fn new() -> Self {
        Self {
            counters: [0; EventType::COUNT * 3],
        }
    }

    #[inline]
    pub fn record(&mut self, event: EventType, is_os: bool) {
        if ENABLE_STATISTICS {
            let index = (event as usize) * 3;
            self.counters[index as usize] += 1;

            if is_os {
                self.counters[index + 2] += 1;
            } else {
                self.counters[index + 1] += 1;
            }
        }
    }

    #[inline]
    pub fn get_line(&self, ts: u64, core_id: u32) -> String {
        let mut line = format!("{},{}", ts, core_id);
        for event in 0..EventType::COUNT {
            line.push_str(&format!(",{}", self.counters[3 * event]));
            line.push_str(&format!(",{}:u", self.counters[3 * event + 1]));
            line.push_str(&format!(",{}:k", self.counters[3 * event + 2]));
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

    pub fn global_query_record(core_id: u32, event: EventType) -> u64 {
        unsafe { (*GLOBAL_STATISTICS.per_core[core_id as usize].get()).counters[event as usize] }
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
