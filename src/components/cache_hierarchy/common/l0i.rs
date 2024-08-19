use std::cell::UnsafeCell;

use crate::parameter::CACHE_LINE_SIZE;

#[repr(align(64))]
struct PerCoreL0 {
    v_block_id: u64,
}

impl PerCoreL0 {
    pub fn new() -> Self {
        Self { v_block_id: 0 }
    }
}

impl Default for PerCoreL0 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct L0InstructionCache<const CORE_COUNT: usize> {
    content: [UnsafeCell<PerCoreL0>; CORE_COUNT],
}

impl<const CORE_COUNT: usize> L0InstructionCache<CORE_COUNT> {
    pub fn new() -> Self {
        Self {
            content: std::array::from_fn(|_| UnsafeCell::new(PerCoreL0::new())),
        }
    }

    // return true if the vcache line is the same as the previous one.
    pub fn check_and_update(&self, core_id: u32, vaddr: u64) -> bool {
        let vcache_line = vaddr >> CACHE_LINE_SIZE.trailing_zeros();
        let core_l0 = unsafe { &mut *self.content[core_id as usize].get() };

        let res = core_l0.v_block_id == vcache_line;

        core_l0.v_block_id = vcache_line;

        res
    }
}

impl<const CORE_COUNT: usize> Default for L0InstructionCache<CORE_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_l0_instruction_cache_check_and_update() {
        let l0_cache: L0InstructionCache<4> = L0InstructionCache::new();
        assert!(!l0_cache.check_and_update(0, 1 << CACHE_LINE_SIZE.trailing_zeros()));
        assert!(l0_cache.check_and_update(0, 1 << CACHE_LINE_SIZE.trailing_zeros()));
        assert!(!l0_cache.check_and_update(0, 2 << CACHE_LINE_SIZE.trailing_zeros()));
    }
}
