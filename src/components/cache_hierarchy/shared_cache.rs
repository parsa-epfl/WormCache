use std::sync::Mutex;

use serde::Serialize;
use serde_json::json;

use crate::components::debug::cache_line_history::CacheLineCoherenceHistory;

// There are two possible operations for an exclusive shared cache
// 1. Empty to the cache, which means a write lock is required.
// 2. Read from the cache, depending on the result:
//    - Read is a hit: Read lock + write lock
//    - Read is a miss: Read lock
// 3. It will be probably OK to use Mutex.

#[derive(Debug)]
pub struct SharedCacheBlock {
    pub block_id_with_v: u64, // the last bit is the valid bit.
    pub ts: u64,
    pub modified: bool,
}

pub struct SharedCache<const SET: usize, const WAY: usize, const EXCLUSIVE: bool> {
    blocks: Box<[Mutex<[SharedCacheBlock; WAY]>; SET]>,
}

impl<const SET: usize, const WAY: usize, const EXCLUSIVE: bool> SharedCache<SET, WAY, EXCLUSIVE> {
    pub fn new() -> Self {
        Self {
            blocks: crate::util::init_heap_array(|_| {
                Mutex::new(std::array::from_fn(|_| SharedCacheBlock {
                    block_id_with_v: 0,
                    ts: 0,
                    modified: false,
                }))
            }),
        }
    }

    pub fn invalidate(&self, block_id: u64) -> bool {
        let set_id = (block_id % SET as u64) as usize;
        let internal_block_id = block_id << 1 | 1;
        let mut blocks = self.blocks[set_id].lock().unwrap();

        // first of all, find whether this block is a hit.
        let hit_block = blocks.iter_mut().find(|p| {
            return p.block_id_with_v == internal_block_id;
        });

        // if it is a hit, we remove this block from the cache
        if let Some(hit_block) = hit_block {
            hit_block.block_id_with_v = 0;
            return true;
        }

        // otherwise, it is a miss.
        return false;
    }
}

// The lookup function for exclusive shared cache.
impl<const SET: usize, const WAY: usize> SharedCache<SET, WAY, true> {
    pub fn lookup(&self, block_id: u64) -> (bool, bool) {
        // (is_hit, is_modified)
        let set_id = (block_id % SET as u64) as usize;
        let internal_block_id = block_id << 1 | 1;
        let mut blocks = self.blocks[set_id].lock().unwrap();

        // first of all, find whether this block is a hit.
        let hit_block = blocks.iter_mut().find(|p| {
            return p.block_id_with_v == internal_block_id;
        });

        // if it is a hit, we remove this block from the cache
        if let Some(hit_block) = hit_block {
            hit_block.block_id_with_v = 0;
            return (true, hit_block.modified);
        }

        // otherwise, it is a miss.
        return (false, false);
    }

    pub fn evict_to(&self, block_id: u64, ts: u64, is_modified: bool) {
        let set_id = (block_id % SET as u64) as usize;
        let internal_block_id = block_id << 1 | 1;

        let mut blocks = self.blocks[set_id].lock().unwrap();

        // first of all, find whether this block is a hit.
        let hit_block = blocks.iter_mut().find(|p| {
            return p.block_id_with_v == internal_block_id;
        });

        // it is definitely not be a hit, so we need to assert.
        if !hit_block.is_none() {
            CacheLineCoherenceHistory::global_get_block_history(block_id)
                .unwrap()
                .print_history();
            assert!(hit_block.is_none());
        }

        // then, find the first invalid block.
        let invalid_block = blocks.iter_mut().find(|p| {
            return (p.block_id_with_v & 1) == 0;
        });

        // if there is an invalid block, we replace that block.
        if let Some(invalid_block) = invalid_block {
            invalid_block.block_id_with_v = internal_block_id;
            invalid_block.modified = is_modified;
            invalid_block.ts = ts;
            return;
        }

        // otherwise, we need to find the oldest block.
        let oldest_block = blocks
            .iter_mut()
            .min_by_key(|p| {
                return p.ts;
            })
            .unwrap();

        // if the oldest block even has larger timestamp than the incoming block, we should print a log and do nothing.
        if oldest_block.ts > ts {
            println!("Warning: the incoming block has smaller timestamp than the oldest block in the shared cache.");
            return;
        }

        // otherwise, we replace the oldest block.
        oldest_block.block_id_with_v = internal_block_id;
        oldest_block.modified = is_modified;
        oldest_block.ts = ts;
    }
}

// The lookup function for non-inclusive shared cache.
impl<const SET: usize, const WAY: usize> SharedCache<SET, WAY, false> {
    pub fn lookup(&self, block_id: u64) -> (bool, bool) {
        let set_id = (block_id % SET as u64) as usize;
        let internal_block_id = block_id << 1 | 1;
        let mut blocks = self.blocks[set_id].lock().unwrap();

        // first of all, find whether this block is a hit.
        let hit_block = blocks.iter_mut().find(|p| {
            return p.block_id_with_v == internal_block_id;
        });

        // if it is a hit, we remove this block from the cache
        match hit_block {
            Some(hit_block) => (true, hit_block.modified),
            None => (false, false),
        }
    }

    pub fn evict_to(&self, block_id: u64, ts: u64, is_modified: bool) {
        let set_id = (block_id % SET as u64) as usize;
        let internal_block_id = block_id << 1 | 1;
        let mut blocks = self.blocks[set_id].lock().unwrap();

        // first of all, find whether this block is a hit.
        let hit_block = blocks.iter_mut().find(|p| {
            return p.block_id_with_v == internal_block_id;
        });

        // If there is a hit, we need to update the block.
        if let Some(hit_block) = hit_block {
            hit_block.ts = ts;
            hit_block.modified = is_modified;
            return;
        }

        // then, find the first invalid block.
        let invalid_block = blocks.iter_mut().find(|p| {
            return (p.block_id_with_v & 1) == 0;
        });

        // if there is an invalid block, we replace that block.
        if let Some(invalid_block) = invalid_block {
            invalid_block.block_id_with_v = internal_block_id;
            invalid_block.modified = is_modified;
            invalid_block.ts = ts;
            return;
        }

        // otherwise, we need to find the oldest block.
        let oldest_block = blocks
            .iter_mut()
            .min_by_key(|p| {
                return p.ts;
            })
            .unwrap();

        // if the oldest block even has larger timestamp than the incoming block, we should print a log and do nothing.
        if oldest_block.ts > ts {
            println!("Warning: the incoming block has smaller timestamp than the oldest block in the shared cache.");
            return;
        }

        // otherwise, we replace the oldest block.
        oldest_block.block_id_with_v = internal_block_id;
        oldest_block.modified = is_modified;
        oldest_block.ts = ts;
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Serialize)]
pub struct SerializedSharedCacheEntry {
    pub tag: u64,
    pub dirty: bool,
    pub writable: bool,
}

impl<const SET: usize, const WAY: usize, const EXCLUSIVE: bool> SharedCache<SET, WAY, EXCLUSIVE> {
    pub fn dump_snapshot(&self, snapshot_name: &str) {
        let mut file = std::fs::File::create(format!("{}/shared_cache.json", snapshot_name)).unwrap();

        let log2_set = SET.trailing_zeros();

        let entries = self
            .blocks
            .iter()
            .map(|entry| {
                let entry = entry.lock().unwrap();
                let mut sorted_lines: Vec<_> = entry.iter().collect();
                sorted_lines.sort_by(|a, b| a.ts.cmp(&b.ts));

                sorted_lines
                    .iter()
                    .filter_map(|block| {
                        if block.block_id_with_v & 1 == 0 {
                            return None;
                        }
                        Some(SerializedSharedCacheEntry {
                            tag: (block.block_id_with_v >> 1) >> log2_set,
                            dirty: block.modified,
                            writable: true,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        serde_json::to_writer_pretty(&mut file, &json!({
            "associativity": WAY,
            "tags": entries,
        })).unwrap();
    }
}
