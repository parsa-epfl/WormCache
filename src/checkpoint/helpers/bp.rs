// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! Branch Predictor checkpoint helpers - unified types for both JSON and rkyv serialization.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

use crate::components::bp::fetch::btb::BTBEntry;
use crate::components::bp::fetch::tage::{FoldedHistory, TAGEBiModalEntry, TAGEGlobalTableEntry};

/// Unified helper for BTB serialization.
/// Works with both serde (JSON) and rkyv formats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct BTBHelper {
    pub array: Vec<Vec<BTBEntry>>,
    pub local_ts: u64,
}

/// Unified helper for Return Address Stack serialization.
/// Works with both serde (JSON) and rkyv formats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct RASHelper {
    pub stack: Vec<u64>,
}

/// Unified helper for TAGE predictor serialization.
/// Works with both serde (JSON) and rkyv formats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct TAGEHelper {
    pub tick: i32,
    pub phist: i32,
    pub ghist: Vec<bool>,
    pub ch_i: Vec<FoldedHistory>,
    pub ch_t: Vec<Vec<FoldedHistory>>,
    pub btable: Vec<TAGEBiModalEntry>,
    pub gtable: Vec<Vec<TAGEGlobalTableEntry>>,
    pub seed: i32,
}

/// Unified helper for per-core fetch unit serialization.
/// Works with both serde (JSON) and rkyv formats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct PerCoreFetchUnitHelper {
    pub btb: BTBHelper,
    pub ras: RASHelper,
    pub tage: TAGEHelper,
}

/// Unified helper for the entire fetch unit serialization.
/// Works with both serde (JSON) and rkyv formats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct FetchUnitHelper {
    pub private_units: Vec<PerCoreFetchUnitHelper>,
}
