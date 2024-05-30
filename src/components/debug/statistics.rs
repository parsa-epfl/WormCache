use strum::{EnumCount, IntoEnumIterator};
use strum_macros::{Display, EnumCount, EnumIter};

use std::cell::UnsafeCell;

use once_cell::sync::Lazy;

use crate::parameter::{self, CORE_COUNT, ENABLE_STATISTICS};

const ALLOCATED_CORE_COUNT: usize = if parameter::USE_UNIFIED_CACHE {
    CORE_COUNT
} else {
    CORE_COUNT * 2
};

#[derive(EnumCount, EnumIter, Display, Debug, Clone, Copy)]
pub enum EventType {
    MemoryAccess = 0,
    InstructionAccess = 1,
    DataAccess = 2,

    PrivateICacheMiss = 3,
    PrivateDCacheMiss = 4,
    PrivateCacheMiss = 5,
    PrivateCacheMissDueToPTW = 6,

    SharedCacheAccess = 7,
    SharedCacheMiss = 8,
    SharedCacheMissDueToPTW = 9,

    TLBMiss = 10,
    ITLBMiss = 11,
    DTLBMiss = 12,

    BranchCount = 13,
    BTBMiss = 14,
    RASMiss = 15,
    TageMiss = 16,
}

#[repr(align(64))]
struct PerCoreStatistics {
    counters: [u64; EventType::COUNT as usize],
}

impl PerCoreStatistics {
    pub fn new() -> Self {
        Self {
            counters: [0; EventType::COUNT as usize],
        }
    }

    #[inline]
    pub fn record(&mut self, event: EventType) {
        if ENABLE_STATISTICS {
            self.counters[event as usize] += 1;
        }
    }

    #[inline]
    pub fn get_line(&self, ts: u64, core_id: u32) -> String {
        let mut line = format!("{},{}", ts, core_id);
        for event in 0..EventType::COUNT as usize {
            line.push_str(&format!(",{}", self.counters[event]));
        }
        line
    }
}

pub struct Statistics {
    per_core: [UnsafeCell<PerCoreStatistics>; ALLOCATED_CORE_COUNT],
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
    pub fn record(&self, core_id: u32, event: EventType) {
        if ENABLE_STATISTICS {
            unsafe {
                (*self.per_core[core_id as usize].get()).record(event);
            }
        }
    }

    pub fn get_header() -> String {
        // generate all event names.
        let headers = EventType::iter()
            .map(|event| event.to_string())
            .collect::<Vec<String>>()
            .join(",");

        return format!("ts,core_id,{}", headers);
    }

    pub fn get_line_for_all_cores(&self, ts: u64) -> Vec<String> {
        let mut lines = Vec::new();
        for core_id in 0..ALLOCATED_CORE_COUNT as u32 {
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
    pub fn global_record(core_id: u32, event: EventType) {
        unsafe {
            GLOBAL_STATISTICS.record(core_id, event);
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
