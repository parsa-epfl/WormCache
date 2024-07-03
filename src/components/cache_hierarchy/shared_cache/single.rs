use std::{
    cell::UnsafeCell,
    io::prelude::*,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::components::cache_hierarchy::util::CCell;

use super::{
    statistics::SharedCacheSetStatistics, SerializedSharedCacheBlock,
    SharedCacheLookupAndInsertResult, SharedCacheLookupResult, SharedCacheSet, VTsViolationResult,
};
use serde_json::json;
use spin::mutex::SpinMutex;

pub struct SingleSharedCache<
    S: SharedCacheSetStatistics,
    G: CCell<SharedCacheSet<WAY, EXCLUSIVE, S>> + std::fmt::Debug,
    const SET: usize,
    const WAY: usize,
    const EXCLUSIVE: bool,
    const VTS_MONITORING: bool = false,
> {
    blocks: Box<[G; SET]>,
    warmed_sets: AtomicUsize,
    _phantom: std::marker::PhantomData<S>,
}

impl<
        S: SharedCacheSetStatistics,
        G: CCell<SharedCacheSet<WAY, EXCLUSIVE, S>> + std::fmt::Debug,
        const SET: usize,
        const WAY: usize,
        const EXCLUSIVE: bool,
    > super::SharedCache for SingleSharedCache<S, G, SET, WAY, EXCLUSIVE>
{
    fn new() -> Self {
        Self {
            blocks: crate::util::init_heap_array(|_| (G::new(SharedCacheSet::new()))),
            warmed_sets: AtomicUsize::new(0),
            _phantom: std::marker::PhantomData,
        }
    }

    fn invalidate(&self, _core_id: u32, block_id: u64, ts: u64, _v_ts: u64) -> Option<bool> {
        let set_idx = (block_id % SET as u64) as usize;
        self.blocks[set_idx].inner().invalidate(block_id, ts);
        return self.blocks[set_idx].inner().invalidate(block_id, ts);
    }

    fn lookup(
        &self,
        _core_id: u32,
        block_id: u64,
        ts: u64,
        abandon_dirty: bool,
        access_type: super::CacheAccessType,
        is_os: bool,
    ) -> (SharedCacheLookupResult) {
        let set_idx = (block_id % SET as u64) as usize;

        self.blocks[set_idx]
            .inner()
            .lookup(block_id, ts, abandon_dirty, access_type, is_os)
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
                .inner()
                .insert(block_id, ts, is_modified, increase_touched_count);
        if just_warmed {
            self.warmed_sets.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn lookup_and_insert_on_miss(
        &self,
        _core_id: u32,
        block_id: u64,
        ts: u64,
        abandon_dirty: bool,
        is_store: bool,
        increase_touched_count: bool,
        access_type: super::CacheAccessType,
        is_os: bool,
    ) -> SharedCacheLookupResult {
        let set_idx = (block_id % SET as u64) as usize;
        let result = self.blocks[set_idx].inner().lookup_and_insert(
            block_id,
            ts,
            abandon_dirty,
            is_store,
            increase_touched_count,
            access_type,
            is_os,
        );

        match result {
            SharedCacheLookupAndInsertResult::Hit(is_dirty) => {
                SharedCacheLookupResult::Hit(is_dirty)
            }
            SharedCacheLookupAndInsertResult::Inserted(just_warmed) => {
                if just_warmed {
                    self.warmed_sets.fetch_add(1, Ordering::Relaxed);
                }
                SharedCacheLookupResult::Miss
            }
            SharedCacheLookupAndInsertResult::Unknown(diff) => {
                SharedCacheLookupResult::Unknown(diff)
            }
        }
    }

    fn warmed_sets_count(&self) -> usize {
        self.warmed_sets.load(Ordering::Relaxed)
    }

    fn warmed_slots_count(&self) -> usize {
        self.blocks
            .iter()
            .map(|entry| {
                let entry = entry.inner();
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
                let entry = entry.inner();
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

    fn information() -> String {
        format!(
            "SingleSharedCache: SET={}, WAY={}, EXCLUSIVE={}",
            SET, WAY, EXCLUSIVE
        )
    }

    fn dump_access_frequency(&self, file_name: &str) {
        let mut file = std::fs::File::create(file_name).unwrap();
        writeln!(file, "idx,{}\n", S::get_header()).unwrap();
        for (idx, entry) in self.blocks.iter().enumerate() {
            let entry = entry.inner();
            writeln!(file, "{},{}\n", idx, entry.statistics.render_line()).unwrap();
        }
    }
}

pub type ParallelSingleSharedCache<S, const SET: usize, const WAY: usize, const EXCLUSIVE: bool> =
    SingleSharedCache<S, SpinMutex<SharedCacheSet<WAY, EXCLUSIVE, S>>, SET, WAY, EXCLUSIVE>;

pub type SerialSingleSharedCache<S, const SET: usize, const WAY: usize, const EXCLUSIVE: bool> =
    SingleSharedCache<S, UnsafeCell<SharedCacheSet<WAY, EXCLUSIVE, S>>, SET, WAY, EXCLUSIVE>;
