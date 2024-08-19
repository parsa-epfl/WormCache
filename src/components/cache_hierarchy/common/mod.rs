mod directory;
mod l0i;
mod private_cache;
mod shared_cache;
mod util;

#[derive(Debug, PartialEq, Clone)]
pub enum CacheAccessType {
    InstructionFetch,

    DataRead,
    DataWrite,

    PageWalkRead,

    PrefetchRead,
    PrefetchWrite,
}

#[derive(PartialEq, Eq, Debug)]
pub enum CacheHierarchyAccessResult {
    HitInSelfPrivateCache,
    MissDueToPermission,
    HitInOtherPrivateCache,
    MissInPrivateCache, // This entry is emitted when we see order violation, because we don't know its state in the shared cache.
    HitInSharedCache,
    Miss,
    Unknown, // This entry is emitted when a memory access arrives late but with a smaller timestamp than a previous write operation. It is unknown because its previous state is not clear.
}

pub use directory::*;
pub use l0i::*;
pub use private_cache::*;
pub use shared_cache::*;
pub use util::*;
