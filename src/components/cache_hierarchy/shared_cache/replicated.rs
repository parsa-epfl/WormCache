use super::{SerializedSharedCacheBlock, SharedCache, SharedCacheSet};

use serde_json::json;
use std::cell::UnsafeCell;

impl<const WAY: usize, const EXCLUSIVE: bool> SharedCacheSet<WAY, EXCLUSIVE> {
    fn fold(&self, other: &Self) -> Self {
        // take the two arrays, combine them, and sort them by the timestamp. Only keel the elements with highest timestamp.
        let mut imm: Vec<_> = self.blocks.iter().chain(other.blocks.iter()).collect();

        // keep the elements with the highest timestamp.
        imm.sort_by(|a, b| b.ts.cmp(&a.ts));

        // keep the first WAY elements.
        imm.truncate(WAY);

        // keep the first WAY elements.
        Self {
            blocks: std::array::from_fn(|i| (*imm[i]).clone()),
            touched_count: usize::min(self.touched_count + other.touched_count, WAY),
        }
    }
}

struct PrivateSharedCache<const SET: usize, const WAY: usize, const EXCLUSIVE: bool> {
    blocks: Box<[SharedCacheSet<WAY, EXCLUSIVE>; SET]>,
}

impl<const SET: usize, const WAY: usize, const EXCLUSIVE: bool>
    PrivateSharedCache<SET, WAY, EXCLUSIVE>
{
    fn new() -> Self {
        Self {
            blocks: crate::util::init_heap_array(|_| SharedCacheSet::new()),
        }
    }

    fn invalidate(&mut self, block_id: u64, _: u64) -> Option<bool> {
        let set_idx = (block_id % SET as u64) as usize;
        return self.blocks[set_idx].invalidate(block_id);
    }

    fn lookup(&mut self, block_id: u64, ts: u64) -> Option<bool> {
        let set_idx = (block_id % SET as u64) as usize;
        return self.blocks[set_idx].lookup(block_id, ts);
    }

    fn insert(&mut self, block_id: u64, ts: u64, is_modified: bool, increase_touched_count: bool) {
        let set_idx = (block_id % SET as u64) as usize;
        self.blocks[set_idx].insert(block_id, ts, is_modified, increase_touched_count);
    }

    fn dump_snapshot(&self, snapshot_name: &str) {
        let mut file =
            std::fs::File::create(format!("{}/shared_cache.json", snapshot_name)).unwrap();

        let log2_set = SET.trailing_zeros();

        let entries = self
            .blocks
            .iter()
            .map(|entry| {
                let mut sorted_lines: Vec<_> = entry.blocks.iter().collect();
                sorted_lines.sort_by(|a, b| a.ts.cmp(&b.ts));

                sorted_lines
                    .iter()
                    .filter_map(|block| {
                        if block.block_id_with_v & 1 == 0 {
                            return None;
                        }
                        Some(SerializedSharedCacheBlock {
                            tag: (block.block_id_with_v >> 1) >> log2_set,
                            dirty: block.modified,
                            writable: true,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        serde_json::to_writer_pretty(
            &mut file,
            &json!({
                "associativity": WAY,
                "tags": entries,
            }),
        )
        .unwrap();
    }

    fn fold(&self, other: &Self) -> Self {
        return Self {
            blocks: self
                .blocks
                .iter()
                .zip(other.blocks.iter())
                .map(|(a, b)| a.fold(b))
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        };
    }
}

pub struct ReplicatedSharedCache<
    const CORE_COUNT: usize,
    const SET: usize,
    const WAY: usize,
    const EXCLUSIVE: bool,
> {
    blocks: [UnsafeCell<PrivateSharedCache<SET, WAY, EXCLUSIVE>>; CORE_COUNT],
}

impl<const CORE_COUNT: usize, const SET: usize, const WAY: usize, const EXCLUSIVE: bool> SharedCache
    for ReplicatedSharedCache<CORE_COUNT, SET, WAY, EXCLUSIVE>
{
    fn new() -> Self {
        Self {
            blocks: std::array::from_fn(|_| UnsafeCell::new(PrivateSharedCache::new())),
        }
    }

    fn invalidate(&self, core_id: u32, block_id: u64, ts: u64) -> Option<bool> {
        let pcache = unsafe { &mut *self.blocks[core_id as usize].get() };
        return pcache.invalidate(block_id, ts);
    }

    fn lookup(&self, core_id: u32, block_id: u64, ts: u64) -> Option<bool> {
        let pcache = unsafe { &mut *self.blocks[core_id as usize].get() };
        return pcache.lookup(block_id, ts);
    }

    fn insert(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    ) {
        let pcache = unsafe { &mut *self.blocks[core_id as usize].get() };
        pcache.insert(block_id, ts, is_modified, increase_touched_count);
    }

    fn dump_snapshot(&self, snapshot_name: &str) {
        // combine the result from all cores
        let f = self
            .blocks
            .iter()
            .fold(PrivateSharedCache::new(), |acc, x| {
                acc.fold(unsafe { &*x.get() })
            });

        f.dump_snapshot(snapshot_name)
    }

    fn warmed_sets_count(&self) -> usize {
        return 0;
    }

    fn warmed_slots_count(&self) -> usize {
        return 0;
    }
}
