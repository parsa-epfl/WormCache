// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.

//! Checkpoint serialization helpers.
//!
//! This module contains helper types that are used for both JSON (serde) and
//! binary (rkyv) serialization of checkpoint data. These types provide a clean
//! separation between runtime data structures and their serialized forms.
//!
//! Each helper type implements:
//! - `From<&T>` to convert from the runtime type to the helper
//! - Both serde and rkyv derives for dual-format serialization

mod bp;
mod directory;
mod private_cache;
mod shared_cache;

pub use bp::*;
pub use directory::*;
pub use private_cache::*;
pub use shared_cache::*;

#[cfg(test)]
mod tests;
