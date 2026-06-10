use crate::{
    debug::statistics::{EventType, Statistics},
    parameter, qemu_api,
};
use spin::Mutex as SpinMutex;
use std::collections::hash_map::Entry;
use std::ffi;

const NUM_SHARDS: usize = 32768;
const SHARD_MASK: u64 = 0x7FFF;

#[allow(dead_code)]
enum BlockAccessState {
    JustWrite { core_id: u32, target_time: u64 },
    UnorderedWrite { max_target_time: u64 },
    JustRead { readers: Vec<(u32, u64)> },
}

struct ShardedCausalityRecords {
    shards: Vec<SpinMutex<rustc_hash::FxHashMap<u64, BlockAccessState>>>,
}

impl ShardedCausalityRecords {
    fn new() -> Self {
        let mut shards = Vec::with_capacity(NUM_SHARDS);
        for _ in 0..NUM_SHARDS {
            shards.push(SpinMutex::new(rustc_hash::FxHashMap::default()));
        }
        ShardedCausalityRecords { shards }
    }

    #[inline]
    fn shard_index(cache_line_id: u64) -> usize {
        (cache_line_id & SHARD_MASK) as usize
    }

    fn clear_all(&self) {
        for shard in &self.shards {
            shard.lock().clear();
        }
    }

    fn process_read(&self, cache_line_id: u64, core_id: u32, target_time: u64) {
        let idx = Self::shard_index(cache_line_id);
        let violation_count = {
            let mut shard = self.shards[idx].lock();
            let mut count = 0u64;

            match shard.entry(cache_line_id) {
                Entry::Vacant(v) => {
                    v.insert(BlockAccessState::JustRead {
                        readers: vec![(core_id, target_time)],
                    });
                }
                Entry::Occupied(mut o) => {
                    let state = o.get_mut();
                    match state {
                        BlockAccessState::JustWrite {
                            target_time: w_time,
                            ..
                        } => {
                            if target_time < *w_time {
                                count += 1;
                            } else {
                                *state = BlockAccessState::JustRead {
                                    readers: vec![(core_id, target_time)],
                                };
                            }
                        }
                        BlockAccessState::JustRead { readers } => {
                            readers.push((core_id, target_time));
                        }
                        BlockAccessState::UnorderedWrite { .. } => {
                            count += 1;
                        }
                    }
                }
            }

            count
        };

        if violation_count > 0 {
            Statistics::global_record_by(
                core_id,
                EventType::SharedMemoryCausalityViolation,
                false,
                violation_count,
            );
        }
    }

    fn process_write(&self, cache_line_id: u64, core_id: u32, target_time: u64) {
        let idx = Self::shard_index(cache_line_id);
        let violation_count = {
            let mut shard = self.shards[idx].lock();
            let mut count = 0u64;

            match shard.entry(cache_line_id) {
                Entry::Vacant(v) => {
                    v.insert(BlockAccessState::JustWrite {
                        core_id,
                        target_time,
                    });
                }
                Entry::Occupied(mut o) => {
                    let state = o.get_mut();
                    match state {
                        BlockAccessState::JustWrite {
                            target_time: existing_time,
                            ..
                        } => {
                            if target_time < *existing_time {
                                *state = BlockAccessState::UnorderedWrite {
                                    max_target_time: (*existing_time).max(target_time),
                                };
                            } else {
                                *state = BlockAccessState::JustWrite {
                                    core_id,
                                    target_time,
                                };
                            }
                        }
                        BlockAccessState::JustRead { readers } => {
                            for (_r_core, r_time) in readers.iter() {
                                if *r_time > target_time {
                                    count += 1;
                                }
                            }
                            *state = BlockAccessState::JustWrite {
                                core_id,
                                target_time,
                            };
                        }
                        BlockAccessState::UnorderedWrite { max_target_time } => {
                            if target_time > *max_target_time {
                                *state = BlockAccessState::JustWrite {
                                    core_id,
                                    target_time,
                                };
                            } else {
                                *max_target_time = (*max_target_time).max(target_time);
                            }
                        }
                    }
                }
            }

            count
        };

        if violation_count > 0 {
            Statistics::global_record_by(
                core_id,
                EventType::SharedMemoryCausalityViolation,
                false,
                violation_count,
            );
        }
    }
}

struct CausalityDetector {
    target_time_ptrs: [*mut u64; parameter::CORE_COUNT],
    waiting_for_quantum_ptrs: [*mut u32; parameter::CORE_COUNT],
    quantum_generation_ptr: *mut u32,
    quantum_barrier_size: u64,
    records: ShardedCausalityRecords,
}

impl CausalityDetector {
    fn new() -> Self {
        CausalityDetector {
            target_time_ptrs: [std::ptr::null_mut(); parameter::CORE_COUNT],
            waiting_for_quantum_ptrs: [std::ptr::null_mut(); parameter::CORE_COUNT],
            quantum_generation_ptr: std::ptr::null_mut(),
            quantum_barrier_size: 0,
            records: ShardedCausalityRecords::new(),
        }
    }

    fn record_memory_access(&self, core_id: u32, pa: u64, is_write: bool) {
        let cache_line_id = pa >> parameter::CACHE_LINE_SIZE.trailing_zeros();
        let target_time = unsafe { self.target_time_ptrs[core_id as usize].read_volatile() };

        if is_write {
            self.records
                .process_write(cache_line_id, core_id, target_time);
        } else {
            self.records
                .process_read(cache_line_id, core_id, target_time);
        }
    }

    fn record_interrupt(&self, receiver_core: u32, source_time: u64, is_ipi: bool) {
        let receiver_time =
            unsafe { self.target_time_ptrs[receiver_core as usize].read_volatile() };

        let mut interrupt_violation = 0u64;
        let mut delayed_violation = 0u64;

        // note that, 100ns is the latency of delivering interrupts.

        if (source_time + 100) < receiver_time && is_ipi {
            // only apply to IPI for this type of interrupt.
            interrupt_violation += 1;
        }

        let waiting =
            unsafe { self.waiting_for_quantum_ptrs[receiver_core as usize].read_volatile() } != 0;

        let current_quantum_generation =
            unsafe { self.quantum_generation_ptr.read_volatile() } as u64;

        let next_quantum_time = (current_quantum_generation + 1) * self.quantum_barrier_size;

        let deliver_time = if is_ipi {
            source_time + 100
        } else {
            source_time
        };

        if waiting {
            if deliver_time >= receiver_time && deliver_time < (next_quantum_time) {
                // well, you should handle it now, but you are waiting, so you are doomed.
                delayed_violation += 1;
            }
        }

        Statistics::global_record_by(
            receiver_core,
            EventType::InterruptCausalityViolation,
            false,
            interrupt_violation,
        );
        Statistics::global_record_by(
            receiver_core,
            EventType::InterruptDelayedWithCausality,
            false,
            delayed_violation,
        );
    }
}

static mut DETECTOR: *const CausalityDetector = std::ptr::null();

unsafe extern "C" fn vcpu_mem_access_causality(
    vcpu_idx: u32,
    info: qemu_api::qemu_plugin_meminfo_t,
    vaddr: u64,
    _userdata: *mut ffi::c_void,
) {
    if (vcpu_idx as usize) >= parameter::SIMULATED_CORE_COUNT {
        return;
    }
    unsafe {
        let hw_handler = qemu_api::qemu_plugin_get_hwaddr(info, vaddr);
        let is_device = qemu_api::qemu_plugin_hwaddr_is_io(hw_handler);

        if !is_device {
            let is_store = qemu_api::qemu_plugin_mem_is_store(info);
            let pa = qemu_api::qemu_plugin_hwaddr_phys_addr(hw_handler);
            (*DETECTOR).record_memory_access(vcpu_idx, pa, is_store);
        }
    }
}

unsafe extern "C" fn vcpu_interrupt_delivered_causality(
    vcpu_idx: u32,
    src_time: u64,
    is_ipi: bool,
) {
    unsafe {
        (*DETECTOR).record_interrupt(vcpu_idx, src_time, is_ipi);
    }
}

pub struct CausalityDetectorPlugin {}

impl super::super::Plugin for CausalityDetectorPlugin {
    fn init(_plugin_id: u64, _options: &rustc_hash::FxHashMap<String, String>) {
        unsafe {
            DETECTOR = Box::into_raw(Box::new(CausalityDetector::new()));

            let detector = &mut *(DETECTOR as *mut CausalityDetector);

            detector.quantum_barrier_size = qemu_api::qemu_plugin_get_quantum_size();
            detector.quantum_generation_ptr =
                qemu_api::qemu_plugin_get_global_quantum_generation_ptr();

            DETECTOR = detector as *const CausalityDetector;

            qemu_api::qemu_plugin_register_on_deliver_interrupt_with_time_cb(Some(
                vcpu_interrupt_delivered_causality,
            ));
        }
    }

    unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
        unsafe {
            let detector = &mut *(DETECTOR as *mut CausalityDetector);

            if detector.quantum_barrier_size == 0 {
                return;
            }

            let n_instruction = qemu_api::qemu_plugin_tb_n_insns(tb);

            if n_instruction == 0 {
                return;
            }

            for i in 0..n_instruction {
                let inst = qemu_api::qemu_plugin_tb_get_insn(tb, i);

                qemu_api::qemu_plugin_register_vcpu_mem_cb(
                    inst,
                    Some(vcpu_mem_access_causality),
                    qemu_api::qemu_plugin_cb_flags_QEMU_PLUGIN_CB_NO_REGS,
                    qemu_api::qemu_plugin_mem_rw_QEMU_PLUGIN_MEM_RW,
                    std::ptr::null_mut(),
                );
            }
        }
    }

    fn serialize(_name: &str) {}
    fn deserialize(_name: &str) {}

    fn serialize_par(name: &str) {
        unsafe {
            if (*DETECTOR).quantum_barrier_size == 0 {
                return;
            }
        }

        use std::io::Write;

        unsafe {
            (*DETECTOR).records.clear_all();
        }

        let mut total_shared_mem: u64 = 0;
        let mut total_interrupt: u64 = 0;
        let mut total_delayed: u64 = 0;

        for core_id in 0..parameter::SIMULATED_CORE_COUNT {
            let (sm, _, _) = Statistics::global_query_record(
                core_id as u32,
                EventType::SharedMemoryCausalityViolation,
            );
            let (iv, _, _) = Statistics::global_query_record(
                core_id as u32,
                EventType::InterruptCausalityViolation,
            );
            let (dv, _, _) = Statistics::global_query_record(
                core_id as u32,
                EventType::InterruptDelayedWithCausality,
            );
            total_shared_mem += sm;
            total_interrupt += iv;
            total_delayed += dv;
        }

        let path = format!("{}/causality.txt", name);
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_fmt(format_args!(
            "SharedMemoryCausalityViolation: {}\n",
            total_shared_mem
        ))
        .unwrap();
        file.write_fmt(format_args!(
            "InterruptCausalityViolation: {}\n",
            total_interrupt
        ))
        .unwrap();
        file.write_fmt(format_args!(
            "InterruptDelayedWithCausality: {}\n",
            total_delayed
        ))
        .unwrap();
    }

    fn deserialize_par(_name: &str) {
        unsafe {
            if (*DETECTOR).quantum_barrier_size == 0 {
                return;
            }
        }

        unsafe {
            // This part is the moment when all CPUState have been created, so we can actually update the timer ptr
            for core_id in 0..parameter::CORE_COUNT {
                let core_id_u32 = core_id as u32;

                let detector = &mut *(DETECTOR as *mut CausalityDetector);
                detector.target_time_ptrs[core_id] =
                    qemu_api::qemu_plugin_get_vcpu_target_time_ptr(core_id_u32);
                detector.waiting_for_quantum_ptrs[core_id] =
                    qemu_api::qemu_plugin_get_vcpu_waiting_for_quantum_ptr(core_id_u32);
                detector.quantum_generation_ptr =
                    qemu_api::qemu_plugin_get_global_quantum_generation_ptr();

                assert!(!detector.target_time_ptrs[core_id].is_null());
                assert!(!detector.waiting_for_quantum_ptrs[core_id].is_null());
            }
        }
    }
}
