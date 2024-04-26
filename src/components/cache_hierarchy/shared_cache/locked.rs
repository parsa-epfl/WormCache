use std::sync::atomic::{AtomicUsize, Ordering};

use super::{SerializedSharedCacheBlock, SharedCacheSet};
use serde_json::json;
use spin::mutex::SpinMutex;

pub struct LockedSharedCache<const SET: usize, const WAY: usize, const EXCLUSIVE: bool> {
    blocks: Box<[SpinMutex<SharedCacheSet<WAY, EXCLUSIVE>>; SET]>,
    warmed_sets: AtomicUsize,
}

impl<const SET: usize, const WAY: usize, const EXCLUSIVE: bool> super::SharedCache
    for LockedSharedCache<SET, WAY, EXCLUSIVE>
{
    fn new() -> Self {
        Self {
            blocks: crate::util::init_heap_array(|_| SpinMutex::new(SharedCacheSet::new())),
            warmed_sets: AtomicUsize::new(0),
        }
    }

    fn invalidate(&self, _core_id: u32, block_id: u64, _ts: u64) -> Option<bool> {
        let set_idx = (block_id % SET as u64) as usize;
        return self.blocks[set_idx].lock().invalidate(block_id);
    }

    fn lookup(&self, _core_id: u32, block_id: u64, ts: u64) -> Option<bool> {
        let set_idx = (block_id % SET as u64) as usize;
        return self.blocks[set_idx].lock().lookup(block_id, ts);
    }

    fn insert(
        &self,
        _core_id: u32,
        block_id: u64,
        ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    ) {
        let set_idx = (block_id % SET as u64) as usize;
        let just_warmed =
            self.blocks[set_idx]
                .lock()
                .insert(block_id, ts, is_modified, increase_touched_count);
        if just_warmed {
            self.warmed_sets.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn warmed_sets_count(&self) -> usize {
        self.warmed_sets.load(Ordering::Relaxed)
    }

    fn warmed_slots_count(&self) -> usize {
        self.blocks
            .iter()
            .map(|entry| {
                let entry = entry.lock();
                entry.touched_count
            })
            .sum()
    }

    fn dump_snapshot(&self, snapshot_name: &str) {
        let mut file =
            std::fs::File::create(format!("{}/shared_cache.json", snapshot_name)).unwrap();

        let log2_set = SET.trailing_zeros();

        let entries = self
            .blocks
            .iter()
            .map(|entry| {
                let entry = entry.lock();
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
}
