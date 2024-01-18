use std::collections::HashMap;

use super::ts_cache::TimestampCache;
use super::tlb::TLB;

use crate::parameter as param;

#[derive(Debug)]
#[repr(align(64))]
pub struct TimestampSingleCoreMemoryHierarchy<
    const T_A: usize, // associativity of the TLB
    const T_S: usize, // set number of the TLB
    const P_A: usize, // associativity of the private cache
    const P_S: usize, // set number of the private cache
    const S_A: usize, // associativity of the shared cache
    const S_S: usize, // set number of the shared cache
> {

    pub tlb: TLB<T_S, T_A>, 
    pub private_cache: TimestampCache<P_A, P_S>,
    pub local_shared_cache: TimestampCache<S_A, S_S>,
    // All cache invalidation requests. They are used for coherence state construction.
    pub invalid_list: HashMap<usize, usize>, // block_id -> ts
    // All evicted dirty cache line. They are used for coherence state construction.
    pub evicted_dirty_list: HashMap<usize, usize>, // block_id -> ts
}

impl<const T_A: usize, const T_S: usize, const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampSingleCoreMemoryHierarchy<T_A, T_S, P_A, P_S, S_A, S_S>
{
    pub fn new() -> Self {
        return Self {
            tlb: TLB::new(),
            private_cache: TimestampCache::new(),
            local_shared_cache: TimestampCache::new(),
            invalid_list: HashMap::new(),
            evicted_dirty_list: HashMap::new(),
        };
    }

    fn access_memory(&mut self, ts: usize, vaddr: usize, is_instruction: bool, is_store: bool) {

    }

    fn access_memory_with_pa(&mut self, ts: usize, paddr: usize, is_instruction: bool, is_store: bool) {
        let block_id = paddr >> param::CACHE_LINE_SIZE.trailing_zeros();
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
                    .record(blk, is_instruction, false, super::get_memory_ts() as usize);
            }
            super::CacheReturnResult::MissWithWriteBack(blk) => {
                if self
                    .local_shared_cache
                    .peek(block_id, is_instruction, is_store, ts)
                {
                    // If hitting in the shared cache, we move it to the private cache.
                    self.local_shared_cache.invalid(block_id);
                }

                self.local_shared_cache.record(blk, false, true, super::get_memory_ts() as usize);
                self.evicted_dirty_list.insert(block_id, ts);
            }
        }
    }

    pub fn invalidate(&mut self, block_id: usize, ts: usize) {
        self.private_cache.invalid(block_id);
        self.local_shared_cache.invalid(block_id);
        self.invalid_list.insert(block_id, ts);
        self.evicted_dirty_list.insert(block_id, ts);
    }

    pub fn get_written_back_dirty_list(&self) -> &HashMap<usize, usize> {
        return &self.evicted_dirty_list;
    }

    pub fn get_invalid_list(&self) -> &HashMap<usize, usize> {
        return &self.invalid_list;
    }

    pub fn clear_written_back_dirty_list(&mut self) {
        self.evicted_dirty_list.clear();
    }

    // This is a temporal function to measure the number of instructions to warming the cache.
    pub fn clean_local_shared_cache(&self) {
        self.local_shared_cache.sets.iter().for_each(|set| {
            set.clean();
        });
    }
}
