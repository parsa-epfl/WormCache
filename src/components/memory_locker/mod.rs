// This module defines the basic memory hierarchies using fine-grained locks.
// It contains the same logical memory hierarchy as the `memory`, but uses locks for shared communication.
// - TLB, which is private.
// - Private caches, with set locks.
// - Directory, with set locks. 
// - Shared caches, with set locks.

mod private_cache;
pub mod shared_cache;
pub mod directory;