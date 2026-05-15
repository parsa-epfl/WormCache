// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! Shared cache checkpoint helpers.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

use crate::components::cache_hierarchy::common::{
    SharedCacheBlock, SharedCacheSet, statistics::SharedCacheSetStatistics,
};

/// Helper for serializing a single shared cache set.
/// Contains only the data needed for checkpointing (no statistics).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct SharedCacheSetHelper {
    pub blocks: Vec<SharedCacheBlock>,
    pub touched_count: usize,
    pub recent_evict_ts: u64,
    pub access_count: u64,
}

impl<const WAY: usize, const SET: usize, const EXCLUSIVE: bool, S: SharedCacheSetStatistics>
    From<&SharedCacheSet<WAY, SET, EXCLUSIVE, S>> for SharedCacheSetHelper
{
    fn from(set: &SharedCacheSet<WAY, SET, EXCLUSIVE, S>) -> Self {
        Self {
            blocks: set.blocks.to_vec(),
            touched_count: set.touched_count,
            recent_evict_ts: set.recent_evict_ts,
            access_count: set.access_count,
        }
    }
}

impl SharedCacheSetHelper {
    /// Convert back to a SharedCacheSet with default statistics.
    pub fn into_set<
        const WAY: usize,
        const SET: usize,
        const EXCLUSIVE: bool,
        S: SharedCacheSetStatistics,
    >(
        self,
    ) -> SharedCacheSet<WAY, SET, EXCLUSIVE, S> {
        let blocks: [SharedCacheBlock; WAY] = self
            .blocks
            .try_into()
            .expect("Block count mismatch during deserialization");

        SharedCacheSet {
            blocks,
            touched_count: self.touched_count,
            recent_evict_ts: self.recent_evict_ts,
            access_count: self.access_count,
            statistics: S::default(),
        }
    }
}

/// Helper for serializing the entire shared cache.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct SharedCacheHelper {
    pub blocks: Vec<SharedCacheSetHelper>,
    pub warmed_sets: usize,
}

impl SharedCacheHelper {
    /// Create a new helper from an iterator of cache sets.
    pub fn from_sets<
        'a,
        const WAY: usize,
        const SET: usize,
        const EXCLUSIVE: bool,
        S: SharedCacheSetStatistics,
    >(
        sets: impl Iterator<Item = &'a SharedCacheSet<WAY, SET, EXCLUSIVE, S>>,
        warmed_sets: usize,
    ) -> Self
    where
        S: 'a,
    {
        Self {
            blocks: sets.map(SharedCacheSetHelper::from).collect(),
            warmed_sets,
        }
    }
}
