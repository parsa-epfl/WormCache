// This fine defines the type of the memory hierarchy in the checkpoint for each checkpoint.

use std::collections::HashMap;

pub struct PrivateCacheParameters {
    pub l1i_sets: usize,
    pub l1i_associativity: usize,

    pub l1d_sets: usize,
    pub l1d_associativity: usize,

    pub l2_sets: usize,
    pub l2_associativity: usize
}

impl PrivateCacheParameters {
    pub fn private_cache_iter(&self) -> [(usize, usize, bool); 2] {
        return [
            (self.l1i_sets, self.l1i_associativity, true),
            (self.l1d_sets, self.l1d_associativity, false)
        ];
    }
}

// Currently the supported coherence model is MESI, which is the model used by QFlex.
#[derive(Clone, Copy)]
pub enum CacheBlockPermission {
    Invalid = 0, // Invalid
    CleanShared = 1, // Shared
    CleanExclusive = 2, // Exclusive
    ModifiedExclusive = 3, // Modified
    ModifiedOwned = 4, // Owned
}

pub struct CacheBlock {
    pub block_id: usize,
    pub perm: CacheBlockPermission
}

pub struct DirectoryBlock {
    pub tag: usize,
    pub replicas: Vec<u8>, // core_ids
    pub last_writer: Option<u8>
}

pub type SerializedCache = Vec<Vec<CacheBlock>>;

pub type SerializedDirectory = Vec<Vec<DirectoryBlock>>;

pub struct MemoryHierarchyCheckPoint {
    pub private_cache: HashMap<u8, SerializedCache>,
    pub directory: SerializedDirectory,
    pub shared_cache: SerializedCache
}
