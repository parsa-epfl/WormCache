// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! Tests for checkpoint helper serialization/deserialization.
//!
//! Each test follows the pattern:
//! 1. Create a runtime type
//! 2. Perform some updates
//! 3. Serialize to helper -> JSON/rkyv bytes
//! 4. Deserialize back
//! 5. Verify state matches

// ============================================================================
// BTB Helper Tests
// ============================================================================

mod btb_tests {
    use crate::checkpoint::helpers::BTBHelper;
    use crate::components::bp::fetch::btb::BTB;
    use crate::components::bp::{BranchResolutionResult, BranchType};

    fn create_and_modify_btb() -> BTB<16, 4> {
        let mut btb = BTB::<16, 4>::new();

        // Train the BTB with some branch instructions
        btb.train(
            0x1000,
            BranchResolutionResult {
                is_taken: true,
                branch_type: BranchType::Conditional,
            },
            0x2000,
        );
        btb.train(
            0x1004,
            BranchResolutionResult {
                is_taken: true,
                branch_type: BranchType::DirectCall,
            },
            0x3000,
        );
        btb.train(
            0x1008,
            BranchResolutionResult {
                is_taken: true,
                branch_type: BranchType::Return,
            },
            0x4000,
        );

        btb
    }

    #[test]
    fn test_btb_helper_json_roundtrip() {
        let btb = create_and_modify_btb();
        let helper = btb.to_checkpoint_helper();

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: BTBHelper = serde_json::from_str(&json).unwrap();

        // Reconstruct BTB
        let btb2 = BTB::<16, 4>::from_checkpoint_helper(helper2);

        // Verify by comparing helpers (since BTB doesn't impl PartialEq)
        let helper3 = btb2.to_checkpoint_helper();
        assert_eq!(helper.local_ts, helper3.local_ts);
        assert_eq!(helper.array.len(), helper3.array.len());
        for (set1, set2) in helper.array.iter().zip(helper3.array.iter()) {
            assert_eq!(set1.len(), set2.len());
            for (entry1, entry2) in set1.iter().zip(set2.iter()) {
                assert_eq!(entry1.tag, entry2.tag);
                assert_eq!(entry1.target, entry2.target);
                assert_eq!(entry1.ts, entry2.ts);
                assert_eq!(entry1.branch_type, entry2.branch_type);
            }
        }
    }

    #[test]
    fn test_btb_helper_rkyv_roundtrip() {
        let btb = create_and_modify_btb();
        let helper = btb.to_checkpoint_helper();

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: BTBHelper =
            rkyv::from_bytes::<BTBHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Reconstruct BTB
        let btb2 = BTB::<16, 4>::from_checkpoint_helper(helper2);

        // Verify
        let helper3 = btb2.to_checkpoint_helper();
        assert_eq!(helper.local_ts, helper3.local_ts);
        assert_eq!(helper.array.len(), helper3.array.len());
    }
}

// ============================================================================
// RAS Helper Tests
// ============================================================================

mod ras_tests {
    use crate::checkpoint::helpers::RASHelper;

    #[test]
    fn test_ras_helper_json_roundtrip() {
        let helper = RASHelper {
            stack: vec![0x1000, 0x2000, 0x3000, 0x4000],
        };

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: RASHelper = serde_json::from_str(&json).unwrap();

        // Verify
        assert_eq!(helper.stack, helper2.stack);
    }

    #[test]
    fn test_ras_helper_rkyv_roundtrip() {
        let helper = RASHelper {
            stack: vec![0x1000, 0x2000, 0x3000, 0x4000],
        };

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: RASHelper =
            rkyv::from_bytes::<RASHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Verify
        assert_eq!(helper.stack, helper2.stack);
    }
}

// ============================================================================
// TAGE Helper Tests
// ============================================================================

mod tage_tests {
    use crate::checkpoint::helpers::TAGEHelper;
    use crate::components::bp::fetch::tage::TAGEPredictor;
    use crate::components::bp::{BranchResolutionResult, BranchType};

    fn create_and_modify_tage() -> TAGEPredictor {
        let mut tage = TAGEPredictor::new();

        // Train with some branches
        for i in 0..10 {
            tage.train(
                0x1000 + i * 4,
                BranchResolutionResult {
                    is_taken: i % 2 == 0,
                    branch_type: BranchType::Conditional,
                },
                0x2000 + i * 4,
            );
        }

        tage
    }

    #[test]
    fn test_tage_helper_json_roundtrip() {
        let tage = create_and_modify_tage();
        let helper = tage.to_checkpoint_helper();

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: TAGEHelper = serde_json::from_str(&json).unwrap();

        // Reconstruct TAGE
        let tage2 = TAGEPredictor::from_checkpoint_helper(helper2);

        // Verify key fields
        let helper3 = tage2.to_checkpoint_helper();
        assert_eq!(helper.tick, helper3.tick);
        assert_eq!(helper.phist, helper3.phist);
        assert_eq!(helper.seed, helper3.seed);
        assert_eq!(helper.ghist, helper3.ghist);
        assert_eq!(helper.btable.len(), helper3.btable.len());
        assert_eq!(helper.gtable.len(), helper3.gtable.len());
    }

    #[test]
    fn test_tage_helper_rkyv_roundtrip() {
        let tage = create_and_modify_tage();
        let helper = tage.to_checkpoint_helper();

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: TAGEHelper =
            rkyv::from_bytes::<TAGEHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Reconstruct and verify
        let tage2 = TAGEPredictor::from_checkpoint_helper(helper2);
        let helper3 = tage2.to_checkpoint_helper();
        assert_eq!(helper.tick, helper3.tick);
        assert_eq!(helper.phist, helper3.phist);
        assert_eq!(helper.seed, helper3.seed);
    }
}

// ============================================================================
// Directory Helper Tests
// ============================================================================

mod directory_tests {
    use crate::checkpoint::helpers::{DirectoryHelper, DirectorySetHelper};
    use crate::components::cache_hierarchy::common::DirectoryEntry;
    use crate::components::cache_hierarchy::common::SharerList;
    use rustc_hash::FxHashMap as HashMap;

    fn create_directory_entries() -> Vec<HashMap<u64, DirectoryEntry>> {
        let mut sets = Vec::new();

        for set_idx in 0..4 {
            let mut entries = HashMap::default();

            for i in 0..3 {
                let block_id = (set_idx * 100 + i) as u64;
                let mut sharers = SharerList::ZERO;
                sharers.set(i, true);
                if i > 0 {
                    sharers.set(i - 1, true);
                }

                entries.insert(
                    block_id,
                    DirectoryEntry {
                        lru_ts: (set_idx * 10 + i) as u64,
                        sharers,
                        in_shared_cache: i % 2 == 0,
                        shared: i > 0,
                    },
                );
            }

            sets.push(entries);
        }

        sets
    }

    #[test]
    fn test_directory_helper_json_roundtrip() {
        let sets = create_directory_entries();
        let helper = DirectoryHelper::from_sets(&sets);

        assert_eq!(helper.sets.len(), 4);

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: DirectoryHelper = serde_json::from_str(&json).unwrap();

        // Convert back to sets
        let sets2 = helper2.into_sets();

        // Verify
        assert_eq!(sets.len(), sets2.len());
        for (original, restored) in sets.iter().zip(sets2.iter()) {
            assert_eq!(original.len(), restored.len());
            for (block_id, entry) in original.iter() {
                let restored_entry = restored.get(block_id).unwrap();
                assert_eq!(entry.lru_ts, restored_entry.lru_ts);
                assert_eq!(entry.in_shared_cache, restored_entry.in_shared_cache);
                assert_eq!(entry.shared, restored_entry.shared);
                assert_eq!(entry.sharers, restored_entry.sharers);
            }
        }
    }

    #[test]
    fn test_directory_helper_rkyv_roundtrip() {
        let sets = create_directory_entries();
        let helper = DirectoryHelper::from_sets(&sets);

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: DirectoryHelper =
            rkyv::from_bytes::<DirectoryHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Convert back and verify
        let sets2 = helper2.into_sets();
        assert_eq!(sets.len(), sets2.len());
        for (original, restored) in sets.iter().zip(sets2.iter()) {
            assert_eq!(original.len(), restored.len());
        }
    }

    #[test]
    fn test_directory_set_helper_roundtrip() {
        let mut entries = HashMap::default();
        entries.insert(
            100,
            DirectoryEntry {
                lru_ts: 50,
                sharers: SharerList::ZERO,
                in_shared_cache: true,
                shared: false,
            },
        );

        let helper = DirectorySetHelper::from_hashmap(&entries);
        let restored = helper.into_hashmap();

        assert_eq!(entries.len(), restored.len());
        assert!(restored.contains_key(&100));
    }
}

// ============================================================================
// Shared Cache Helper Tests
// ============================================================================

mod shared_cache_tests {
    use crate::checkpoint::helpers::{SharedCacheHelper, SharedCacheSetHelper};
    use crate::components::cache_hierarchy::common::{
        CacheAccessType, SharedCacheAccessRequest, SharedCacheAccessSource,
        SharedCacheSet,
    };
    use crate::components::cache_hierarchy::common::statistics::ZeroSharedCacheSetStatistics;

    type TestSet = SharedCacheSet<8, 256, false, ZeroSharedCacheSetStatistics>;

    fn create_and_modify_shared_cache_set() -> TestSet {
        let mut set = TestSet::new();

        // Insert some blocks
        for i in 0..4 {
            let block_id = (i * 256) as u64; // Same set, different tags
            let request = SharedCacheAccessRequest {
                block_id,
                is_os: false,
                source: SharedCacheAccessSource::Core(i as u32),
                access_type: CacheAccessType::DataRead,
            };
            set.lookup_and_insert(&request, (i + 1) as u64, true);
        }

        set
    }

    #[test]
    fn test_shared_cache_set_helper_json_roundtrip() {
        let set = create_and_modify_shared_cache_set();
        let helper = SharedCacheSetHelper::from(&set);

        assert_eq!(helper.blocks.len(), 8);
        assert_eq!(helper.touched_count, 4);

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: SharedCacheSetHelper = serde_json::from_str(&json).unwrap();

        // Reconstruct set
        let set2: TestSet = helper2.into_set();

        // Verify
        let helper3 = SharedCacheSetHelper::from(&set2);
        assert_eq!(helper.touched_count, helper3.touched_count);
        assert_eq!(helper.recent_evict_ts, helper3.recent_evict_ts);
        assert_eq!(helper.access_count, helper3.access_count);
        assert_eq!(helper.blocks.len(), helper3.blocks.len());

        // Verify individual blocks
        for (b1, b2) in helper.blocks.iter().zip(helper3.blocks.iter()) {
            assert_eq!(b1.block_id_with_v, b2.block_id_with_v);
            assert_eq!(b1.ts, b2.ts);
            assert_eq!(b1.modified, b2.modified);
        }
    }

    #[test]
    fn test_shared_cache_set_helper_rkyv_roundtrip() {
        let set = create_and_modify_shared_cache_set();
        let helper = SharedCacheSetHelper::from(&set);

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: SharedCacheSetHelper =
            rkyv::from_bytes::<SharedCacheSetHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Reconstruct and verify
        let set2: TestSet = helper2.into_set();
        let helper3 = SharedCacheSetHelper::from(&set2);
        assert_eq!(helper.touched_count, helper3.touched_count);
        assert_eq!(helper.blocks.len(), helper3.blocks.len());
    }

    #[test]
    fn test_shared_cache_helper_json_roundtrip() {
        let sets: Vec<TestSet> = (0..4).map(|_| create_and_modify_shared_cache_set()).collect();

        let helper = SharedCacheHelper::from_sets(sets.iter(), 4);

        assert_eq!(helper.blocks.len(), 4);
        assert_eq!(helper.warmed_sets, 4);

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: SharedCacheHelper = serde_json::from_str(&json).unwrap();

        // Verify
        assert_eq!(helper.blocks.len(), helper2.blocks.len());
        assert_eq!(helper.warmed_sets, helper2.warmed_sets);
    }

    #[test]
    fn test_shared_cache_helper_rkyv_roundtrip() {
        let sets: Vec<TestSet> = (0..4).map(|_| create_and_modify_shared_cache_set()).collect();

        let helper = SharedCacheHelper::from_sets(sets.iter(), 4);

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: SharedCacheHelper =
            rkyv::from_bytes::<SharedCacheHelper, rkyv::rancor::Error>(&bytes).unwrap();

        assert_eq!(helper.blocks.len(), helper2.blocks.len());
        assert_eq!(helper.warmed_sets, helper2.warmed_sets);
    }
}

// ============================================================================
// Private Cache Helper Tests
// ============================================================================

mod private_cache_tests {
    use crate::checkpoint::helpers::{HarvardPrivateCacheHelper, UnifiedPrivateCacheHelper};
    use crate::components::cache_hierarchy::common::PrivateCacheSet;

    fn create_private_cache_set(asso: usize) -> PrivateCacheSet {
        let mut set = PrivateCacheSet::new(asso);

        // Manually set some lines to simulate cache state
        // The private cache set has a lines Vec that we can modify
        for i in 0..std::cmp::min(3, set.lines.len()) {
            set.lines[i].block_id_with_v = ((i as u64 * 1000) << 1) | 1; // valid bit set
            set.lines[i].ts = (i + 1) as u64;
            set.lines[i].is_instruction = false;
            set.lines[i].writeable = true;
            set.lines[i].modified = i % 2 == 0;
        }
        set.touched_count = 3;

        set
    }

    #[test]
    fn test_unified_private_cache_helper_json_roundtrip() {
        let sets: Vec<PrivateCacheSet> = (0..4).map(|_| create_private_cache_set(8)).collect();

        let helper = UnifiedPrivateCacheHelper {
            cache: sets.clone(),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: UnifiedPrivateCacheHelper = serde_json::from_str(&json).unwrap();

        // Verify
        assert_eq!(helper.cache.len(), helper2.cache.len());
        for (s1, s2) in helper.cache.iter().zip(helper2.cache.iter()) {
            assert_eq!(s1.lines.len(), s2.lines.len());
            for (l1, l2) in s1.lines.iter().zip(s2.lines.iter()) {
                assert_eq!(l1.block_id_with_v, l2.block_id_with_v);
                assert_eq!(l1.ts, l2.ts);
            }
        }
    }

    #[test]
    fn test_unified_private_cache_helper_rkyv_roundtrip() {
        let sets: Vec<PrivateCacheSet> = (0..4).map(|_| create_private_cache_set(8)).collect();

        let helper = UnifiedPrivateCacheHelper {
            cache: sets.clone(),
        };

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: UnifiedPrivateCacheHelper =
            rkyv::from_bytes::<UnifiedPrivateCacheHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Verify
        assert_eq!(helper.cache.len(), helper2.cache.len());
    }

    #[test]
    fn test_harvard_private_cache_helper_json_roundtrip() {
        let i_cache: Vec<PrivateCacheSet> = (0..4).map(|_| create_private_cache_set(4)).collect();
        let d_cache: Vec<PrivateCacheSet> = (0..4).map(|_| create_private_cache_set(8)).collect();

        let helper = HarvardPrivateCacheHelper {
            i_cache: i_cache.clone(),
            d_cache: d_cache.clone(),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: HarvardPrivateCacheHelper = serde_json::from_str(&json).unwrap();

        // Verify
        assert_eq!(helper.i_cache.len(), helper2.i_cache.len());
        assert_eq!(helper.d_cache.len(), helper2.d_cache.len());
    }

    #[test]
    fn test_harvard_private_cache_helper_rkyv_roundtrip() {
        let i_cache: Vec<PrivateCacheSet> = (0..4).map(|_| create_private_cache_set(4)).collect();
        let d_cache: Vec<PrivateCacheSet> = (0..4).map(|_| create_private_cache_set(8)).collect();

        let helper = HarvardPrivateCacheHelper {
            i_cache: i_cache.clone(),
            d_cache: d_cache.clone(),
        };

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: HarvardPrivateCacheHelper =
            rkyv::from_bytes::<HarvardPrivateCacheHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Verify
        assert_eq!(helper.i_cache.len(), helper2.i_cache.len());
        assert_eq!(helper.d_cache.len(), helper2.d_cache.len());
    }
}

// ============================================================================
// FetchUnit Helper Tests
// ============================================================================

mod fetch_unit_tests {
    use crate::checkpoint::helpers::{
        BTBHelper, FetchUnitHelper, PerCoreFetchUnitHelper, RASHelper, TAGEHelper,
    };

    fn create_per_core_fetch_unit_helper() -> PerCoreFetchUnitHelper {
        PerCoreFetchUnitHelper {
            btb: BTBHelper {
                array: vec![vec![]; 16],
                local_ts: 100,
            },
            ras: RASHelper {
                stack: vec![0x1000, 0x2000, 0x3000],
            },
            tage: TAGEHelper {
                tick: 5,
                phist: 10,
                ghist: vec![true, false, true, false],
                ch_i: vec![],
                ch_t: vec![],
                btable: vec![],
                gtable: vec![],
                seed: 42,
            },
        }
    }

    #[test]
    fn test_per_core_fetch_unit_helper_json_roundtrip() {
        let helper = create_per_core_fetch_unit_helper();

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: PerCoreFetchUnitHelper = serde_json::from_str(&json).unwrap();

        // Verify
        assert_eq!(helper.btb.local_ts, helper2.btb.local_ts);
        assert_eq!(helper.ras.stack, helper2.ras.stack);
        assert_eq!(helper.tage.tick, helper2.tage.tick);
        assert_eq!(helper.tage.phist, helper2.tage.phist);
        assert_eq!(helper.tage.seed, helper2.tage.seed);
        assert_eq!(helper.tage.ghist, helper2.tage.ghist);
    }

    #[test]
    fn test_per_core_fetch_unit_helper_rkyv_roundtrip() {
        let helper = create_per_core_fetch_unit_helper();

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: PerCoreFetchUnitHelper =
            rkyv::from_bytes::<PerCoreFetchUnitHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Verify
        assert_eq!(helper.btb.local_ts, helper2.btb.local_ts);
        assert_eq!(helper.ras.stack, helper2.ras.stack);
        assert_eq!(helper.tage.seed, helper2.tage.seed);
    }

    #[test]
    fn test_fetch_unit_helper_json_roundtrip() {
        let helper = FetchUnitHelper {
            private_units: vec![
                create_per_core_fetch_unit_helper(),
                create_per_core_fetch_unit_helper(),
                create_per_core_fetch_unit_helper(),
                create_per_core_fetch_unit_helper(),
            ],
        };

        // Serialize to JSON
        let json = serde_json::to_string(&helper).unwrap();

        // Deserialize from JSON
        let helper2: FetchUnitHelper = serde_json::from_str(&json).unwrap();

        // Verify
        assert_eq!(helper.private_units.len(), helper2.private_units.len());
        for (u1, u2) in helper.private_units.iter().zip(helper2.private_units.iter()) {
            assert_eq!(u1.btb.local_ts, u2.btb.local_ts);
            assert_eq!(u1.ras.stack, u2.ras.stack);
            assert_eq!(u1.tage.seed, u2.tage.seed);
        }
    }

    #[test]
    fn test_fetch_unit_helper_rkyv_roundtrip() {
        let helper = FetchUnitHelper {
            private_units: vec![
                create_per_core_fetch_unit_helper(),
                create_per_core_fetch_unit_helper(),
            ],
        };

        // Serialize to rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();

        // Deserialize from rkyv
        let helper2: FetchUnitHelper =
            rkyv::from_bytes::<FetchUnitHelper, rkyv::rancor::Error>(&bytes).unwrap();

        // Verify
        assert_eq!(helper.private_units.len(), helper2.private_units.len());
    }
}
