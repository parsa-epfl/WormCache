// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! Private cache checkpoint helpers.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

use crate::components::cache_hierarchy::common::PrivateCacheSet;

/// Helper for serializing a unified private cache (per-core).
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct UnifiedPrivateCacheHelper {
    pub cache: Vec<PrivateCacheSet>,
}

/// Helper for serializing a Harvard private cache (per-core).
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct HarvardPrivateCacheHelper {
    pub i_cache: Vec<PrivateCacheSet>,
    pub d_cache: Vec<PrivateCacheSet>,
}
