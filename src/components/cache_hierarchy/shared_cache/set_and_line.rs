use crate::components::debug::cache_line_history::CacheLineCoherenceHistory;

#[derive(Debug, Clone)]
pub struct SharedCacheBlock {
    pub block_id_with_v: u64, // the last bit is the valid bit.
    pub ts: u64,
    pub modified: bool,
}

#[derive(Debug)]
pub struct SharedCacheSet<const WAY: usize, const EXCLUSIVE: bool> {
    pub blocks: [SharedCacheBlock; WAY],
    pub touched_count: usize,
}

impl<const WAY: usize, const EXCLUSIVE: bool> SharedCacheSet<WAY, EXCLUSIVE> {
    pub fn new() -> Self {
        Self {
            blocks: std::array::from_fn(|_| SharedCacheBlock {
                block_id_with_v: 0,
                ts: 0,
                modified: false,
            }),
            touched_count: 0,
        }
    }

    fn peek(&mut self, block_id: u64, ts: u64) -> Option<bool> {
        let internal_block_id = block_id << 1 | 1;

        // first of all, find whether this block is a hit.
        let hit_block = self.blocks.iter_mut().find(|p| {
            return p.block_id_with_v == internal_block_id;
        });

        if let Some(hit_block) = hit_block {
            if ts > hit_block.ts {
                hit_block.ts = ts;
            }
            return Some(hit_block.modified);
        }

        // otherwise, it is a miss.
        return None;
    }

    pub fn invalidate(&mut self, block_id: u64) -> Option<bool> {
        let internal_block_id = block_id << 1 | 1;

        // first of all, find whether this block is a hit.
        let hit_block = self.blocks.iter_mut().find(|p| {
            return p.block_id_with_v == internal_block_id;
        });

        // if it is a hit, we remove this block from the cache
        if let Some(hit_block) = hit_block {
            let res = Some(hit_block.modified);
            hit_block.block_id_with_v = 0;
            return res;
        }

        // otherwise, it is a miss.
        return None;
    }

    #[inline]
    pub fn lookup(&mut self, block_id: u64, ts: u64) -> Option<bool> {
        return if EXCLUSIVE {
            self.invalidate(block_id)
        } else {
            self.peek(block_id, ts)
        };
    }

    #[inline]
    pub fn insert(
        &mut self,
        block_id: u64,
        ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    ) {
        let internal_block_id = block_id << 1 | 1;

        // first of all, find whether this block is a hit.
        let hit_block = self.blocks.iter_mut().find(|p| {
            return p.block_id_with_v == internal_block_id;
        });

        // it is definitely not be a hit, so we need to assert.

        if let Some(hit_block) = hit_block {
            if EXCLUSIVE {
                // this should not happen for exclusive caches
                CacheLineCoherenceHistory::global_get_block_history(block_id)
                    .unwrap()
                    .print_history();
                panic!("Error: the incoming block is already in the shared cache.");
            } else {
                // update the timestamp and the modified bit.
                if ts > hit_block.ts {
                    hit_block.ts = ts;
                }
                hit_block.modified = is_modified;
                return;
            }
        }

        if increase_touched_count && self.touched_count < WAY {
            self.touched_count += 1;
        }

        // then, find the first invalid block.
        let invalid_block = self.blocks.iter_mut().find(|p| {
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
        let oldest_block = self
            .blocks
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
