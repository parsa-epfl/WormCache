// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! Private cache checkpoint helpers.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

use crate::components::cache_hierarchy::common::PrivateCacheSet;

/// Helper for serializing a unified private cache (per-core).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct UnifiedPrivateCacheHelper {
    pub cache: Vec<PrivateCacheSet>,
}

/// Helper for serializing a Harvard private cache (per-core).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct HarvardPrivateCacheHelper {
    pub i_cache: Vec<PrivateCacheSet>,
    pub d_cache: Vec<PrivateCacheSet>,
}

#[cfg(test)]
mod tests {
    use super::{HarvardPrivateCacheHelper, UnifiedPrivateCacheHelper};
    use crate::components::cache_hierarchy::common::PrivateCacheSet;

    fn instruction_set_with_both_origins() -> PrivateCacheSet {
        let mut set = PrivateCacheSet::new(2);
        for (index, line) in set.lines.iter_mut().enumerate() {
            line.block_id_with_v = (((index + 10) as u64) << 1) | 1;
            line.ts = (index + 1) as u64;
            line.is_instruction = true;
            line.prefetched = index == 0;
        }
        set.touched_count = 2;
        set
    }

    #[test]
    fn unified_origin_survives_json_and_rkyv_roundtrip() {
        let helper = UnifiedPrivateCacheHelper {
            cache: vec![instruction_set_with_both_origins()],
        };

        let json = serde_json::to_vec(&helper).unwrap();
        let from_json: UnifiedPrivateCacheHelper = serde_json::from_slice(&json).unwrap();
        assert_eq!(helper, from_json);

        let archive = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
        let from_archive =
            rkyv::from_bytes::<UnifiedPrivateCacheHelper, rkyv::rancor::Error>(&archive).unwrap();
        assert_eq!(helper, from_archive);
    }

    #[test]
    fn harvard_origin_survives_json_and_rkyv_roundtrip() {
        let helper = HarvardPrivateCacheHelper {
            i_cache: vec![instruction_set_with_both_origins()],
            d_cache: vec![PrivateCacheSet::new(2)],
        };

        let json = serde_json::to_vec(&helper).unwrap();
        let from_json: HarvardPrivateCacheHelper = serde_json::from_slice(&json).unwrap();
        assert_eq!(helper, from_json);

        let archive = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
        let from_archive =
            rkyv::from_bytes::<HarvardPrivateCacheHelper, rkyv::rancor::Error>(&archive).unwrap();
        assert_eq!(helper, from_archive);
    }

    #[test]
    fn legacy_json_defaults_prefetched_to_false() {
        let legacy_set = serde_json::json!({
            "lines": [{
                "block_id_with_v": 21,
                "ts": 1,
                "is_instruction": true,
                "writeable": false,
                "modified": false
            }],
            "touched_count": 1,
            "recent_invalid_slot_index": null,
            "hit_time": 0,
            "hit_index_acc": 0
        });

        let unified: UnifiedPrivateCacheHelper = serde_json::from_value(serde_json::json!({
            "cache": [legacy_set.clone()]
        }))
        .unwrap();
        let line = &unified.cache[0].lines[0];
        assert!(line.is_valid());
        assert!(line.is_instruction);
        assert!(!line.prefetched);

        let harvard: HarvardPrivateCacheHelper = serde_json::from_value(serde_json::json!({
            "i_cache": [legacy_set],
            "d_cache": []
        }))
        .unwrap();
        let line = &harvard.i_cache[0].lines[0];
        assert!(line.is_valid());
        assert!(line.is_instruction);
        assert!(!line.prefetched);
    }
}
