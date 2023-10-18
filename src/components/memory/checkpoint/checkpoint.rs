// This fine defines the type of the memory hierarchy in the checkpoint for each checkpoint.

use std::collections::HashMap;
use serde::{self, Serialize};

pub struct PrivateCacheParameters {
    pub l1i_sets: usize,
    pub l1i_associativity: usize,

    pub l1d_sets: usize,
    pub l1d_associativity: usize,

    pub l2_sets: usize,
    pub l2_associativity: usize,

    pub directory_associativity: usize
}


// Currently the supported coherence model is MESI, which is the model used by QFlex.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum CacheBlockState {
    Invalid = 0, // Invalid
    CleanShared = 1, // Shared
    CleanExclusive = 2, // Exclusive
    ModifiedExclusive = 3, // Modified
    ModifiedOwned = 4, // Owned
}

// The normal cache blocks
#[derive(PartialEq, Eq, Clone, Debug, Serialize)]
pub struct CacheBlock {
    pub block_id: usize,
    pub state: CacheBlockState,
    pub in_instruction_cache: bool,
    pub in_data_cache: bool
}

#[derive(PartialEq, Eq, Clone, Debug, Serialize)]
pub struct DirectoryBlock {
    pub block_id: usize,
    pub replicas: Vec<u8>, // core_ids
    pub last_writer: Option<u8>
}


pub type SerializedCache = Vec<Vec<CacheBlock>>;

pub type SerializedDirectory = Vec<Vec<DirectoryBlock>>;

#[derive(Serialize, Debug)]
pub struct MemoryHierarchyCheckPoint {
    pub l1i: HashMap<u8, SerializedCache>,
    pub l1d: HashMap<u8, SerializedCache>,
    pub l2: HashMap<u8, SerializedCache>,
    pub directory: SerializedDirectory,
    pub shared_cache: SerializedCache
}
