// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! SMS (Spatial Memory Streaming) checkpoint helpers.
//!
//! This module provides helper types for serializing the PHT (Pattern History Table)
//! component of SMS. These types work with both serde (JSON) and rkyv formats.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// Helper for serializing a single PHT entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct PHTEntryHelper {
    pub tag: u64,
    pub access_pattern: Vec<u8>,
    pub write_pattern: Vec<u8>,
    pub read_pattern: Vec<u8>,
    pub ts: u64,
    pub valid: bool,
}

/// Helper for serializing a PHT set (collection of entries).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct PHTSetHelper {
    pub entries: Vec<PHTEntryHelper>,
}

/// Helper for serializing a per-core PHT (collection of sets).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct PHTPerCoreHelper {
    pub sets: Vec<PHTSetHelper>,
}

/// Helper for serializing the entire PHT structure (all cores).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct PHTHelper {
    pub tables: Vec<PHTPerCoreHelper>,
}
