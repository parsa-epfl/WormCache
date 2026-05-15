use super::FlexusParameter;
pub use crate::checkpoint::helpers::{
    HarvardPrivateCacheHelper, SharedCacheHelper, UnifiedPrivateCacheHelper,
};
use crate::components::cache_hierarchy::common::SharedCacheAccessSource;

mod private_cache;
mod shared_cache;

pub fn process_cache_hierarchy(
    checkpoint_folder: &String,
    flexus_configuration: &FlexusParameter,
    output_folder: &String,
) {
    let (harvard_is_rkyv, harvard_cache_checkpoints) =
        super::detect_checkpoint_files(checkpoint_folder, "harvard");
    let (unified_is_rkyv, unified_cache_checkpoints) =
        super::detect_checkpoint_files(checkpoint_folder, "unified");

    let is_harvard_cache = !harvard_cache_checkpoints.is_empty();
    let is_unified_cache = !unified_cache_checkpoints.is_empty();

    assert!((is_harvard_cache || is_unified_cache) && !(is_harvard_cache && is_unified_cache));

    let private_cache = if is_harvard_cache {
        if harvard_cache_checkpoints.len() > 1 {
            println!(
                "Harvard cache is detected (parallel, {} workers, {}).",
                harvard_cache_checkpoints.len(),
                if harvard_is_rkyv { "rkyv" } else { "JSON" }
            );
            let mut cache: Vec<HarvardPrivateCacheHelper> = Vec::new();
            for file_name in &harvard_cache_checkpoints {
                let path = format!("{}/{}", checkpoint_folder, file_name);
                let bytes = crate::util::read_compressed(&path);
                let worker: Vec<HarvardPrivateCacheHelper> = if harvard_is_rkyv {
                    rkyv::from_bytes::<Vec<HarvardPrivateCacheHelper>, rkyv::rancor::Error>(&bytes)
                        .unwrap()
                } else {
                    serde_json::from_slice(&bytes).unwrap()
                };
                cache.extend(worker);
            }
            private_cache::FlexusPrivateCacheCheckpointHelper::from_harvard_caches(
                cache,
                flexus_configuration,
            )
        } else {
            assert!(harvard_cache_checkpoints.len() == 1);
            println!(
                "Harvard cache is detected ({}). Filename: {}",
                if harvard_is_rkyv { "rkyv" } else { "JSON" },
                harvard_cache_checkpoints[0]
            );
            let path = format!("{}/{}", checkpoint_folder, harvard_cache_checkpoints[0]);
            let bytes = crate::util::read_compressed(&path);
            let cache: Vec<HarvardPrivateCacheHelper> = if harvard_is_rkyv {
                rkyv::from_bytes::<Vec<HarvardPrivateCacheHelper>, rkyv::rancor::Error>(&bytes)
                    .unwrap()
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            private_cache::FlexusPrivateCacheCheckpointHelper::from_harvard_caches(
                cache,
                flexus_configuration,
            )
        }
    } else {
        if unified_cache_checkpoints.len() > 1 {
            println!(
                "Unified cache is detected (parallel, {} workers, {}).",
                unified_cache_checkpoints.len(),
                if unified_is_rkyv { "rkyv" } else { "JSON" }
            );
            let mut cache: Vec<UnifiedPrivateCacheHelper> = Vec::new();
            for file_name in &unified_cache_checkpoints {
                let path = format!("{}/{}", checkpoint_folder, file_name);
                let bytes = crate::util::read_compressed(&path);
                let worker: Vec<UnifiedPrivateCacheHelper> = if unified_is_rkyv {
                    rkyv::from_bytes::<Vec<UnifiedPrivateCacheHelper>, rkyv::rancor::Error>(&bytes)
                        .unwrap()
                } else {
                    serde_json::from_slice(&bytes).unwrap()
                };
                cache.extend(worker);
            }
            private_cache::FlexusPrivateCacheCheckpointHelper::from_unified_caches(
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
            let path = format!("{}/{}", checkpoint_folder, unified_cache_checkpoints[0]);
            let bytes = crate::util::read_compressed(&path);
            let cache: Vec<UnifiedPrivateCacheHelper> = if unified_is_rkyv {
                rkyv::from_bytes::<Vec<UnifiedPrivateCacheHelper>, rkyv::rancor::Error>(&bytes)
                    .unwrap()
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            private_cache::FlexusPrivateCacheCheckpointHelper::from_unified_caches(
                cache,
                flexus_configuration,
            )
        }
    };

    private_cache.export(output_folder.clone());

    let (llc_is_rkyv, shared_cache_checkpoints) =
        super::detect_checkpoint_files(checkpoint_folder, "llc");

    let mut shared_cache: SharedCacheHelper = if shared_cache_checkpoints.len() > 1 {
        println!(
            "Shared cache is detected (parallel, {} shards, {}).",
            shared_cache_checkpoints.len(),
            if llc_is_rkyv { "rkyv" } else { "JSON" }
        );
        let mut merged_blocks = Vec::new();
        let mut warmed_sets = 0;
        for file_name in &shared_cache_checkpoints {
            let path = format!("{}/{}", checkpoint_folder, file_name);
            let bytes = crate::util::read_compressed(&path);
            let helper: SharedCacheHelper = if llc_is_rkyv {
                rkyv::from_bytes::<SharedCacheHelper, rkyv::rancor::Error>(&bytes).unwrap()
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            merged_blocks.extend(helper.blocks);
            warmed_sets = helper.warmed_sets;
        }
        SharedCacheHelper {
            blocks: merged_blocks,
            warmed_sets,
        }
    } else {
        assert!(shared_cache_checkpoints.len() == 1);
        println!(
            "Shared cache is detected ({}). Filename: {}",
            if llc_is_rkyv { "rkyv" } else { "JSON" },
            shared_cache_checkpoints[0]
        );
        let path = format!("{}/{}", checkpoint_folder, shared_cache_checkpoints[0]);
        let bytes = crate::util::read_compressed(&path);
        if llc_is_rkyv {
            rkyv::from_bytes::<SharedCacheHelper, rkyv::rancor::Error>(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        }
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
