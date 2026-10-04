// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! IBSP (Instruction-Based Stride Prefetcher) checkpoint helpers.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize,
)]
pub struct RPTEntryHelper {
    pub tag: u64,
    pub last_block: u64,
    pub last_stride: i64,
    pub ts: u64,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize,
)]
pub struct RPTSetHelper {
    pub entries: Vec<RPTEntryHelper>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvDeserialize, RkyvSerialize,
)]
pub struct RPTPerCoreHelper {
    pub sets: Vec<RPTSetHelper>,
}
