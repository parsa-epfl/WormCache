use std::cell::UnsafeCell;

use once_cell::sync::Lazy;

use crate::parameter::{CORE_COUNT, ENABLE_STATISTICS};

#[derive(Debug, Clone, Copy)]
pub enum EventType {
    MemoryAccess = 0,
    InstructionAccess = 1,
    DataAccess = 2,

    PrivateICacheMiss = 3,
    PrivateDCacheMiss = 4,
    PrivateCacheMiss = 5,

    SharedCacheAccess = 6,
    SharedCacheMiss = 7,

    TLBMiss = 8,
    ITLBMiss = 9,
    DTLBMiss = 10,

    EventCount,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EventType::MemoryAccess => write!(f, "MemoryAccess"),
            EventType::InstructionAccess => write!(f, "InstructionAccess"),
            EventType::DataAccess => write!(f, "DataAccess"),
            EventType::PrivateICacheMiss => write!(f, "PrivateICacheMiss"),
            EventType::PrivateDCacheMiss => write!(f, "PrivateDCacheMiss"),
            EventType::PrivateCacheMiss => write!(f, "PrivateCacheMiss"),
            EventType::SharedCacheAccess => write!(f, "SharedCacheAccess"),
            EventType::SharedCacheMiss => write!(f, "SharedCacheMiss"),
            EventType::TLBMiss => write!(f, "TLBMiss"),
            EventType::ITLBMiss => write!(f, "ITLBMiss"),
            EventType::DTLBMiss => write!(f, "DTLBMiss"),
            EventType::EventCount => write!(f, "EventCount"),
        }
    }
}

#[repr(align(64))]
struct PerCoreStatistics {
    counters: [u64; EventType::EventCount as usize],
}

impl PerCoreStatistics {
    pub fn new() -> Self {
        return Self {
            counters: [0; EventType::EventCount as usize],
        };
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
        for event in 0..EventType::EventCount as usize {
            line.push_str(&format!(",{}", self.counters[event]));
        }
        return line;
    }
}

pub struct Statistics {
    per_core: [UnsafeCell<PerCoreStatistics>; CORE_COUNT],
}

impl Statistics {
    pub fn new() -> Self {
        return Self {
            per_core: std::array::from_fn(|_| UnsafeCell::new(PerCoreStatistics::new())),
        };
    }

    #[inline]
    pub fn record(&self, core_id: u32, event: EventType) {
        if ENABLE_STATISTICS {
            unsafe {
                (*self.per_core[core_id as usize].get()).record(event);
            }
        }
    }

    pub const fn get_header() -> &'static str {
        return "timestamp,core_id,MemoryAccess,InstructionAccess,DataAccess,PrivateICacheMiss,PrivateDCacheMiss,PrivateCacheMiss,SharedCacheAccess,SharedCacheMiss,TLBMiss,ITLBMiss,DTLBMiss";
    }

    pub fn get_line_for_all_cores(&self, ts: u64) -> Vec<String> {
        let mut lines = Vec::new();
        for core_id in 0..CORE_COUNT as u32 {
            unsafe {
                lines.push((*self.per_core[core_id as usize].get()).get_line(ts, core_id));
            }
        }
        return lines;
    }
}

static mut GLOBAL_STATISTICS: Lazy<Statistics> = Lazy::new(Statistics::new);

impl Statistics {
    pub fn global_record(core_id: u32, event: EventType) {
        unsafe {
            GLOBAL_STATISTICS.record(core_id, event);
        }
    }

    pub fn global_get_line_for_all_cores(ts: u64) -> Vec<String> {
        unsafe {
            return GLOBAL_STATISTICS.get_line_for_all_cores(ts);
        }
    }
}
