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
    use crate::components::cache_hierarchy::common::statistics::ZeroSharedCacheSetStatistics;
    use crate::components::cache_hierarchy::common::{
        CacheAccessType, SharedCacheAccessRequest, SharedCacheAccessSource, SharedCacheSet,
    };

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
        let sets: Vec<TestSet> = (0..4)
            .map(|_| create_and_modify_shared_cache_set())
            .collect();

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
        let sets: Vec<TestSet> = (0..4)
            .map(|_| create_and_modify_shared_cache_set())
            .collect();

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
        for (u1, u2) in helper
            .private_units
            .iter()
            .zip(helper2.private_units.iter())
        {
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

// ============================================================================
// MMU Helper Tests
// ============================================================================

mod mmu_tests {
    use crate::checkpoint::helpers::{
        AddressSpaceID, FullyAssociativeL1MMUHelper, FullyAssociativeTLBEntry,
        FullyAssociativeTLBHelper, HugeTLBHelper, MMUHelper, MMUsHelper, NoMMUHelper,
        OrdinaryMMUHelper, TLBEntry, TLBHelper, TLBSetHelper,
    };

    fn create_tlb_entry(vpn: u64, ppn: u64, ts: u64, valid: bool) -> TLBEntry {
        TLBEntry {
            valid,
            ts,
            asid: AddressSpaceID::NonGlobal(42),
            vpn,
            ppn,
            is_instruction: false,
        }
    }

    fn create_tlb_helper() -> TLBHelper {
        TLBHelper {
            entries: vec![
                TLBSetHelper {
                    entries: vec![
                        create_tlb_entry(0x1000, 0x2000, 100, true),
                        create_tlb_entry(0x3000, 0x4000, 200, true),
                        create_tlb_entry(0, 0, 0, false),
                        create_tlb_entry(0, 0, 0, false),
                    ],
                    current_pointer: 2,
                },
                TLBSetHelper {
                    entries: vec![
                        create_tlb_entry(0x5000, 0x6000, 300, true),
                        create_tlb_entry(0, 0, 0, false),
                        create_tlb_entry(0, 0, 0, false),
                        create_tlb_entry(0, 0, 0, false),
                    ],
                    current_pointer: 1,
                },
            ],
        }
    }

    fn create_fully_associative_tlb_helper() -> FullyAssociativeTLBHelper {
        FullyAssociativeTLBHelper {
            elements: vec![
                (
                    0x12345678,
                    FullyAssociativeTLBEntry {
                        ts: 100,
                        ppn: 0x1000,
                    },
                ),
                (
                    0x87654321,
                    FullyAssociativeTLBEntry {
                        ts: 200,
                        ppn: 0x2000,
                    },
                ),
            ],
            associativity: 64,
            deferred_elements_exist: false,
        }
    }

    fn create_huge_tlb_helper() -> HugeTLBHelper {
        HugeTLBHelper {
            entries: vec![
                (0x100, (AddressSpaceID::Global, 0x200)),
                (0x300, (AddressSpaceID::NonGlobal(10), 0x400)),
            ],
        }
    }

    fn create_ordinary_mmu_helper() -> OrdinaryMMUHelper {
        OrdinaryMMUHelper {
            itlb: create_tlb_helper(),
            dtlb: create_tlb_helper(),
            stlb: create_tlb_helper(),
            htbl_2mb: create_huge_tlb_helper(),
            htlb_1gb: HugeTLBHelper { entries: vec![] },
        }
    }

    #[test]
    fn test_address_space_id_roundtrip() {
        // Test Global
        let asid = AddressSpaceID::Global;
        let json = serde_json::to_string(&asid).unwrap();
        let asid2: AddressSpaceID = serde_json::from_str(&json).unwrap();
        assert_eq!(asid, asid2);

        // Test NonGlobal
        let asid = AddressSpaceID::NonGlobal(1234);
        let json = serde_json::to_string(&asid).unwrap();
        let asid2: AddressSpaceID = serde_json::from_str(&json).unwrap();
        assert_eq!(asid, asid2);

        // Test rkyv
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&asid).unwrap();
        let asid3: AddressSpaceID =
            rkyv::from_bytes::<AddressSpaceID, rkyv::rancor::Error>(&bytes).unwrap();
        assert_eq!(asid, asid3);
    }

    #[test]
    fn test_tlb_entry_json_roundtrip() {
        let entry = create_tlb_entry(0x1000, 0x2000, 100, true);
        let json = serde_json::to_string(&entry).unwrap();
        let entry2: TLBEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(entry.valid, entry2.valid);
        assert_eq!(entry.ts, entry2.ts);
        assert_eq!(entry.vpn, entry2.vpn);
        assert_eq!(entry.ppn, entry2.ppn);
    }

    #[test]
    fn test_tlb_entry_rkyv_roundtrip() {
        let entry = create_tlb_entry(0x1000, 0x2000, 100, true);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&entry).unwrap();
        let entry2: TLBEntry = rkyv::from_bytes::<TLBEntry, rkyv::rancor::Error>(&bytes).unwrap();

        assert_eq!(entry.valid, entry2.valid);
        assert_eq!(entry.ts, entry2.ts);
        assert_eq!(entry.vpn, entry2.vpn);
        assert_eq!(entry.ppn, entry2.ppn);
    }

    #[test]
    fn test_tlb_helper_json_roundtrip() {
        let tlb = create_tlb_helper();
        let json = serde_json::to_string(&tlb).unwrap();
        let tlb2: TLBHelper = serde_json::from_str(&json).unwrap();

        assert_eq!(tlb.entries.len(), tlb2.entries.len());
        for (set1, set2) in tlb.entries.iter().zip(tlb2.entries.iter()) {
            assert_eq!(set1.current_pointer, set2.current_pointer);
            assert_eq!(set1.entries.len(), set2.entries.len());
        }
    }

    #[test]
    fn test_tlb_helper_rkyv_roundtrip() {
        let tlb = create_tlb_helper();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&tlb).unwrap();
        let tlb2: TLBHelper = rkyv::from_bytes::<TLBHelper, rkyv::rancor::Error>(&bytes).unwrap();

        assert_eq!(tlb.entries.len(), tlb2.entries.len());
    }

    #[test]
    fn test_fully_associative_tlb_helper_json_roundtrip() {
        let tlb = create_fully_associative_tlb_helper();
        let json = serde_json::to_string(&tlb).unwrap();
        let tlb2: FullyAssociativeTLBHelper = serde_json::from_str(&json).unwrap();

        assert_eq!(tlb.associativity, tlb2.associativity);
        assert_eq!(tlb.deferred_elements_exist, tlb2.deferred_elements_exist);
        assert_eq!(tlb.elements.len(), tlb2.elements.len());
    }

    #[test]
    fn test_fully_associative_tlb_helper_rkyv_roundtrip() {
        let tlb = create_fully_associative_tlb_helper();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&tlb).unwrap();
        let tlb2: FullyAssociativeTLBHelper =
            rkyv::from_bytes::<FullyAssociativeTLBHelper, rkyv::rancor::Error>(&bytes).unwrap();

        assert_eq!(tlb.associativity, tlb2.associativity);
        assert_eq!(tlb.elements.len(), tlb2.elements.len());
    }

    #[test]
    fn test_huge_tlb_helper_json_roundtrip() {
        let htlb = create_huge_tlb_helper();
        let json = serde_json::to_string(&htlb).unwrap();
        let htlb2: HugeTLBHelper = serde_json::from_str(&json).unwrap();

        assert_eq!(htlb.entries.len(), htlb2.entries.len());
    }

    #[test]
    fn test_huge_tlb_helper_rkyv_roundtrip() {
        let htlb = create_huge_tlb_helper();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&htlb).unwrap();
        let htlb2: HugeTLBHelper =
            rkyv::from_bytes::<HugeTLBHelper, rkyv::rancor::Error>(&bytes).unwrap();

        assert_eq!(htlb.entries.len(), htlb2.entries.len());
    }

    #[test]
    fn test_ordinary_mmu_helper_json_roundtrip() {
        let mmu = create_ordinary_mmu_helper();
        let helper = MMUHelper::OrdinaryMMU(mmu);
        let json = serde_json::to_string(&helper).unwrap();
        let helper2: MMUHelper = serde_json::from_str(&json).unwrap();

        match helper2 {
            MMUHelper::OrdinaryMMU(mmu2) => {
                assert_eq!(mmu2.itlb.entries.len(), 2);
                assert_eq!(mmu2.dtlb.entries.len(), 2);
                assert_eq!(mmu2.stlb.entries.len(), 2);
            }
            _ => panic!("Expected OrdinaryMMU variant"),
        }
    }

    #[test]
    fn test_ordinary_mmu_helper_rkyv_roundtrip() {
        let mmu = create_ordinary_mmu_helper();
        let helper = MMUHelper::OrdinaryMMU(mmu);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
        let helper2: MMUHelper =
            rkyv::from_bytes::<MMUHelper, rkyv::rancor::Error>(&bytes).unwrap();

        match helper2 {
            MMUHelper::OrdinaryMMU(mmu2) => {
                assert_eq!(mmu2.itlb.entries.len(), 2);
            }
            _ => panic!("Expected OrdinaryMMU variant"),
        }
    }

    #[test]
    fn test_fully_associative_l1_mmu_helper_json_roundtrip() {
        let helper = FullyAssociativeL1MMUHelper {
            l0_itlb: (0x1000, AddressSpaceID::NonGlobal(42), 0x2000),
            stlb: create_tlb_helper(),
            itlb: create_fully_associative_tlb_helper(),
            dtlb: create_fully_associative_tlb_helper(),
            htbl_2m: create_huge_tlb_helper(),
            htbl_1g: HugeTLBHelper { entries: vec![] },
        };
        let mmu_helper = MMUHelper::FullyAssociativeL1MMU(helper);
        let json = serde_json::to_string(&mmu_helper).unwrap();
        let mmu_helper2: MMUHelper = serde_json::from_str(&json).unwrap();

        match mmu_helper2 {
            MMUHelper::FullyAssociativeL1MMU(h) => {
                assert_eq!(h.l0_itlb.0, 0x1000);
                assert_eq!(h.l0_itlb.2, 0x2000);
            }
            _ => panic!("Expected FullyAssociativeL1MMU variant"),
        }
    }

    #[test]
    fn test_fully_associative_l1_mmu_helper_rkyv_roundtrip() {
        let helper = FullyAssociativeL1MMUHelper {
            l0_itlb: (0x1000, AddressSpaceID::NonGlobal(42), 0x2000),
            stlb: create_tlb_helper(),
            itlb: create_fully_associative_tlb_helper(),
            dtlb: create_fully_associative_tlb_helper(),
            htbl_2m: create_huge_tlb_helper(),
            htbl_1g: HugeTLBHelper { entries: vec![] },
        };
        let mmu_helper = MMUHelper::FullyAssociativeL1MMU(helper);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&mmu_helper).unwrap();
        let mmu_helper2: MMUHelper =
            rkyv::from_bytes::<MMUHelper, rkyv::rancor::Error>(&bytes).unwrap();

        match mmu_helper2 {
            MMUHelper::FullyAssociativeL1MMU(h) => {
                assert_eq!(h.l0_itlb.0, 0x1000);
            }
            _ => panic!("Expected FullyAssociativeL1MMU variant"),
        }
    }

    #[test]
    fn test_no_mmu_helper_json_roundtrip() {
        let helper = MMUHelper::NoMMU(NoMMUHelper {});
        let json = serde_json::to_string(&helper).unwrap();
        let helper2: MMUHelper = serde_json::from_str(&json).unwrap();

        match helper2 {
            MMUHelper::NoMMU(_) => {}
            _ => panic!("Expected NoMMU variant"),
        }
    }

    #[test]
    fn test_no_mmu_helper_rkyv_roundtrip() {
        let helper = MMUHelper::NoMMU(NoMMUHelper {});
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
        let helper2: MMUHelper =
            rkyv::from_bytes::<MMUHelper, rkyv::rancor::Error>(&bytes).unwrap();

        match helper2 {
            MMUHelper::NoMMU(_) => {}
            _ => panic!("Expected NoMMU variant"),
        }
    }

    #[test]
    fn test_mmus_helper_json_roundtrip() {
        let mmus_helper = MMUsHelper {
            mmus: vec![
                MMUHelper::OrdinaryMMU(create_ordinary_mmu_helper()),
                MMUHelper::NoMMU(NoMMUHelper {}),
            ],
        };
        let json = serde_json::to_string(&mmus_helper).unwrap();
        let mmus_helper2: MMUsHelper = serde_json::from_str(&json).unwrap();

        assert_eq!(mmus_helper.mmus.len(), mmus_helper2.mmus.len());
    }

    #[test]
    fn test_mmus_helper_rkyv_roundtrip() {
        let mmus_helper = MMUsHelper {
            mmus: vec![
                MMUHelper::OrdinaryMMU(create_ordinary_mmu_helper()),
                MMUHelper::NoMMU(NoMMUHelper {}),
            ],
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&mmus_helper).unwrap();
        let mmus_helper2: MMUsHelper =
            rkyv::from_bytes::<MMUsHelper, rkyv::rancor::Error>(&bytes).unwrap();

        assert_eq!(mmus_helper.mmus.len(), mmus_helper2.mmus.len());
    }
}
