// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! Directory checkpoint helpers - unified type for both JSON and rkyv serialization.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};

use crate::components::cache_hierarchy::common::DirectoryEntry;

/// Unified helper for serializing a directory set.
/// Uses Vec of tuples for rkyv compatibility, and HashMap for internal conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct DirectorySetHelper {
    pub entries: Vec<(u64, DirectoryEntry)>,
}

impl DirectorySetHelper {
    pub fn from_hashmap(map: &HashMap<u64, DirectoryEntry>) -> Self {
        Self {
            entries: map.iter().map(|(k, v)| (*k, v.clone())).collect(),
        }
    }

    pub fn into_hashmap(self) -> HashMap<u64, DirectoryEntry> {
        self.entries.into_iter().collect()
    }
}

/// Unified helper for serializing the entire directory.
/// Works with both serde (JSON) and rkyv formats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct DirectoryHelper {
    pub sets: Vec<DirectorySetHelper>,
}

impl DirectoryHelper {
    /// Create from a vector of HashMaps (the internal directory representation).
    pub fn from_sets(sets: &[HashMap<u64, DirectoryEntry>]) -> Self {
        Self {
            sets: sets.iter().map(DirectorySetHelper::from_hashmap).collect(),
        }
    }

    /// Convert back to vector of HashMaps.
    pub fn into_sets(self) -> Vec<HashMap<u64, DirectoryEntry>> {
        self.sets.into_iter().map(|s| s.into_hashmap()).collect()
    }
}
