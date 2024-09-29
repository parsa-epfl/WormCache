mod cache_hierarchy;
mod frontend;
mod mmu;

pub use cache_hierarchy::process_cache_hierarchy;
pub use frontend::process_frontend;
pub use mmu::process_mmus;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FlexusDirectoryType {
    Infinite,
    Standard { sets: usize, associativity: usize },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FlexusParameter {
    pub l1i_sets: usize,
    pub l1i_associativity: usize,

    pub l1d_sets: usize,
    pub l1d_associativity: usize,

    pub itlb_sets: usize,
    pub itlb_associativity: usize,

    pub dtlb_sets: usize,
    pub dtlb_associativity: usize,

    pub l2_sets: usize,
    pub l2_associativity: usize,

    pub directory: FlexusDirectoryType,

    pub btb_sets: usize,
    pub btb_associativity: usize,
}
