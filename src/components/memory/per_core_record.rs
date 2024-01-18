use std::collections::HashMap;

use super::ts_cache::TimestampCache;
use super::mmu::AbstractMMU;
use super::mmu::MemoryManagementUnit;
use crate::arch::AArch64;


use crate::parameter as param;

#[derive(Debug)]
#[repr(align(64))]
pub struct TimestampSingleCoreMemoryHierarchy<
    MMU: AbstractMMU,
    const P_A: usize, // associativity of the private cache
    const P_S: usize, // set number of the private cache
    const S_A: usize, // associativity of the shared cache
    const S_S: usize, // set number of the shared cache
> {
    pub mmu: MMU,
    pub private_cache: TimestampCache<P_A, P_S>,
    pub local_shared_cache: TimestampCache<S_A, S_S>,
    // All cache invalidation requests. They are used for coherence state construction.
    pub invalid_list: HashMap<u64, usize>, // block_id -> ts
    // All evicted dirty cache line. They are used for coherence state construction.
    pub evicted_dirty_list: HashMap<u64, usize>, // block_id -> ts
}

impl<MMU: AbstractMMU, const P_A: usize, const P_S: usize, const S_A: usize, const S_S: usize>
    TimestampSingleCoreMemoryHierarchy<MMU, P_A, P_S, S_A, S_S>
{
    pub fn new() -> Self {
        return Self {
            mmu: MMU::new(),
            private_cache: TimestampCache::new(),
            local_shared_cache: TimestampCache::new(),
            invalid_list: HashMap::new(),
            evicted_dirty_list: HashMap::new(),
        };
    }

    pub fn access_memory(&mut self, ts: usize, vaddr: u64, is_instruction: bool, is_store: bool) {
        // step 1: translation the VA to the PA. 
        let vpn = vaddr >> 12;
        let translation = unsafe { self.mmu.translate_and_refill(vpn as u64, ts as u64) };
        // step 2: if the translation is a miss, we need to replay the trace of accessing physical memory.
        match translation {
            super::mmu::MMUTranslationResult::Hit(ppn) => {
                let paddr = (ppn << 12) | (vaddr & 0xfff);
                self.access_memory_with_pa(ts, paddr, is_instruction, is_store);
            }
            super::mmu::MMUTranslationResult::Miss(ppn, walk_trace) => {
                // replay the trace.
                let paddr = (ppn << 12) | (vaddr & 0xfff);
                for pa in walk_trace {
                    self.access_memory_with_pa(ts, pa, false, false);
                }
                self.access_memory_with_pa(ts, paddr, is_instruction, is_store);
            }
            super::mmu::MMUTranslationResult::MissNotCacheable(ppn) => {
                let paddr = (ppn << 12) | (vaddr & 0xfff);
                self.access_memory_with_pa(ts, paddr, is_instruction, is_store);
            }
        }
    }

    pub fn access_memory_with_pa(&mut self, ts: usize, paddr: u64, is_instruction: bool, is_store: bool) {
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

    pub fn invalidate(&mut self, block_id: u64, ts: usize) {
        self.private_cache.invalid(block_id);
        self.local_shared_cache.invalid(block_id);
        self.invalid_list.insert(block_id, ts);
        self.evicted_dirty_list.insert(block_id, ts);
    }

    pub fn get_written_back_dirty_list(&self) -> &HashMap<u64, usize> {
        return &self.evicted_dirty_list;
    }

    pub fn get_invalid_list(&self) -> &HashMap<u64, usize> {
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
