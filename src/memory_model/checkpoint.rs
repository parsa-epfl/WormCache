// This fine defines the type of the memory hierarchy in the checkpoint for each checkpoint.

use std::collections::HashMap;

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
    pub private_cache: HashMap<usize, SerializedCache>,
    pub directory: SerializedDirectory,
    pub shared_cache: SerializedCache
}
