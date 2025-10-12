use crate::{
    arch::AArch64,
    components::cache_hierarchy::{
        common::L0InstructionCache,
        mmu::{self, AbstractMMU, MMUFlushMode, MMUTranslationResult, tlb::AddressSpaceID},
    },
    parameter, qemu_api,
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use spin::Mutex as SpinMutex;
use std::ffi;

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
    // intervals: Vec<u64>,
    intervals: FxHashMap<u64, u64>, // interval_ns -> count
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct CommunicationInterval {
    interval_ns: u64,
    // from_core: u32,
    // to_core: u32,
    is_write: bool,
    is_os_access: bool,
    is_page_walk: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct InterruptInterval {
    interval_ns: u64,
    core_id: u32,
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
        is_write: bool,
        is_os: bool,
        is_page_walk: bool,
        ts: u64,
    ) {
        let shard_idx = Self::get_shard_index(cache_line_id);
        let mut shard = self.shards[shard_idx].lock();

        if is_write {
            // This is a write access
            let record = shard.entry(cache_line_id).or_insert(CacheLineAccessRecord {
                last_write_timestamp: ts,
                last_writer_core: core_id,
                // intervals: Vec::new(),
                intervals: FxHashMap::default(),
            });

            // Check if different core wrote to the same cache line
            if record.last_writer_core != core_id && record.last_write_timestamp > 0 {
                let interval = ts.saturating_sub(record.last_write_timestamp);
                // record.intervals.push(CommunicationInterval {
                //     interval_ns: interval,
                //     // from_core: record.last_writer_core,
                //     // to_core: core_id,
                //     is_write: true,
                //     is_os_access: is_os,
                //     is_page_walk,
                // });
                // record.intervals.push(interval);
                *record.intervals.entry(interval).or_insert(0) += 1;
            }

            record.last_write_timestamp = ts;
            record.last_writer_core = core_id;
        } else {
            // This is a read access
            if let Some(record) = shard.get_mut(&cache_line_id) {
                if record.last_writer_core != core_id {
                    // Different core reading after a write
                    let interval = ts.saturating_sub(record.last_write_timestamp);
                    // record.intervals.push(CommunicationInterval {
                    //     interval_ns: interval,
                    //     // from_core: record.last_writer_core,
                    //     // to_core: core_id,
                    //     is_write: false,
                    //     is_os_access: is_os,
                    //     is_page_walk,
                    // });
                    // record.intervals.push(interval);
                    *record.intervals.entry(interval).or_insert(0) += 1;
                }
            }
        }
    }

    fn dump_to_json(&self, file_path: &str, total_memory_accesses: u64) {
        let mut cache_comm_data = FxHashMap::default();

        // Collect data from all shards
        for shard in &self.shards {
            let shard_guard = shard.lock();
            for (cache_line_id, record) in shard_guard.iter() {
                if !record.intervals.is_empty() {
                    cache_comm_data.insert(
                        *cache_line_id,
                        record.intervals.clone(),
                    );
                }
            }
        }

        let json_str = serde_json::to_string_pretty(&(total_memory_accesses, cache_comm_data)).unwrap();
        std::fs::write(file_path, json_str).unwrap();
        println!("Dumped cache line communication to {}", file_path);
    }
}

#[repr(C)]
struct PerCoreMemoryAccess {
    total_accesses: u64,
    __padding: [u64; 7], // Padding to avoid false sharing
}

struct CommunicationRecorder {
    cache_line_records: ShardedCacheLineRecords,
    last_interrupt_timestamp: Vec<SpinMutex<u64>>,
    // interrupt_intervals: SpinMutex<Vec<InterruptInterval>>,
    interrupt_intervals: SpinMutex<FxHashMap<u64, u64>>, // interval_ns -> count
    mmu_state: Vec<SpinMutex<CommunicationMMU>>,
    l0_cache: L0InstructionCache<{ parameter::CORE_COUNT }>,
    memory_access_count: Vec<SpinMutex<PerCoreMemoryAccess>>,
}

impl CommunicationRecorder {
    fn new() -> Self {
        let mut last_interrupt_timestamp = Vec::with_capacity(parameter::CORE_COUNT);
        let mut mmu_state = Vec::with_capacity(parameter::CORE_COUNT);

        for _ in 0..parameter::CORE_COUNT {
            last_interrupt_timestamp.push(SpinMutex::new(0));
            mmu_state.push(SpinMutex::new(CommunicationMMU::new()));
        }

        CommunicationRecorder {
            cache_line_records: ShardedCacheLineRecords::new(),
            last_interrupt_timestamp,
            interrupt_intervals: SpinMutex::new(FxHashMap::default()),
            mmu_state,
            l0_cache: L0InstructionCache::new(),
            memory_access_count: (0..parameter::CORE_COUNT)
                .map(|_| SpinMutex::new(PerCoreMemoryAccess {
                    total_accesses: 0,
                    __padding: [0; 7],
                }))
                .collect(),
        }
    }

    fn record_memory_access(
        &self,
        core_id: u32,
        pa: u64,
        is_write: bool,
        is_os: bool,
        is_page_walk: bool,
        ts: u64,
    ) {
        let cache_line_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
        self.memory_access_count[core_id as usize].lock().total_accesses += 1;
        self.cache_line_records.record_access(
            cache_line_id,
            core_id,
            is_write,
            is_os,
            is_page_walk,
            ts,
        );
    }

    fn record_interrupt(&self, core_id: u32, ts: u64) {
        let mut last_ts = self.last_interrupt_timestamp[core_id as usize].lock();
        if *last_ts > 0 {
            let interval = ts.saturating_sub(*last_ts);
            *self.interrupt_intervals.lock().entry(interval).or_insert(0) += 1;
        }
        *last_ts = ts;
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
                        is_os,
                        true, // this is a page walk
                        ts,
                    );
                    self.memory_access_count[core_id as usize].lock().total_accesses += 1;
                }
                Some(pa)
            }
            MMUTranslationResult::MissNotCacheable(pa) => Some(pa),
        }
    }

    fn dump_to_json(&self, name: &str) {
        // Dump cache line communication intervals
        let cache_comm_file = format!("{}/cache_line_communication.json", name);
        let total_memory_accesses: u64 = self.memory_access_count.iter().map(|m| m.lock().total_accesses).sum();
        self.cache_line_records.dump_to_json(&cache_comm_file, total_memory_accesses);

        // Dump interrupt intervals
        let interrupt_file = format!("{}/interrupt_intervals.json", name);
        let intervals = self.interrupt_intervals.lock();
        let json_str = serde_json::to_string_pretty(&*intervals).unwrap();
        std::fs::write(&interrupt_file, json_str).unwrap();
        println!("Dumped interrupt intervals to {}", interrupt_file);
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
                let snapshot_name = format!("communication_final_{}", current_time);
                std::fs::create_dir_all(&snapshot_name).unwrap();
                recorder.dump_to_json(&snapshot_name);
            }

            println!("Communication plugin: Serialization complete. Exiting...");
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
            let inst_virtual_addr = inst_virtual_addr as u64;
            let is_os = (inst_virtual_addr >> 48) & 1 == 1;
            let ts = std::ptr::addr_of!(QUANTUM_GENERATION).read_volatile() * 100;

            let recorder = &*PLUGIN;

            // Record the memory access (page walk tracking happens in translate_and_record if needed)
            recorder.record_memory_access(vcpu_idx, pa, is_store, is_os, false, ts);
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
            recorder.record_memory_access(vcpu_idx, pa, false, is_os, false, ts);
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
            qemu_api::qemu_plugin_register_flushing_local_tlb_cb(Some(vcpu_tlb_flush));
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
            for i in 0..n_instruction {
                let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);
                block_id.push(
                    qemu_api::qemu_plugin_insn_haddr(inst) as usize
                        >> crate::parameter::CACHE_LINE_SIZE.trailing_zeros(),
                );
            }

            let fb_info = crate::util::find_fetch_block_from_block_id_sequence(block_id);

            // bind the instruction callback.
            for (idx, _) in fb_info.into_iter() {
                let i = qemu_api::qemu_plugin_tb_get_insn(tb, idx);

                let insn_addr = (qemu_api::qemu_plugin_insn_vaddr(i) as u64) & 0x1_ffff_ffff_ffff;
                let offset = idx as u64;
                let combined = insn_addr | (offset << 49);

                qemu_api::qemu_plugin_register_vcpu_insn_exec_cb(
                    i,
                    Some(vcpu_insn_exec),
                    qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                    combined as *mut ffi::c_void,
                );
            }

            // bind the memory callback.
            for i in 0..n_instruction {
                let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);

                let insn_addr =
                    (qemu_api::qemu_plugin_insn_vaddr(inst) as u64) & 0x1_ffff_ffff_ffff;
                let offset = i as u64;
                let combined = insn_addr | (offset << 49);

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
