// This module defines the basic memory hierarchies using fine-grained locks.
// It contains the same logical memory hierarchy as the `memory`, but uses locks for shared communication.
// - TLB, which is private.
// - Private caches, with set locks.
// - Directory, with set locks.
// - Shared caches, with set locks.

pub mod common;
mod parallel_hierarchy;
mod single_cache_hierarchy;

pub use parallel_hierarchy::hierarchy;
pub use parallel_hierarchy::ParallelCacheHierarchyPlugin;

pub use single_cache_hierarchy::SingleCacheHierarchyPlugin;
