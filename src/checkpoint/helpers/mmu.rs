// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! MMU checkpoint helpers - unified types for both JSON and rkyv serialization.
//!
//! This module provides helper types for serializing/deserializing MMU state.
//! The key challenge with MMU serialization is that the runtime types use
//! HashMaps and generic const parameters, which need to be flattened for
//! rkyv serialization.
//!
//! We reuse `AddressSpaceID`, `TLBEntry`, and `FullyAssociativeTLBEntry` directly
//! since they already have both serde and rkyv derives.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

// Re-export the original types that can be used directly
pub use crate::components::cache_hierarchy::mmu::tlb::{
    AddressSpaceID, FullyAssociativeTLBEntry, TLBEntry,
};

/// Helper for serializing a TLB set.
/// Uses a Vec instead of a fixed-size array to support different associativities.
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct TLBSetHelper {
    pub entries: Vec<TLBEntry>,
    #[serde(default)]
    pub current_pointer: usize,
}

/// Helper for serializing a full TLB.
/// Uses Vecs instead of fixed-size arrays to support different configurations.
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct TLBHelper {
    pub entries: Vec<TLBSetHelper>,
}

/// Helper for serializing a FullyAssociativeTLB.
/// Uses a Vec of tuples instead of FxHashMap for rkyv compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct FullyAssociativeTLBHelper {
    /// Entries stored as (hash, entry) pairs.
    pub elements: Vec<(u64, FullyAssociativeTLBEntry)>,
    pub associativity: usize,
    pub deferred_elements_exist: bool,
}

impl From<&crate::components::cache_hierarchy::mmu::tlb::FullyAssociativeTLB>
    for FullyAssociativeTLBHelper
{
    fn from(tlb: &crate::components::cache_hierarchy::mmu::tlb::FullyAssociativeTLB) -> Self {
        Self {
            elements: tlb.elements.iter().map(|(k, v)| (*k, v.clone())).collect(),
            associativity: tlb.associativity,
            deferred_elements_exist: tlb.deferred_elements_exist,
        }
    }
}

impl FullyAssociativeTLBHelper {
    pub fn into_fully_associative_tlb(
        self,
    ) -> crate::components::cache_hierarchy::mmu::tlb::FullyAssociativeTLB {
        crate::components::cache_hierarchy::mmu::tlb::FullyAssociativeTLB {
            elements: self.elements.into_iter().collect(),
            associativity: self.associativity,
            deferred_elements_exist: self.deferred_elements_exist,
        }
    }
}

/// Helper for serializing huge page TLB entries (2MB/1GB pages).
/// Uses Vec of tuples instead of HashMap for rkyv compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct HugeTLBHelper {
    /// Entries stored as (vpn_shifted, (asid, ppn_shifted)) pairs.
    pub entries: Vec<(u64, (AddressSpaceID, u64))>,
}

impl HugeTLBHelper {
    pub fn from_hashmap(map: &rustc_hash::FxHashMap<u64, (AddressSpaceID, u64)>) -> Self {
        Self {
            entries: map
                .iter()
                .map(|(k, (asid, ppn))| (*k, (*asid, *ppn)))
                .collect(),
        }
    }

    pub fn into_hashmap(self) -> rustc_hash::FxHashMap<u64, (AddressSpaceID, u64)> {
        self.entries.into_iter().collect()
    }
}

/// Helper for serializing the OrdinaryMMU.
/// This struct captures all TLB state without the generic const parameters.
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct OrdinaryMMUHelper {
    pub itlb: TLBHelper,
    pub dtlb: TLBHelper,
    pub stlb: TLBHelper,
    pub htbl_2mb: HugeTLBHelper,
    pub htlb_1gb: HugeTLBHelper,
}

/// Helper for serializing the FullyAssociativeL1MMU.
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct FullyAssociativeL1MMUHelper {
    /// L0 ITLB: (vpn, asid, ppn)
    pub l0_itlb: (u64, AddressSpaceID, u64),
    pub stlb: TLBHelper,
    pub itlb: FullyAssociativeTLBHelper,
    pub dtlb: FullyAssociativeTLBHelper,
    pub htbl_2m: HugeTLBHelper,
    pub htbl_1g: HugeTLBHelper,
}

/// Helper for serializing NoMMU (empty state).
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct NoMMUHelper {}

/// Unified MMU helper that can represent any MMU type.
/// This enum allows serializing different MMU implementations uniformly.
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub enum MMUHelper {
    NoMMU(NoMMUHelper),
    OrdinaryMMU(OrdinaryMMUHelper),
    FullyAssociativeL1MMU(FullyAssociativeL1MMUHelper),
}

/// Helper for serializing multiple MMUs (one per core).
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct MMUsHelper {
    pub mmus: Vec<MMUHelper>,
}
