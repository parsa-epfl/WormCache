use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::util::get_monotonic_ts;

fn read_proc_peak_memory() -> (u64, u64) {
    let mut peak_virtual = 0u64;
    let mut peak_rss = 0u64;

    let content = match std::fs::read_to_string("/proc/self/status") {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };

    for line in content.lines() {
        if line.starts_with("VmPeak:") {
            peak_virtual = line
                .split_ascii_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
        } else if line.starts_with("VmHWM:") {
            peak_rss = line
                .split_ascii_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
        }
    }

    (peak_virtual, peak_rss)
}

static SIMULATION_START_TS: AtomicU64 = AtomicU64::new(0);

pub fn init_simulation_start() {
    SIMULATION_START_TS.store(get_monotonic_ts(), Ordering::Relaxed);
}

#[derive(serde::Serialize)]
struct LoadCheckpoint {
    total_ns: u64,
    memory_state_ns: u64,
    uarch_state_ns: u64,
}

#[derive(serde::Serialize)]
struct SaveCheckpoint {
    total_ns: u64,
    memory_state_ns: u64,
    uarch_state_ns: u64,
    dirty_snapshot_ns: u64,
    savevm_state_ns: u64,
    pre_work_ns: u64,
    bdrv_snapshot_ns: u64,
}

#[derive(serde::Serialize)]
struct TimingReport {
    load_checkpoint: LoadCheckpoint,
    save_checkpoint: SaveCheckpoint,
    simulation_ns: u64,
    total_wall_ns: u64,
    uffd_pages_loaded: u64,
    peak_virtual_kb: u64,
    peak_rss_kb: u64,
}

fn build_report() -> Option<TimingReport> {
    let now = get_monotonic_ts();
    let sim_start = SIMULATION_START_TS.load(Ordering::Relaxed);

    let qemu_timing = unsafe {
        let ptr = crate::qemu_api::qemu_plugin_get_timing_info();
        if ptr.is_null() {
            println!("Warning: qemu_plugin_get_timing_info returned null.");
            return None;
        }
        &*ptr
    };

    let total_save_ns = qemu_timing.total_save_time_ns;
    let total_load_ns = qemu_timing.total_load_time_ns;
    let save_mem_ns = qemu_timing.save_memory_state_time_ns;
    let load_mem_ns = qemu_timing.load_memory_state_time_ns;
    let save_uarch_ns = qemu_timing.save_uarch_state_time_ns;
    let load_uarch_ns = qemu_timing.load_uarch_state_time_ns;
    let uffd_pages = qemu_timing.uffd_pages_loaded;
    let save_dirty_snap_ns = qemu_timing.save_dirty_snapshot_time_ns;
    let save_savevm_ns = qemu_timing.save_qemu_savevm_state_time_ns;
    let save_pre_work_ns = qemu_timing.save_pre_work_time_ns;
    let save_bdrv_snap_ns = qemu_timing.save_bdrv_snapshot_time_ns;

    let total_wall_ns = now.saturating_sub(sim_start);
    let simulation_ns = total_wall_ns
        .saturating_sub(total_save_ns)
        .saturating_sub(total_load_ns);

    let (peak_virtual_kb, peak_rss_kb) = read_proc_peak_memory();

    Some(TimingReport {
        load_checkpoint: LoadCheckpoint {
            total_ns: total_load_ns,
            memory_state_ns: load_mem_ns,
            uarch_state_ns: load_uarch_ns,
        },
        save_checkpoint: SaveCheckpoint {
            total_ns: total_save_ns,
            memory_state_ns: save_mem_ns,
            uarch_state_ns: save_uarch_ns,
            dirty_snapshot_ns: save_dirty_snap_ns,
            savevm_state_ns: save_savevm_ns,
            pre_work_ns: save_pre_work_ns,
            bdrv_snapshot_ns: save_bdrv_snap_ns,
        },
        simulation_ns,
        total_wall_ns,
        uffd_pages_loaded: uffd_pages,
        peak_virtual_kb,
        peak_rss_kb,
    })
}

fn write_json(report: &TimingReport, json_filename: &str) {
    let json_str = serde_json::to_string_pretty(report).unwrap();
    if let Ok(mut file) = std::fs::File::create(json_filename) {
        let _ = file.write_all(json_str.as_bytes());
        let _ = file.flush();
    } else {
        eprintln!(
            "Warning: Failed to create timing JSON file '{}'",
            json_filename
        );
    }
}

pub fn save_timing_json(json_filename: &str) {
    let report = match build_report() {
        Some(r) => r,
        None => return,
    };
    write_json(&report, json_filename);
}

pub fn print_time_breakdown(json_filename: &str) {
    let report = match build_report() {
        Some(r) => r,
        None => return,
    };

    println!("========== Host Time Breakdown ==========");
    println!(
        "  Loading checkpoints  | total {:>12} ns  mem {:>12} ns  uarch {:>12} ns",
        report.load_checkpoint.total_ns,
        report.load_checkpoint.memory_state_ns,
        report.load_checkpoint.uarch_state_ns
    );
    println!(
        "  Storing checkpoints  | total {:>12} ns  mem {:>12} ns  uarch {:>12} ns  dirty_snap {:>12} ns  savevm {:>12} ns  pre_work {:>12} ns  bdrv {:>12} ns",
        report.save_checkpoint.total_ns,
        report.save_checkpoint.memory_state_ns,
        report.save_checkpoint.uarch_state_ns,
        report.save_checkpoint.dirty_snapshot_ns,
        report.save_checkpoint.savevm_state_ns,
        report.save_checkpoint.pre_work_ns,
        report.save_checkpoint.bdrv_snapshot_ns,
    );
    println!("  Simulating           | {:>12} ns", report.simulation_ns);
    println!("  -------------------------------------------");
    println!("  Total wall time      | {:>12} ns", report.total_wall_ns);
    println!("  UFFD pages loaded    | {:>12}", report.uffd_pages_loaded);
    println!("  Peak virtual memory  | {:>12} kB", report.peak_virtual_kb);
    println!("  Peak RSS memory      | {:>12} kB", report.peak_rss_kb);
    println!("===========================================");

    write_json(&report, json_filename);
}
