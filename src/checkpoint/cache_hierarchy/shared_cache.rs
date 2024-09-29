use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    checkpoint::FlexusParameter,
    components::cache_hierarchy::common::{PrivateCacheLine, SharedCacheBlock},
};

#[derive(Serialize, Deserialize)]
struct SharedCacheSet {
    blocks: Vec<SharedCacheBlock>,
    touched_count: usize,
    recent_evict_ts: u64,
    recent_evict_vts: u64,
    access_count: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SingleSharedCacheSerdeHelper {
    // This block is different from the one from worm_cache.
    blocks: Vec<SharedCacheSet>,
    warmed_sets: usize,
}

#[derive(Serialize)]
pub struct FlexusSerializedSharedCacheBlock {
    pub tag: u64,
    pub dirty: bool,
    pub writable: bool,
}

fn serialize_a_share_cache_set(
    set: &SharedCacheSet,
    set_number: usize,
) -> Vec<FlexusSerializedSharedCacheBlock> {
    let mut result = vec![];
    let log2_set = set_number.trailing_zeros();

    for block in set.blocks.iter() {
        result.push(FlexusSerializedSharedCacheBlock {
            tag: block.block_id_with_v >> 1 >> log2_set,
            dirty: block.modified,
            writable: true,
        });
    }

    result
}

impl SingleSharedCacheSerdeHelper {
    pub fn process_evicted_cache_line(&mut self, line: &PrivateCacheLine, accessor: u32) {
        let block_id = line.block_id();
        let set_idx = block_id as usize % self.blocks.len();
        let set = &mut self.blocks[set_idx];

        for block in set.blocks.iter_mut() {
            let is_valid = block.block_id_with_v & 1 == 1;

            if !is_valid {
                assert!(block.ts == 0);
            }

            let block_id = block.block_id_with_v >> 1;
            if block_id == line.block_id() {
                if is_valid && line.ts > block.ts {
                    block.ts = line.ts;
                    block.modified = line.modified;
                }

                return;
            }
        }

        set.blocks.push(SharedCacheBlock {
            block_id_with_v: (line.block_id() << 1) | 1,
            ts: line.ts,
            modified: line.modified,
            last_accessor: accessor,
        });
    }

    pub fn resize(&mut self, flexus_configuration: &FlexusParameter) {
        assert!(self.blocks.len() % flexus_configuration.l2_sets == 0);

        let mut new_blocks = vec![];

        for _ in 0..flexus_configuration.l2_sets {
            new_blocks.push(SharedCacheSet {
                blocks: vec![],
                touched_count: 0,
                recent_evict_ts: 0,
                recent_evict_vts: 0,
                access_count: 0,
            });
        }

        // Step 1: Combine sets.
        for (set_idx, set) in self.blocks.iter().enumerate() {
            let new_idx = set_idx % flexus_configuration.l2_sets;

            new_blocks[new_idx]
                .blocks
                .extend(set.blocks.iter().cloned());
        }

        // Step 2: Filter each set and keep the most recent blocks.
        for set in new_blocks.iter_mut() {
            set.blocks.retain(|block| block.block_id_with_v & 1 == 1); // keep valid blocks.
            set.blocks.sort_by_key(|block| block.ts);
            set.blocks.reverse();

            set.blocks.truncate(flexus_configuration.l2_associativity);
        }

        self.blocks = new_blocks;
    }

    pub fn export(&self, folder_name: &String, flexus_configuration: &FlexusParameter) {
        let mut file = std::fs::File::create(format!("{}/sys-L2-cache.json", folder_name)).unwrap();

        serde_json::to_writer(
            &mut file,
            &json!({
                "associativity": flexus_configuration.l2_associativity,
                "tags": self.blocks.iter().map(|set| {
                    serialize_a_share_cache_set(set, flexus_configuration.l2_sets)
                }).collect::<Vec<_>>(),
            }),
        )
        .unwrap();

        println!(
            "Shared cache is exported to {}/sys-L2-cache.json",
            folder_name
        );
    }
}
