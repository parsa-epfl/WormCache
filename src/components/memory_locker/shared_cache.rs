use std::sync::Mutex;
use std::io::prelude::*;

// There are two possible operations for an exclusive shared cache
// 1. Empty to the cache, which means a write lock is required.
// 2. Read from the cache, depending on the result:
//    - Read is a hit: Read lock + write lock
//    - Read is a miss: Read lock
// 3. It will be probably OK to use Mutex.

#[derive(Debug)]
pub struct SharedCacheBlock {
    pub valid: bool,
    pub tag: u64,
    pub ts: u64,
}

#[derive(Debug)]
#[repr(align(64))]
pub struct SharedCacheSet<const WAY: usize> {
    pub blocks: [SharedCacheBlock; WAY],
    pub access_counter: u64,
}

impl<const WAY: usize> SharedCacheSet<WAY> {
    pub fn new() -> Self {
        Self {
            blocks: std::array::from_fn(|_| SharedCacheBlock {
                valid: false,
                tag: 0,
                ts: 0,
            }),
            access_counter: 0,
        }
    }
}

pub struct ExclusiveSharedCache<const SET: usize, const WAY: usize> {
    blocks: Box<[Mutex<SharedCacheSet<WAY>>; SET]>,
}

impl<const SET: usize, const WAY: usize> ExclusiveSharedCache<SET, WAY> {
    pub fn new() -> Self {
        Self {
            blocks: crate::util::init_heap_array(|_| Mutex::new(SharedCacheSet::new())),
        }
    }

    pub fn allocate(&self, block_id: u64, ts: u64) {
        let set_id = (block_id % SET as u64) as usize;
        let mut blocks = self.blocks[set_id].lock().unwrap();

        blocks.access_counter += 1;

        // first of all, find whether this block is a hit.
        let hit_block = blocks.blocks.iter_mut().find(|p| {
            return p.valid && p.tag == block_id;
        });

        // it is definitely not be a hit, so we need to assert.
        assert!(hit_block.is_none());

        // then, find the first invalid block.
        let invalid_block = blocks.blocks.iter_mut().find(|p| {
            return !p.valid;
        });

        // if there is an invalid block, we replace that block.
        if let Some(invalid_block) = invalid_block {
            invalid_block.valid = true;
            invalid_block.tag = block_id;
            invalid_block.ts = ts;
            return;
        }

        // otherwise, we need to find the oldest block.
        let oldest_block = blocks
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
        oldest_block.valid = true;
        oldest_block.tag = block_id;
        oldest_block.ts = ts;
    }

    pub fn lookup(&self, block_id: u64) -> bool {
        let set_id = (block_id % SET as u64) as usize;
        let mut blocks = self.blocks[set_id].lock().unwrap();

        blocks.access_counter += 1;

        // first of all, find whether this block is a hit.
        let hit_block = blocks.blocks.iter_mut().find(|p| {
            return p.valid && p.tag == block_id;
        });

        // if it is a hit, we remove this block from the cache
        if let Some(hit_block) = hit_block {
            hit_block.valid = false;
            return true;
        }

        // otherwise, it is a miss.
        return false;
    }

    pub fn invalidate(&self, block_id: u64) -> bool {
        let set_id = (block_id % SET as u64) as usize;
        let mut blocks = self.blocks[set_id].lock().unwrap();

        // first of all, find whether this block is a hit.
        let hit_block = blocks.blocks.iter_mut().find(|p| {
            return p.valid && p.tag == block_id;
        });

        // if it is a hit, we remove this block from the cache
        if let Some(hit_block) = hit_block {
            hit_block.valid = false;
            return true;
        }

        // otherwise, it is a miss.
        return false;
    }

    pub fn dump_access_counter(&self) {
        // write the access counter of each set to a file. Each set takes a line.
        let mut file = std::fs::File::create("access_counter.txt").unwrap();
        for set in self.blocks.iter() {
            let set = set.lock().unwrap();
            writeln!(file, "{}", set.access_counter).unwrap();
        }
    }
}
