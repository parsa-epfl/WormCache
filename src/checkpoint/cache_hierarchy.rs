use super::FlexusParameter;
pub use crate::checkpoint::helpers::{
    HarvardPrivateCacheHelper, SharedCacheHelper, UnifiedPrivateCacheHelper,
};
use serde_json;
use zstd::stream::read::Decoder;

use crate::components::cache_hierarchy::common::SharedCacheAccessSource;

mod private_cache;
mod shared_cache;

/// Detect whether to use rkyv or JSON format based on file existence.
fn detect_rkyv_format(checkpoint_folder: &str, pattern: &str) -> (bool, Vec<String>) {
    // First check for rkyv files
    let rkyv_files: Vec<String> = std::fs::read_dir(checkpoint_folder)
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

    if !rkyv_files.is_empty() {
        return (true, rkyv_files);
    }

    // Fall back to JSON files
    let json_files: Vec<String> = std::fs::read_dir(checkpoint_folder)
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

    (false, json_files)
}

pub fn process_cache_hierarchy(
    checkpoint_folder: &String,
    flexus_configuration: &FlexusParameter,
    output_folder: &String,
) {
    let (harvard_is_rkyv, harvard_cache_checkpoints) =
        detect_rkyv_format(checkpoint_folder, "harvard");
    let (unified_is_rkyv, unified_cache_checkpoints) =
        detect_rkyv_format(checkpoint_folder, "unified");

    let is_harvard_cache = !harvard_cache_checkpoints.is_empty();
    let is_unified_cache = !unified_cache_checkpoints.is_empty();

    assert!((is_harvard_cache || is_unified_cache) && !(is_harvard_cache && is_unified_cache));

    let private_cache = if is_harvard_cache {
        assert!(harvard_cache_checkpoints.len() == 1);
        println!(
            "Harvard cache is detected ({}). Filename: {}",
            if harvard_is_rkyv { "rkyv" } else { "JSON" },
            harvard_cache_checkpoints[0]
        );

        let file = std::fs::File::open(format!(
            "{}/{}",
            checkpoint_folder, harvard_cache_checkpoints[0]
        ))
        .unwrap();

        let cache: Vec<HarvardPrivateCacheHelper> = if harvard_is_rkyv {
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            rkyv::from_bytes::<Vec<HarvardPrivateCacheHelper>, rkyv::rancor::Error>(&bytes).unwrap()
        } else {
            let decoder = Decoder::new(file).unwrap();
            serde_json::from_reader(decoder).unwrap()
        };

        private_cache::FlexusPrivateCacheCheckpointHelper::from_harvard_caches(
            cache,
            flexus_configuration,
        )
    } else {
        assert!(unified_cache_checkpoints.len() == 1);
        println!(
            "Unified cache is detected ({}). Filename: {}",
            if unified_is_rkyv { "rkyv" } else { "JSON" },
            unified_cache_checkpoints[0]
        );

        let file = std::fs::File::open(format!(
            "{}/{}",
            checkpoint_folder, unified_cache_checkpoints[0]
        ))
        .unwrap();

        let cache: Vec<UnifiedPrivateCacheHelper> = if unified_is_rkyv {
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            rkyv::from_bytes::<Vec<UnifiedPrivateCacheHelper>, rkyv::rancor::Error>(&bytes).unwrap()
        } else {
            let decoder = Decoder::new(file).unwrap();
            serde_json::from_reader(decoder).unwrap()
        };

        private_cache::FlexusPrivateCacheCheckpointHelper::from_unified_caches(
            cache,
            flexus_configuration,
        )
    };

    private_cache.export(output_folder.clone());

    // Now, we need to process the shared cache.
    let (llc_is_rkyv, shared_cache_checkpoints) = detect_rkyv_format(checkpoint_folder, "llc");

    assert!(shared_cache_checkpoints.len() == 1);
    println!(
        "Shared cache is detected ({}). Filename: {}",
        if llc_is_rkyv { "rkyv" } else { "JSON" },
        shared_cache_checkpoints[0]
    );

    let file = std::fs::File::open(format!(
        "{}/{}",
        checkpoint_folder, shared_cache_checkpoints[0]
    ))
    .unwrap();

    let mut shared_cache: SharedCacheHelper = if llc_is_rkyv {
        let mut decoder = Decoder::new(file).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

        // The unified helper works for both JSON and rkyv directly
        rkyv::from_bytes::<SharedCacheHelper, rkyv::rancor::Error>(&bytes).unwrap()
    } else {
        let decoder = Decoder::new(file).unwrap();
        serde_json::from_reader(decoder).unwrap()
    };

    if flexus_configuration.no_resizing {
        assert!(private_cache.get_evicted_lines().is_empty());
    }

    for (_, (line, accessor)) in private_cache.get_evicted_lines().iter() {
        shared_cache.process_evicted_cache_line(line, SharedCacheAccessSource::Core(*accessor));
    }

    shared_cache.resize(flexus_configuration);

    shared_cache.export_slices(output_folder, flexus_configuration);
}
