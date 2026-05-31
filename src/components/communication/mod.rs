use crate::{
    arch::AArch64,
    components::cache_hierarchy::{
        common::L0InstructionCache,
        mmu::{self, tlb::AddressSpaceID, AbstractMMU, MMUFlushMode, MMUTranslationResult},
    },
    debug::{noc_traffic::NocTraffic, statistics::Statistics},
    parameter, qemu_api,
};
use bitvec::{array::BitArray, order::Lsb0, BitArr};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use spin::Mutex as SpinMutex;
use std::ffi;

pub mod aarch64_decoder;

const MODEL_SHARED_MEMORY: bool = false;

// Global timestamp updated by QEMU's quantum incremental handler
#[unsafe(no_mangle)]
static mut QUANTUM_GENERATION: u64 = 0;

// Threshold for automatic serialization and quit (in nanoseconds)
static mut QUIT_THRESHOLD_NS: u64 = u64::MAX;

// Define the MMU type based on parameter configuration
type CommunicationMMU = mmu::OrdinaryMMU<
    AArch64,
    { parameter::ITLB_ASSO },
    { parameter::ITLB_SET },
    { parameter::DTLB_ASSO },
    { parameter::DTLB_SET },
    { parameter::STLB_ENABLED },
    { parameter::STLB_ASSO },
    { parameter::STLB_SET },
    { parameter::NO_HUGE_PAGE },
>;

// Data structures for tracking intervals
#[derive(Serialize, Deserialize, Clone, Debug)]
struct CacheLineAccessRecord {
    last_write_timestamp: u64,
    last_writer_core: u32,
    recent_readers: BitArr!(for parameter::CORE_COUNT, in u64, Lsb0),
    // Per-core intervals: core_id -> (interval_ns -> count)
    per_core_intervals: FxHashMap<u32, FxHashMap<u64, u64>>,
    // per_core_access_pc: FxHashMap<u32, FxHashMap<u64, u64>>, // core_id -> (pc -> count)
    // Per-core counters
    per_core_instruction_fetch_count: FxHashMap<u32, u64>,
    per_core_page_walk_count: FxHashMap<u32, u64>,
    per_core_os_access_count: FxHashMap<u32, u64>,
    per_core_load_exclusive_count: FxHashMap<u32, u64>,
    per_core_atomic_access_count: FxHashMap<u32, u64>,
    // trace: Vec<(u32, u64, bool, bool, bool, bool, bool, bool, u64, u64, u64)>, // (core_id, cache_line_id, is_instruction, is_write, is_os, is_page_walk, is_atomic, is_load_exclusive, ts, pc, va)
}

impl CacheLineAccessRecord {
    fn new() -> Self {
        CacheLineAccessRecord {
            last_write_timestamp: 0,
            last_writer_core: 0,
            recent_readers: BitArray::ZERO,
            per_core_intervals: FxHashMap::default(),
            // per_core_access_pc: FxHashMap::default(),
            per_core_instruction_fetch_count: FxHashMap::default(),
            per_core_page_walk_count: FxHashMap::default(),
            per_core_os_access_count: FxHashMap::default(),
            per_core_load_exclusive_count: FxHashMap::default(),
            per_core_atomic_access_count: FxHashMap::default(),
            // trace: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SerializedCacheLineRecord {
    per_core_intervals: FxHashMap<u32, FxHashMap<u64, u64>>, // core_id -> (interval_ns -> count)
    per_core_instruction_fetch_count: FxHashMap<u32, u64>,
    per_core_page_walk_count: FxHashMap<u32, u64>,
    per_core_os_access_count: FxHashMap<u32, u64>,
    per_core_load_exclusive_count: FxHashMap<u32, u64>,
    per_core_atomic_access_count: FxHashMap<u32, u64>,
    // per_core_access_pc: FxHashMap<u32, FxHashMap<u64, u64>>, // core_id -> (pc -> count)
    // trace: Vec<(u32, u64, bool, bool, bool, bool, bool, bool, u64, u64, u64)>, // (core_id, cache_line_id, is_instruction, is_write, is_os, is_page_walk, is_atomic, is_load_exclusive, ts, pc, va)
}

// Sharded cache line records for fine-grained locking
const NUM_SHARDS: usize = 32768; // 32K shards
const SHARD_MASK: u64 = 0x7FFF; // Last 15 bits

struct ShardedCacheLineRecords {
    shards: Vec<SpinMutex<FxHashMap<u64, CacheLineAccessRecord>>>,
}

impl ShardedCacheLineRecords {
    fn new() -> Self {
        let mut shards = Vec::with_capacity(NUM_SHARDS);
        for _ in 0..NUM_SHARDS {
            shards.push(SpinMutex::new(FxHashMap::default()));
        }
        ShardedCacheLineRecords { shards }
    }

    #[inline]
    fn get_shard_index(cache_line_id: u64) -> usize {
        (cache_line_id & SHARD_MASK) as usize
    }

    fn record_access(
        &self,
        cache_line_id: u64,
        core_id: u32,
        is_instruction: bool,
        is_write: bool,
        is_os: bool,
        is_page_walk: bool,
        is_atomic: bool,
        is_load_exclusive: bool,
        ts: u64,
        _pc: u64,
        _va: u64,
    ) {
        let shard_idx = Self::get_shard_index(cache_line_id);
        let mut shard = self.shards[shard_idx].lock();

        if is_write {
            // This is a write access
            let record = shard
                .entry(cache_line_id)
                .or_insert_with(CacheLineAccessRecord::new);

            // Check if different core wrote to the same cache line
            if record.last_writer_core != core_id && record.last_write_timestamp > 0 {
                let interval = ts.saturating_sub(record.last_write_timestamp);

                // Record interval for this core (using HashMap entry API)
                *record
                    .per_core_intervals
                    .entry(core_id)
                    .or_insert_with(FxHashMap::default)
                    .entry(interval)
                    .or_insert(0) += 1;

                if is_page_walk {
                    *record.per_core_page_walk_count.entry(core_id).or_insert(0) += 1;
                }

                if is_os {
                    *record.per_core_os_access_count.entry(core_id).or_insert(0) += 1;
                }

                if is_atomic {
                    *record
                        .per_core_atomic_access_count
                        .entry(core_id)
                        .or_insert(0) += 1;
                }

                if is_load_exclusive {
                    *record
                        .per_core_load_exclusive_count
                        .entry(core_id)
                        .or_insert(0) += 1;
                }

                if is_instruction {
                    *record
                        .per_core_instruction_fetch_count
                        .entry(core_id)
                        .or_insert(0) += 1;
                }

                // Record PC access
                // *record.per_core_access_pc
                //     .entry(core_id)
                //     .or_insert_with(FxHashMap::default)
                //     .entry(pc)
                //     .or_insert(0) += 1;
            }

            record.last_write_timestamp = ts;
            record.last_writer_core = core_id;
            record.recent_readers.fill(false);
        } else {
            // This is a read access
            if let Some(record) = shard.get_mut(&cache_line_id) {
                if record.last_writer_core != core_id
                    && record.recent_readers[core_id as usize] == false
                {
                    // Different core reading after a write
                    let interval = ts.saturating_sub(record.last_write_timestamp);

                    // Record interval for this core (using HashMap entry API)
                    *record
                        .per_core_intervals
                        .entry(core_id)
                        .or_insert_with(FxHashMap::default)
                        .entry(interval)
                        .or_insert(0) += 1;

                    if is_page_walk {
                        *record.per_core_page_walk_count.entry(core_id).or_insert(0) += 1;
                    }

                    if is_os {
                        *record.per_core_os_access_count.entry(core_id).or_insert(0) += 1;
                    }

                    if is_atomic {
                        *record
                            .per_core_atomic_access_count
                            .entry(core_id)
                            .or_insert(0) += 1;
                    }

                    if is_load_exclusive {
                        *record
                            .per_core_load_exclusive_count
                            .entry(core_id)
                            .or_insert(0) += 1;
                    }

                    if is_instruction {
                        *record
                            .per_core_instruction_fetch_count
                            .entry(core_id)
                            .or_insert(0) += 1;
                    }

                    record.recent_readers.set(core_id as usize, true);

                    // Record PC access
                    // *record.per_core_access_pc
                    //     .entry(core_id)
                    //     .or_insert_with(FxHashMap::default)
                    //     .entry(pc)
                    //     .or_insert(0) += 1;
                }
            }
        }

        // if cache_line_id == 1062304200 {
        //     // trace.
        //     let record = shard.entry(cache_line_id).or_insert_with(CacheLineAccessRecord::new);
        //     record.trace.push((core_id, cache_line_id, is_instruction, is_write, is_os, is_page_walk, is_atomic, is_load_exclusive, ts, pc, va));
        // }
    }

    fn dump(&self, file_path: &str, total_memory_accesses: u64) {
        let mut cache_comm_data = FxHashMap::default();

        // Collect data from all shards
        for shard in &self.shards {
            let mut shard_guard = shard.lock();
            for (cache_line_id, record) in shard_guard.iter_mut() {
                // Check if any core has recorded intervals for this cache line
                let has_data = record
                    .per_core_intervals
                    .values()
                    .any(|intervals| !intervals.is_empty());

                if has_data {
                    cache_comm_data.insert(
                        *cache_line_id,
                        SerializedCacheLineRecord {
                            per_core_intervals: std::mem::take(&mut record.per_core_intervals),
                            per_core_instruction_fetch_count: std::mem::take(
                                &mut record.per_core_instruction_fetch_count,
                            ),
                            per_core_page_walk_count: std::mem::take(
                                &mut record.per_core_page_walk_count,
                            ),
                            per_core_os_access_count: std::mem::take(
                                &mut record.per_core_os_access_count,
                            ),
                            per_core_load_exclusive_count: std::mem::take(
                                &mut record.per_core_load_exclusive_count,
                            ),
                            per_core_atomic_access_count: std::mem::take(
                                &mut record.per_core_atomic_access_count,
                            ),
                            // per_core_access_pc: record.per_core_access_pc.clone(),
                            // trace: std::mem::take(&mut record.trace),
                        },
                    );
                }
            }
        }

        // Serialize with MessagePack and compress with zstd
        let msgpack_data = rmp_serde::to_vec(&(total_memory_accesses, cache_comm_data)).unwrap();
        let compressed_data = zstd::encode_all(msgpack_data.as_slice(), 3).unwrap();
        let compressed_len = compressed_data.len();
        std::fs::write(file_path, compressed_data).unwrap();
        println!(
            "Dumped cache line communication to {} (msgpack+zstd, {} bytes)",
            file_path, compressed_len
        );
    }
}

#[repr(C)]
struct PerCoreMemoryAccess {
    total_accesses: u64,
    __padding: [u64; 7], // Padding to avoid false sharing
}

// Thresholds for busy-wait detection
const BUSY_WAIT_INSTRUCTION_THRESHOLD: u64 = 30; // Less than 30 instructions between WFE/CAS
const BUSY_WAIT_TIME_THRESHOLD: u64 = 100; // Less than 1 quantum (100 ns)

// Per-core WFE state for spin detection
#[derive(Clone, Debug)]
struct WFEState {
    last_instruction_count: u64,
    last_timestamp: u64,
}

impl WFEState {
    fn new() -> Self {
        WFEState {
            last_instruction_count: 0,
            last_timestamp: 0,
        }
    }
}

// Per-PC CAS state for spin detection with address tracking
// A CAS is recognized as entering a busy-wait loop when:
// 1. It's executed again with the same memory address
// 2. The instruction difference is less than the threshold
#[derive(Clone, Debug)]
struct CASPCState {
    last_instruction_count: u64,
    last_timestamp: u64,
    last_address: u64,  // Physical address of last CAS access
    in_busy_wait: bool, // Whether this PC is currently in a busy-wait loop
}

impl CASPCState {
    fn new() -> Self {
        CASPCState {
            last_instruction_count: 0,
            last_timestamp: 0,
            last_address: 0,
            in_busy_wait: false,
        }
    }
}

struct CommunicationRecorder {
    // Interrupt
    last_interrupt_timestamp: Vec<SpinMutex<u64>>,
    interrupt_intervals: Vec<SpinMutex<FxHashMap<u64, u64>>>, // per-core: interval_ns -> count

    // WFI and idle.
    last_wfi_timestamp: Vec<SpinMutex<u64>>,
    idle_intervals: Vec<SpinMutex<FxHashMap<u64, u64>>>, // per-core: WFI-to-interrupt interval_ns -> count

    // WFE spin detection
    wfe_state: Vec<SpinMutex<WFEState>>,
    wfe_total_count: Vec<SpinMutex<u64>>, // per-core: total count of WFE instructions executed
    wfe_busy_wait_count: Vec<SpinMutex<u64>>, // per-core: count of detected busy-wait loops
    wfe_instruction_diff_distribution: Vec<SpinMutex<FxHashMap<u64, u64>>>, // per-core: instruction_diff -> count
    wfe_time_diff_distribution: Vec<SpinMutex<FxHashMap<u64, u64>>>, // per-core: time_diff_ns -> count

    // CAS spin detection with per-PC tracking
    // Each core has a hashtable: PC -> CASPCState
    cas_pc_state: Vec<SpinMutex<FxHashMap<u64, CASPCState>>>,
    cas_total_count: Vec<SpinMutex<u64>>, // per-core: total count of CAS instructions executed
    cas_busy_wait_count: Vec<SpinMutex<u64>>, // per-core: count of detected busy-wait loop iterations
    cas_busy_wait_entry_count: Vec<SpinMutex<u64>>, // per-core: count of busy-wait loop entries (first spin detection)
    cas_instruction_diff_distribution: Vec<SpinMutex<FxHashMap<u64, u64>>>, // per-core: instruction_diff -> count
    cas_time_diff_distribution: Vec<SpinMutex<FxHashMap<u64, u64>>>, // per-core: time_diff_ns -> count

    // Necessary structure to model shared-memory access.
    mmu_state: Vec<SpinMutex<CommunicationMMU>>,
    l0_cache: L0InstructionCache<{ parameter::CORE_COUNT }>,
    memory_access_count: Vec<SpinMutex<PerCoreMemoryAccess>>,
    cache_line_records: ShardedCacheLineRecords,
}

impl CommunicationRecorder {
    fn new() -> Self {
        let mut last_interrupt_timestamp = Vec::with_capacity(parameter::CORE_COUNT);
        let mut last_wfi_timestamp = Vec::with_capacity(parameter::CORE_COUNT);
        let mut mmu_state = Vec::with_capacity(parameter::CORE_COUNT);
        let mut interrupt_intervals = Vec::with_capacity(parameter::CORE_COUNT);
        let mut idle_intervals = Vec::with_capacity(parameter::CORE_COUNT);
        let mut wfe_state = Vec::with_capacity(parameter::CORE_COUNT);
        let mut wfe_total_count = Vec::with_capacity(parameter::CORE_COUNT);
        let mut wfe_busy_wait_count = Vec::with_capacity(parameter::CORE_COUNT);
        let mut wfe_instruction_diff_distribution = Vec::with_capacity(parameter::CORE_COUNT);
        let mut wfe_time_diff_distribution = Vec::with_capacity(parameter::CORE_COUNT);
        let mut cas_pc_state = Vec::with_capacity(parameter::CORE_COUNT);
        let mut cas_total_count = Vec::with_capacity(parameter::CORE_COUNT);
        let mut cas_busy_wait_count = Vec::with_capacity(parameter::CORE_COUNT);
        let mut cas_busy_wait_entry_count = Vec::with_capacity(parameter::CORE_COUNT);
        let mut cas_instruction_diff_distribution = Vec::with_capacity(parameter::CORE_COUNT);
        let mut cas_time_diff_distribution = Vec::with_capacity(parameter::CORE_COUNT);

        for _ in 0..parameter::CORE_COUNT {
            last_interrupt_timestamp.push(SpinMutex::new(0));
            last_wfi_timestamp.push(SpinMutex::new(0));
            mmu_state.push(SpinMutex::new(CommunicationMMU::new()));
            interrupt_intervals.push(SpinMutex::new(FxHashMap::default()));
            idle_intervals.push(SpinMutex::new(FxHashMap::default()));
            wfe_state.push(SpinMutex::new(WFEState::new()));
            wfe_total_count.push(SpinMutex::new(0));
            wfe_busy_wait_count.push(SpinMutex::new(0));
            wfe_instruction_diff_distribution.push(SpinMutex::new(FxHashMap::default()));
            wfe_time_diff_distribution.push(SpinMutex::new(FxHashMap::default()));
            cas_pc_state.push(SpinMutex::new(FxHashMap::default()));
            cas_total_count.push(SpinMutex::new(0));
            cas_busy_wait_count.push(SpinMutex::new(0));
            cas_busy_wait_entry_count.push(SpinMutex::new(0));
            cas_instruction_diff_distribution.push(SpinMutex::new(FxHashMap::default()));
            cas_time_diff_distribution.push(SpinMutex::new(FxHashMap::default()));
        }

        CommunicationRecorder {
            cache_line_records: ShardedCacheLineRecords::new(),
            last_interrupt_timestamp,
            interrupt_intervals,
            last_wfi_timestamp,
            idle_intervals,
            wfe_state,
            wfe_total_count,
            wfe_busy_wait_count,
            wfe_instruction_diff_distribution,
            wfe_time_diff_distribution,
            cas_pc_state,
            cas_total_count,
            cas_busy_wait_count,
            cas_busy_wait_entry_count,
            cas_instruction_diff_distribution,
            cas_time_diff_distribution,
            mmu_state,
            l0_cache: L0InstructionCache::new(),
            memory_access_count: (0..parameter::CORE_COUNT)
                .map(|_| {
                    SpinMutex::new(PerCoreMemoryAccess {
                        total_accesses: 0,
                        __padding: [0; 7],
                    })
                })
                .collect(),
        }
    }

    fn record_memory_access(
        &self,
        core_id: u32,
        pa: u64,
        is_instruction: bool,
        is_atomic: bool,
        is_load_exclusive: bool,
        is_write: bool,
        is_os: bool,
        is_page_walk: bool,
        ts: u64,
        pc: u64,
        va: u64,
    ) {
        let cache_line_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
        self.memory_access_count[core_id as usize]
            .lock()
            .total_accesses += 1;
        self.cache_line_records.record_access(
            cache_line_id,
            core_id,
            is_instruction,
            is_write,
            is_os,
            is_page_walk,
            is_atomic,
            is_load_exclusive,
            ts,
            pc,
            va,
        );
    }

    fn record_interrupt(&self, core_id: u32, ts: u64) {
        let mut last_ts = self.last_interrupt_timestamp[core_id as usize].lock();
        if *last_ts > 0 {
            let interval = ts.saturating_sub(*last_ts);
            *self.interrupt_intervals[core_id as usize]
                .lock()
                .entry(interval)
                .or_insert(0) += 1;
        }
        *last_ts = ts;

        // Record idle interval (WFI to interrupt)
        let mut last_wfi_ts = self.last_wfi_timestamp[core_id as usize].lock();
        if *last_wfi_ts > 0 {
            let idle_interval = ts.saturating_sub(*last_wfi_ts);
            *self.idle_intervals[core_id as usize]
                .lock()
                .entry(idle_interval)
                .or_insert(0) += 1;
            *last_wfi_ts = 0; // Reset after recording
        }
    }

    fn record_wfi(&self, core_id: u32, ts: u64) {
        let mut last_wfi_ts = self.last_wfi_timestamp[core_id as usize].lock();
        assert!(ts != 0);
        if *last_wfi_ts == 0 {
            *last_wfi_ts = ts;
        }
    }

    fn record_wfe(&self, core_id: u32, ts: u64, instruction_count: u64) {
        // Increment total WFE count for this core
        *self.wfe_total_count[core_id as usize].lock() += 1;

        let mut state = self.wfe_state[core_id as usize].lock();

        // Check if we have a previous WFE to compare against
        if state.last_timestamp > 0 {
            // Calculate differences
            let instruction_diff = instruction_count.saturating_sub(state.last_instruction_count);
            let time_diff = ts.saturating_sub(state.last_timestamp);

            // Record in distributions for later analysis
            *self.wfe_instruction_diff_distribution[core_id as usize]
                .lock()
                .entry(instruction_diff)
                .or_insert(0) += 1;

            *self.wfe_time_diff_distribution[core_id as usize]
                .lock()
                .entry(time_diff)
                .or_insert(0) += 1;

            // Check if this looks like a busy-wait loop
            if instruction_diff < BUSY_WAIT_INSTRUCTION_THRESHOLD
                && time_diff < BUSY_WAIT_TIME_THRESHOLD
            {
                *self.wfe_busy_wait_count[core_id as usize].lock() += 1;
            }
        }

        // Update state for next WFE comparison
        state.last_instruction_count = instruction_count;
        state.last_timestamp = ts;
    }

    /// Record a CAS instruction execution with memory access information.
    ///
    /// The spin detection logic:
    /// 1. Track state per (core, PC) pair in a hashtable
    /// 2. A CAS is considered part of a busy-wait loop when:
    ///    - Same PC executed again
    ///    - Same memory address (physical)
    ///    - Instruction difference < threshold (30 instructions)
    /// 3. A busy-wait "entry" is counted only on the first spin detection for a PC
    ///    (when in_busy_wait transitions from false to true)
    fn record_cas(&self, core_id: u32, pc: u64, pa: u64, ts: u64, instruction_count: u64) {
        // Increment total CAS count for this core
        *self.cas_total_count[core_id as usize].lock() += 1;

        let mut pc_states = self.cas_pc_state[core_id as usize].lock();
        let state = pc_states.entry(pc).or_insert_with(CASPCState::new);

        // Check if we have a previous CAS at this PC to compare against
        if state.last_timestamp > 0 {
            // Calculate differences
            let instruction_diff = instruction_count.saturating_sub(state.last_instruction_count);
            let time_diff = ts.saturating_sub(state.last_timestamp);

            // Record in distributions for later analysis
            *self.cas_instruction_diff_distribution[core_id as usize]
                .lock()
                .entry(instruction_diff)
                .or_insert(0) += 1;

            *self.cas_time_diff_distribution[core_id as usize]
                .lock()
                .entry(time_diff)
                .or_insert(0) += 1;

            // Check if this looks like a busy-wait loop:
            // - Same address as before
            // - Instruction difference below threshold
            let same_address = state.last_address == pa;
            let is_spinning = same_address && instruction_diff < BUSY_WAIT_INSTRUCTION_THRESHOLD;

            if is_spinning {
                // Count this as a busy-wait iteration
                *self.cas_busy_wait_count[core_id as usize].lock() += 1;

                // If this is a new entry into busy-wait (wasn't spinning before),
                // count it as an entry
                if !state.in_busy_wait {
                    *self.cas_busy_wait_entry_count[core_id as usize].lock() += 1;
                    state.in_busy_wait = true;
                }
            } else {
                // Not spinning anymore (address changed or too many instructions)
                state.in_busy_wait = false;
            }
        }

        // Update state for next CAS comparison at this PC
        state.last_instruction_count = instruction_count;
        state.last_timestamp = ts;
        state.last_address = pa;
    }

    fn flush_tlb(&self, core_id: u32, mode: MMUFlushMode) {
        self.mmu_state[core_id as usize].lock().flush(mode);
    }

    fn translate_and_record(
        &self,
        core_id: u32,
        va: u64,
        ts: u64,
        is_instruction: bool,
        is_os: bool,
    ) -> Option<u64> {
        let translation = self.mmu_state[core_id as usize]
            .lock()
            .translate_and_refill(core_id, va, ts, is_instruction);

        match translation {
            MMUTranslationResult::Hit(pa, _) => Some(pa),
            MMUTranslationResult::Miss(pa, walk_traces) => {
                // Record page walk accesses
                for &trace_pa in &walk_traces {
                    if trace_pa == u64::MAX {
                        break;
                    }
                    let cache_line_id = trace_pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
                    self.cache_line_records.record_access(
                        cache_line_id,
                        core_id,
                        false, // page walks are reads
                        false,
                        is_os,
                        true, // this is a page walk
                        false,
                        false,
                        ts,
                        0,
                        0,
                    );
                    self.memory_access_count[core_id as usize]
                        .lock()
                        .total_accesses += 1;
                }
                Some(pa)
            }
            MMUTranslationResult::MissNotCacheable(pa) => Some(pa),
        }
    }

    fn dump(&self) {
        if MODEL_SHARED_MEMORY {
            // Dump cache line communication intervals
            let cache_comm_file = format!("cache_line_communication.msgpack.zstd");
            let total_memory_accesses: u64 = self
                .memory_access_count
                .iter()
                .map(|m| m.lock().total_accesses)
                .sum();
            self.cache_line_records
                .dump(&cache_comm_file, total_memory_accesses);
        }

        // Dump per-core interrupt intervals
        let mut per_core_interrupt_intervals = FxHashMap::default();
        for (core_id, intervals_lock) in self.interrupt_intervals.iter().enumerate() {
            let intervals = intervals_lock.lock();
            if !intervals.is_empty() {
                per_core_interrupt_intervals.insert(core_id, intervals.clone());
            }
        }
        let msgpack_data = rmp_serde::to_vec(&per_core_interrupt_intervals).unwrap();

        let interrupt_file = std::fs::File::create("interrupt_intervals.msgpack.zstd").unwrap();
        let mut encoder = zstd::stream::Encoder::new(interrupt_file, 3).unwrap();
        std::io::Write::write_all(&mut encoder, &msgpack_data).unwrap();
        encoder.finish().unwrap();
        println!(
            "Dumped per-core interrupt intervals to interrupt_intervals.msgpack.zstd (msgpack+zstd)"
        );

        // Dump per-core idle intervals (WFI to interrupt)
        let mut per_core_idle_intervals = FxHashMap::default();
        for (core_id, intervals_lock) in self.idle_intervals.iter().enumerate() {
            let intervals = intervals_lock.lock();
            if !intervals.is_empty() {
                per_core_idle_intervals.insert(core_id, intervals.clone());
            }
        }
        let msgpack_data = rmp_serde::to_vec(&per_core_idle_intervals).unwrap();

        let idle_file = std::fs::File::create("idle_intervals.msgpack.zstd").unwrap();
        let mut encoder = zstd::stream::Encoder::new(idle_file, 3).unwrap();
        std::io::Write::write_all(&mut encoder, &msgpack_data).unwrap();
        encoder.finish().unwrap();
        println!("Dumped per-core idle intervals to idle_intervals.msgpack.zstd (msgpack+zstd)");

        // Dump WFE spin detection data
        // Format: (total_wfe_count, busy_wait_count, instruction_diff_distribution, time_diff_distribution)
        let mut wfe_data: FxHashMap<usize, (u64, u64, FxHashMap<u64, u64>, FxHashMap<u64, u64>)> =
            FxHashMap::default();
        for core_id in 0..parameter::CORE_COUNT {
            let total_count = *self.wfe_total_count[core_id].lock();
            let busy_wait_count = *self.wfe_busy_wait_count[core_id].lock();
            let instruction_diff_dist = self.wfe_instruction_diff_distribution[core_id]
                .lock()
                .clone();
            let time_diff_dist = self.wfe_time_diff_distribution[core_id].lock().clone();

            // Only include cores that have WFE data
            if total_count > 0
                || busy_wait_count > 0
                || !instruction_diff_dist.is_empty()
                || !time_diff_dist.is_empty()
            {
                wfe_data.insert(
                    core_id,
                    (
                        total_count,
                        busy_wait_count,
                        instruction_diff_dist,
                        time_diff_dist,
                    ),
                );
            }
        }

        if !wfe_data.is_empty() {
            let msgpack_data = rmp_serde::to_vec(&wfe_data).unwrap();
            let wfe_file = std::fs::File::create("wfe_spin_detection.msgpack.zstd").unwrap();
            let mut encoder = zstd::stream::Encoder::new(wfe_file, 3).unwrap();
            std::io::Write::write_all(&mut encoder, &msgpack_data).unwrap();
            encoder.finish().unwrap();
            println!(
                "Dumped WFE spin detection data to wfe_spin_detection.msgpack.zstd (msgpack+zstd)"
            );

            // Print summary
            let total_wfe: u64 = wfe_data.values().map(|(total, _, _, _)| total).sum();
            let total_busy_wait: u64 = wfe_data.values().map(|(_, count, _, _)| count).sum();
            println!(
                "WFE spin detection summary: {} total WFE instructions, {} busy-wait loop iterations detected",
                total_wfe, total_busy_wait
            );
        }

        // Dump CAS spin detection data
        // Format: (total_cas_count, busy_wait_count, busy_wait_entry_count, instruction_diff_distribution, time_diff_distribution)
        let mut cas_data: FxHashMap<
            usize,
            (u64, u64, u64, FxHashMap<u64, u64>, FxHashMap<u64, u64>),
        > = FxHashMap::default();
        for core_id in 0..parameter::CORE_COUNT {
            let total_count = *self.cas_total_count[core_id].lock();
            let busy_wait_count = *self.cas_busy_wait_count[core_id].lock();
            let busy_wait_entry_count = *self.cas_busy_wait_entry_count[core_id].lock();
            let instruction_diff_dist = self.cas_instruction_diff_distribution[core_id]
                .lock()
                .clone();
            let time_diff_dist = self.cas_time_diff_distribution[core_id].lock().clone();

            // Only include cores that have CAS data
            if total_count > 0
                || busy_wait_count > 0
                || busy_wait_entry_count > 0
                || !instruction_diff_dist.is_empty()
                || !time_diff_dist.is_empty()
            {
                cas_data.insert(
                    core_id,
                    (
                        total_count,
                        busy_wait_count,
                        busy_wait_entry_count,
                        instruction_diff_dist,
                        time_diff_dist,
                    ),
                );
            }
        }

        if !cas_data.is_empty() {
            let msgpack_data = rmp_serde::to_vec(&cas_data).unwrap();
            let cas_file = std::fs::File::create("cas_spin_detection.msgpack.zstd").unwrap();
            let mut encoder = zstd::stream::Encoder::new(cas_file, 3).unwrap();
            std::io::Write::write_all(&mut encoder, &msgpack_data).unwrap();
            encoder.finish().unwrap();
            println!(
                "Dumped CAS spin detection data to cas_spin_detection.msgpack.zstd (msgpack+zstd)"
            );

            // Print summary
            let total_cas: u64 = cas_data.values().map(|(total, _, _, _, _)| total).sum();
            let total_busy_wait: u64 = cas_data.values().map(|(_, count, _, _, _)| count).sum();
            let total_entries: u64 = cas_data.values().map(|(_, _, entries, _, _)| entries).sum();
            println!(
                "CAS spin detection summary: {} total CAS, {} busy-wait iterations, {} busy-wait entries",
                total_cas, total_busy_wait, total_entries
            );
        }
    }
}

static mut PLUGIN: *const CommunicationRecorder = std::ptr::null();

// Periodic checking callback to serialize and quit when threshold is reached
unsafe extern "C" fn periodic_checking_callback(_diff: u64) -> bool {
    unsafe {
        let current_time = std::ptr::addr_of!(QUANTUM_GENERATION).read() * 100;
        let threshold = std::ptr::addr_of!(QUIT_THRESHOLD_NS).read();

        if current_time >= threshold {
            println!(
                "Communication plugin: Reached threshold {} ns at {} ns. Serializing and quitting...",
                threshold, current_time
            );

            // Serialize the data
            if !PLUGIN.is_null() {
                let recorder = &*PLUGIN;
                recorder.dump();
            }

            println!("Communication plugin: Serialization complete. Exiting...");

            // Print host timing breakdown.
            crate::debug::timing::print_time_breakdown("simulation_ckpt_time.json");

            // Save the statistics.
            Statistics::save_to_csv("statistics.final.csv", current_time);
            NocTraffic::save_to_csv("noc_traffic.final.csv");
            crate::plugin_on_exit();
            std::process::exit(0);
        }
    }
    false
}

unsafe extern "C" fn vcpu_mem_access(
    vcpu_idx: u32,
    info: qemu_api::qemu_plugin_meminfo_t,
    vaddr: u64,
    inst_virtual_addr: *mut ffi::c_void,
) {
    unsafe {
        let hw_handler = qemu_api::qemu_plugin_get_hwaddr(info, vaddr);
        let is_device = qemu_api::qemu_plugin_hwaddr_is_io(hw_handler);

        if !is_device {
            let is_store = qemu_api::qemu_plugin_mem_is_store(info);
            let pa = qemu_api::qemu_plugin_hwaddr_phys_addr(hw_handler);

            // Decode user data:
            // Bits [48:0]: instruction virtual address (49 bits)
            // Bit  [49]:   is_atomic flag
            // Bit  [50]:   is_load_exclusive flag
            // Bit  [51]:   is_cas flag
            // Bits [52+]:  offset
            let userdata = inst_virtual_addr as u64;
            let inst_virtual_addr = userdata & 0x1_ffff_ffff_ffff;
            let is_atomic = ((userdata >> 49) & 1) != 0;
            let is_load_exclusive = ((userdata >> 50) & 1) != 0;
            let is_cas = ((userdata >> 51) & 1) != 0;

            let is_os = (inst_virtual_addr >> 48) & 1 == 1;
            let pc = if is_os {
                inst_virtual_addr | 0xffff_0000_0000_0000
            } else {
                inst_virtual_addr
            };
            let ts = std::ptr::addr_of!(QUANTUM_GENERATION).read_volatile() * 100;

            let recorder = &*PLUGIN;

            // Handle CAS instruction spin detection
            if is_cas {
                // Get instruction count from statistics
                let (instruction_count, _, _) = Statistics::global_query_record(
                    vcpu_idx,
                    crate::debug::statistics::EventType::Instruction,
                );

                // Record CompareAndSwap in global statistics
                Statistics::global_record(
                    vcpu_idx,
                    crate::debug::statistics::EventType::CompareAndSwap,
                    is_os,
                );

                // Record CAS with physical address for spin detection
                recorder.record_cas(vcpu_idx, pc, pa, ts, instruction_count);
            }

            // Record the memory access (only if MODEL_SHARED_MEMORY is enabled)
            if MODEL_SHARED_MEMORY {
                recorder.record_memory_access(
                    vcpu_idx,
                    pa,
                    false, // is_instruction
                    is_atomic,
                    is_load_exclusive,
                    is_store,
                    is_os,
                    false, // is_page_walk (handled separately in translate_and_record)
                    ts,
                    pc,
                    vaddr,
                );
            }
        }
    }
}

unsafe extern "C" fn vcpu_insn_exec(vcpu_idx: u32, inst_virtual_addr: *mut ffi::c_void) {
    unsafe {
        let vpn = qemu_api::qemu_plugin_read_pc_vpn();
        let vaddr = vpn << 12 | (inst_virtual_addr as u64 & 0xfff);

        let recorder = &*PLUGIN;

        // Check L0 instruction cache (no lock needed, L0InstructionCache uses interior mutability)
        if recorder.l0_cache.check_and_update(vcpu_idx, vaddr) {
            return;
        }

        let ts = std::ptr::addr_of!(QUANTUM_GENERATION).read_volatile() * 100;
        let is_os = vaddr >> 63 == 1;

        // Translate through MMU and record page walks if necessary
        if let Some(pa) = recorder.translate_and_record(vcpu_idx, vaddr, ts, true, is_os) {
            recorder.record_memory_access(
                vcpu_idx, pa, true, false, false, false, is_os, false, ts, vaddr, vaddr,
            );
        }
    }
}

unsafe extern "C" fn vcpu_tlb_flush(
    vcpu_idx: u32,
    mode: u32,
    asid: u64,
    vpn: u64,
    page_count: u64,
) {
    unsafe {
        let info = if mode == 0 {
            MMUFlushMode::All
        } else if mode == 1 {
            MMUFlushMode::ByASID(AddressSpaceID::NonGlobal(asid as u16))
        } else if mode == 2 {
            MMUFlushMode::ByVPN(vpn, page_count)
        } else if mode == 3 {
            MMUFlushMode::ByVPNAndASID(vpn, page_count, AddressSpaceID::NonGlobal(asid as u16))
        } else {
            return;
        };

        if parameter::MEASURE_HALF_OF_CORES && vcpu_idx >= parameter::CORE_COUNT as u32 / 2 {
            return;
        }

        let recorder = &*PLUGIN;
        recorder.flush_tlb(vcpu_idx, info);
    }
}

unsafe extern "C" fn vcpu_interrupt_delivered(vcpu_idx: u32) {
    unsafe {
        let ts = std::ptr::addr_of!(QUANTUM_GENERATION).read_volatile() * 100;
        let recorder = &*PLUGIN;
        recorder.record_interrupt(vcpu_idx, ts);
    }
}

unsafe extern "C" fn vcpu_exec_wfi(vcpu_idx: u32, _: *mut ffi::c_void) {
    unsafe {
        let ts = std::ptr::addr_of!(QUANTUM_GENERATION).read_volatile() * 100;

        let recorder = &*PLUGIN;
        recorder.record_wfi(vcpu_idx, ts);
    }
}

unsafe extern "C" fn vcpu_exec_wfe(vcpu_idx: u32, _: *mut ffi::c_void) {
    unsafe {
        let ts = std::ptr::addr_of!(QUANTUM_GENERATION).read_volatile() * 100;

        // Get instruction count from statistics
        let (instruction_count, _, _) = Statistics::global_query_record(
            vcpu_idx,
            crate::debug::statistics::EventType::Instruction,
        );

        // Record WaitForEvent in global statistics
        let is_os = false; // WFE is typically executed in user space for spin locks
        Statistics::global_record(
            vcpu_idx,
            crate::debug::statistics::EventType::WaitForEvent,
            is_os,
        );

        let recorder = &*PLUGIN;
        recorder.record_wfe(vcpu_idx, ts, instruction_count);
    }
}

// Note: CAS handling has been moved to vcpu_mem_access callback
// because CAS spin detection requires the physical address of the memory access

pub struct CommunicationRecordingPlugin {}

impl super::super::Plugin for CommunicationRecordingPlugin {
    fn init(_plugin_id: u64, options: &rustc_hash::FxHashMap<String, String>) {
        unsafe {
            PLUGIN = Box::into_raw(Box::new(CommunicationRecorder::new()));

            // Parse quit_threshold option (in nanoseconds)
            if let Some(threshold_str) = options.get("quit_threshold_ns") {
                match threshold_str.parse::<u64>() {
                    Ok(threshold) => {
                        QUIT_THRESHOLD_NS = threshold;
                        println!(
                            "Communication plugin: Quit threshold set to {} ns ({:.2} ms)",
                            threshold,
                            threshold as f64 / 1_000_000.0
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "Communication plugin: Failed to parse quit_threshold_ns '{}': {}. Using default (no auto-quit).",
                            threshold_str, e
                        );
                    }
                }
            } else {
                println!(
                    "Communication plugin: No quit_threshold_ns specified. Auto-quit disabled."
                );
            }

            // Register the quantum timestamp variable with QEMU
            // QEMU will update this variable as simulation time advances
            let timestamp_ptr = &raw mut QUANTUM_GENERATION;
            let registered =
                qemu_api::qemu_plugin_register_plugin_quantum_generation_increment_variable(
                    timestamp_ptr,
                );
            if !registered {
                eprintln!(
                    "Warning: Failed to register quantum timestamp variable. Timestamps may not be accurate."
                );
                panic!();
            } else {
                println!(
                    "Communication plugin: Registered quantum timestamp variable at {:?}",
                    timestamp_ptr
                );
            }

            // Register periodic checking callback if threshold is set
            if QUIT_THRESHOLD_NS != u64::MAX {
                let callback_registered = qemu_api::qemu_plugin_register_periodic_check_cb(Some(
                    periodic_checking_callback,
                ));
                if callback_registered {
                    println!(
                        "Communication plugin: Registered periodic checking callback for auto-quit"
                    );
                } else {
                    eprintln!(
                        "Warning: Failed to register periodic checking callback. Auto-quit may not work."
                    );
                }
            }

            // Register interrupt callback
            qemu_api::qemu_plugin_register_on_deliver_interrupt_cb(Some(vcpu_interrupt_delivered));

            // Register TLB flush callback
            if MODEL_SHARED_MEMORY {
                qemu_api::qemu_plugin_register_flushing_local_tlb_cb(Some(vcpu_tlb_flush));
            }
        }
    }

    unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
        unsafe {
            let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

            if n_instruction == 0 {
                return;
            }

            assert!(n_instruction < 32768);

            let mut block_id = vec![];
            let mut insn_flags = vec![]; // Store (is_atomic, is_load_exclusive, is_cas) for each instruction

            for i in 0..n_instruction {
                let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
                block_id.push(
                    qemu_api::qemu_plugin_insn_haddr(inst) as usize
                        >> crate::parameter::CACHE_LINE_SIZE.trailing_zeros(),
                );

                // Decode the instruction to determine if it's atomic or load-exclusive
                let insn_data_ptr = qemu_api::qemu_plugin_insn_data(inst);
                let insn_bytes = std::slice::from_raw_parts(insn_data_ptr as *const u8, 4);

                // AArch64 instructions are little-endian 32-bit values
                let insn_word = u32::from_le_bytes([
                    insn_bytes[0],
                    insn_bytes[1],
                    insn_bytes[2],
                    insn_bytes[3],
                ]);

                let is_atomic = aarch64_decoder::is_atomic_operation(insn_word);
                let is_load_exclusive = aarch64_decoder::is_load_exclusive(insn_word);
                let is_cas = aarch64_decoder::is_cas_operation(insn_word);

                insn_flags.push((is_atomic, is_load_exclusive, is_cas));
            }

            let fb_info = crate::util::find_fetch_block_from_block_id_sequence(block_id);

            // bind the instruction callback.
            for (idx, _) in fb_info.into_iter() {
                let i = qemu_api::qemu_plugin_tb_get_insn(tb, idx);

                let insn_addr = (qemu_api::qemu_plugin_insn_vaddr(i) as u64) & 0x1_ffff_ffff_ffff;
                let offset = idx as u64;

                // Encode flags in user data:
                // Bits [48:0]: instruction virtual address (49 bits)
                // Bit  [49]:   is_atomic flag
                // Bit  [50]:   is_load_exclusive flag
                // Bit  [51]:   is_cas flag
                // Bits [52+]:  offset
                let (is_atomic, is_load_exclusive, is_cas) = insn_flags[idx as usize];
                let combined = insn_addr
                    | ((is_atomic as u64) << 49)
                    | ((is_load_exclusive as u64) << 50)
                    | ((is_cas as u64) << 51)
                    | (offset << 52);

                if MODEL_SHARED_MEMORY {
                    qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                        i,
                        Some(vcpu_insn_exec),
                        qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        combined as *mut ffi::c_void,
                    );
                }
            }

            // Check for WFI and WFE instructions and register callbacks
            for i in 0..n_instruction {
                let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
                let literal = qemu_api::qemu_plugin_insn_data(inst) as *const u32;
                let literal = *literal;

                // WFI instruction encoding for AArch64: 0xD503207F
                if literal == 0b_1101_0101_0000_0011_0010_0000_0111_1111 {
                    qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                        inst,
                        Some(vcpu_exec_wfi),
                        qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        std::ptr::null_mut(),
                    );
                }

                // WFE instruction encoding for AArch64: 0xD503205F
                // Binary: 1101_0101_0000_0011_0010_0000_0101_1111
                if literal == 0b_1101_0101_0000_0011_0010_0000_0101_1111 {
                    qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                        inst,
                        Some(vcpu_exec_wfe),
                        qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                        std::ptr::null_mut(),
                    );
                }

                // CAS instructions are handled in the memory callback below
                // where we have access to the physical address
            }

            // bind the memory callback.
            // Note: We register memory callbacks for CAS instructions regardless of MODEL_SHARED_MEMORY
            // because CAS spin detection requires the physical address from memory accesses.
            for i in 0..n_instruction {
                let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
                let (is_atomic, is_load_exclusive, is_cas) = insn_flags[i as usize];

                // Skip non-CAS instructions if MODEL_SHARED_MEMORY is false
                if !MODEL_SHARED_MEMORY && !is_cas {
                    continue;
                }

                let insn_addr =
                    (qemu_api::qemu_plugin_insn_vaddr(inst) as u64) & 0x1_ffff_ffff_ffff;
                let offset = i as u64;

                // Encode flags in user data:
                // Bits [48:0]: instruction virtual address (49 bits)
                // Bit  [49]:   is_atomic flag
                // Bit  [50]:   is_load_exclusive flag
                // Bit  [51]:   is_cas flag
                // Bits [52+]:  offset
                let combined = insn_addr
                    | ((is_atomic as u64) << 49)
                    | ((is_load_exclusive as u64) << 50)
                    | ((is_cas as u64) << 51)
                    | (offset << 52);

                qemu_api::qemu_plugin_register_vcpu_mem_cb(
                    inst,
                    Some(vcpu_mem_access),
                    qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                    qemu_api::qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
                    combined as *mut ffi::c_void,
                );
            }
        }
    }

    fn serialize(_name: &str) {}

    fn deserialize(_name: &str) {
        // For this plugin, we don't need to restore state from checkpoints
        // as we're only recording intervals during runtime
    }
}
