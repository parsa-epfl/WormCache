pub mod cache_hierarchy;
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

    pub l2_sets: usize,
    pub l2_associativity: usize,
    pub l2_slice_count: usize,

    pub stlb_sets: usize,
    pub stlb_associativity: usize,

    pub directory: FlexusDirectoryType,
    pub directory_slice_count: usize,

    pub btb_sets: usize,
    pub btb_associativity: usize,

    #[serde(skip)]
    pub no_resizing: bool,
}

impl Default for FlexusParameter {
    fn default() -> Self {
        FlexusParameter {
            l1i_sets: 0,
            l1i_associativity: 0,
            l1d_sets: 0,
            l1d_associativity: 0,
            l2_sets: 0,
            l2_associativity: 0,
            l2_slice_count: 0,
            stlb_sets: 0,
            stlb_associativity: 0,
            directory: FlexusDirectoryType::Standard {
                sets: 0,
                associativity: 0,
            },
            directory_slice_count: 0,
            btb_sets: 0,
            btb_associativity: 0,
            no_resizing: false,
        }
    }
}
