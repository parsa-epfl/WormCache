pub mod cache_hierarchy;
mod frontend;
pub mod helpers;
mod mmu;
mod sms;

pub use cache_hierarchy::process_cache_hierarchy;
pub use frontend::process_frontend;
pub use mmu::process_mmus;
use serde::{Deserialize, Serialize};
pub use sms::process_sms;

/// Extract the numeric suffix from a checkpoint file name for natural sorting.
///
/// File names have the form `prefix-N.ext` (single) or `prefix-kind-N.ext` (parallel).
/// We extract the last numeric segment before the extension.
/// E.g. `harvard-0-worker-10.rkyv.zstd` → 10, `fetch-worker-0.rkyv.zstd` → 0.
pub(crate) fn parse_checkpoint_index(name: &str) -> u32 {
    let stem = name
        .trim_end_matches(".rkyv.zstd")
        .trim_end_matches(".json.zstd");
    stem.rsplit('-')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Detect checkpoint files matching `pattern` in a folder.
/// Returns `(is_rkyv, sorted_filenames)` — prefers rkyv, falls back to JSON.
/// Single-file format returns 1 file, parallel format returns N files sorted by index.
pub(crate) fn detect_checkpoint_files(
    checkpoint_folder: &str,
    pattern: &str,
) -> (bool, Vec<String>) {
    let mut files: Vec<String> = std::fs::read_dir(checkpoint_folder)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let file_name = entry.file_name().into_string().unwrap();
            if file_name.contains(pattern) && file_name.ends_with(".rkyv.zstd") {
                Some(file_name)
            } else {
                None
            }
        })
        .collect();

    if !files.is_empty() {
        files.sort_by_key(|n| parse_checkpoint_index(n));
        return (true, files);
    }

    files = std::fs::read_dir(checkpoint_folder)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let file_name = entry.file_name().into_string().unwrap();
            if file_name.contains(pattern) && file_name.ends_with(".json.zstd") {
                Some(file_name)
            } else {
                None
            }
        })
        .collect();

    files.sort_by_key(|n| parse_checkpoint_index(n));
    (false, files)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FlexusDirectoryType {
    Infinite,
    Standard { sets: usize, associativity: usize },
}

#[derive(Clone, Serialize, Deserialize)]
pub enum FlexusSTLBInclusion {
    Inclusive,
    Exclusive,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FlexusParameter {
    pub l1i_sets: usize,
    pub l1i_associativity: usize,

    pub l1d_sets: usize,
    pub l1d_associativity: usize,

    pub l2_sets: usize,
    pub l2_associativity: usize,
    pub l2_slice_count: usize,

    pub itlb_sets: usize,
    pub itlb_associativity: usize,

    pub dtlb_sets: usize,
    pub dtlb_associativity: usize,

    pub stlb_sets: usize,
    pub stlb_associativity: usize,
    pub stlb_inclusion: FlexusSTLBInclusion,

    pub directory: FlexusDirectoryType,
    pub directory_slice_count: usize,

    pub btb_sets: usize,
    pub btb_associativity: usize,

    pub pht_sets: usize,
    pub pht_associativity: usize,

    #[serde(skip)]
    pub no_resizing: bool,
}
