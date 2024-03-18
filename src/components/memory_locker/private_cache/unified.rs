use super::{PrivateCacheLine, PrivateCachePokeResult, PrivateCacheSet};
use std::sync::Mutex;

#[repr(align(64))]
pub struct UnifiedPerCorePrivateCache<const SET: usize, const ASSO: usize> {
    cache: Box<[Mutex<PrivateCacheSet<ASSO>>; SET]>,
}

impl<const SET: usize, const ASSO: usize> UnifiedPerCorePrivateCache<SET, ASSO> {
    pub fn new() -> Self {
        Self {
            cache: crate::util::init_heap_array(|_| Mutex::new(PrivateCacheSet::new())),
        }
    }

    pub fn get_set(&self, block_id: u64) -> &Mutex<PrivateCacheSet<ASSO>> {
        let set_id = block_id as usize % SET;
        return &self.cache[set_id];
    }

    // Interface for testing.
    pub fn contains_block(&self, block_id: u64) -> bool {
        let set_id = block_id as usize % SET;
        let set = self.cache[set_id].lock().unwrap();
        return set.poke(block_id).is_some();
    }

    pub fn is_block_modified(&self, block_id: u64) -> bool {
        let set_id = block_id as usize % SET;
        let set = self.cache[set_id].lock().unwrap();
        let line = set.poke(block_id);
        if let Some(line) = line {
            return line.modified;
        } else {
            return false;
        }
    }
}

pub struct UnifiedPrivateCache {}
