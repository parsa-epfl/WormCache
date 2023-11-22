use std::collections::HashMap;
use super::ts_cache::TimestampCache;

#[derive(Debug)]
#[repr(align(64))]
pub struct TimestampSingleCoreMemoryHierarchy<
    const P_A: usize, // associativity of the private cache
    const P_S: usize, // set number of the private cache
    const S_A: usize, // associativity of the shared cache
    const S_S: usize, // set number of the shared cache
> {
    pub private_cache: TimestampCache<P_A, P_S>,
    pub local_shared_cache: TimestampCache<S_A, S_S>,
    // All cache invalidation requests. They are used for coherence state construction.
    pub invalid_list: HashMap<usize, usize>, // block_id -> ts
    // All evicted dirty cache line. They are used for coherence state construction.
    pub evicted_dirty_list: HashMap<usize, usize>, // block_id -> ts
}

impl<const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampSingleCoreMemoryHierarchy<P_A, P_S, S_A, S_S>
{
    pub fn new() -> Self {
        return Self {
            private_cache: TimestampCache::new(),
            local_shared_cache: TimestampCache::new(),
            invalid_list: HashMap::new(),
            evicted_dirty_list: HashMap::new(),
        };
    }

    pub fn access_memory(&mut self, ts: usize, paddr: usize, is_instruction: bool, is_store: bool) {
        let block_id = paddr >> crate::CACHE_LINE_SIZE.trailing_zeros();
        let res = self
            .private_cache
            .record(block_id, is_instruction, is_store, ts);
        match res {
            super::CacheReturnResult::Miss => {
                if self
                    .local_shared_cache
                    .peek(block_id, is_instruction, is_store, ts)
                {
                    // If hitting in the shared cache, we move it to the private cache.
                    self.local_shared_cache.invalid(block_id);
                }
            }
            super::CacheReturnResult::Hit => {}
            super::CacheReturnResult::MissWithEviction(blk, is_instruction) => {
                if self
                    .local_shared_cache
                    .peek(block_id, is_instruction, is_store, ts)
                {
                    // If hitting in the shared cache, we move it to the private cache.
                    self.local_shared_cache.invalid(block_id);
                }

                self.local_shared_cache
                    .record(blk, is_instruction, false, ts);
            }
            super::CacheReturnResult::MissWithWriteBack(blk) => {
                if self
                    .local_shared_cache
                    .peek(block_id, is_instruction, is_store, ts)
                {
                    // If hitting in the shared cache, we move it to the private cache.
                    self.local_shared_cache.invalid(block_id);
                }

                self.local_shared_cache.record(blk, false, true, ts);
                self.evicted_dirty_list.insert(block_id, ts);
            }
        }
    }
    // This is a temporal function to measure the number of instructions to warming the cache.
    pub fn clean_local_shared_cache(&mut self) {
        self.local_shared_cache.warmed_count.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

