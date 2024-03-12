use std::sync::RwLock;

#[derive(Debug, Clone, Copy)]
pub struct PrivateCacheLine {
    pub tag: u64, // the last bit is the valid bit.
    pub ts: u64,
    pub write_ts: u64,
    pub is_instruction: bool,
    pub modified: bool, // This is false also means that you cannot write to this cache line. There is no need to maintain a Clean and writable state. This state can be inferred by the coherence protocol.
}

impl PrivateCacheLine {
    pub fn block_id(&self) -> u64 {
        return self.tag >> 1;
    }
}

// Migrate some functions to this struct, with lock permission.
#[derive(Debug)]
pub struct PrivateCacheSet<const WAY: usize> {
    pub lines: [PrivateCacheLine; WAY],
}

#[derive(PartialEq, Eq)]
pub enum PrivateCachePokeResult {
    Hit,
    Miss,
    PermissionViolation,
}

impl<const WAY: usize> PrivateCacheSet<WAY> {
    pub fn new() -> Self {
        Self {
            lines: [PrivateCacheLine {
                tag: 0,
                ts: 0,
                write_ts: 0,
                is_instruction: false,
                modified: false,
            }; WAY],
        }
    }

    pub fn poke(&self, block_id: u64) -> Option<PrivateCacheLine> {
        let block_id_to_find = (block_id << 1) | 1;

        // find from the cache set with block id.
        let hit_element = self.lines.iter().find(|p| {
            return p.tag == block_id_to_find;
        });

        if let Some(hit_element) = hit_element {
            return Some(hit_element.clone());
        } else {
            // it is possible to see this path. One case is that the cache line is evicted before updating the directory.
            return None;
        }
    }

    // This function check the cache and update the cache if it is a cache hit. Otherwise, it return false.
    pub fn poke_and_update(
        &mut self,
        block_id: u64,
        ts: u64,
        is_store: bool,
        is_instruction_fetch: bool,
    ) -> PrivateCachePokeResult {
        let block_id_to_find = (block_id << 1) | 1;

        let hit_element = self.lines.iter_mut().find(|p| {
            return p.tag == block_id_to_find;
        });

        if let Some(line) = hit_element {
            if is_store {
                if line.modified {
                    line.ts = ts;
                    line.write_ts = ts;
                    return PrivateCachePokeResult::Hit;
                } else {
                    return PrivateCachePokeResult::PermissionViolation;
                }
            }
            line.ts = ts;
            line.is_instruction = is_instruction_fetch;
            return PrivateCachePokeResult::Hit;
        } else {
            return PrivateCachePokeResult::Miss;
        }
    }

    pub fn refill(
        &mut self,
        block_id: u64,
        ts: u64,
        is_instruction: bool,
        modified: bool,
    ) -> Option<PrivateCacheLine> {
        let block_id_to_find = (block_id << 1) | 1;

        // find from the cache set with block id.
        let hit_element = self.lines.iter_mut().find(|p| {
            return p.tag == block_id_to_find;
        });

        assert!(hit_element.is_none());
        // find the first invalid element.
        let invalid_element = self.lines.iter_mut().find(|p| {
            return (p.tag & 0x1) == 0;
        });

        if let Some(invalid_element) = invalid_element {
            invalid_element.ts = ts;
            invalid_element.tag = block_id_to_find;
            invalid_element.is_instruction = is_instruction;
            invalid_element.modified = modified;
            if modified {
                invalid_element.write_ts = ts;
            }
            return None;
        } else {
            // find the oldest element.
            let oldest_element = self.lines.iter_mut().min_by(|p, q| {
                return p.ts.cmp(&q.ts);
            });

            match oldest_element {
                Some(oldest_element) => {
                    // Here we need to be careful. In case we have order violation, we don't know the result of this cache hit / miss.
                    let res = oldest_element.clone();
                    oldest_element.ts = ts;
                    oldest_element.tag = block_id_to_find;
                    oldest_element.is_instruction = is_instruction;
                    oldest_element.modified = modified;
                    if modified {
                        oldest_element.write_ts = ts;
                    }
                    return Some(res);
                }
                None => {
                    unreachable!("PrivateCache::insert: no element in the cache set.");
                }
            }
        }
    }

    pub fn invalidate(&mut self, block_id: u64) -> Option<PrivateCacheLine> {
        let block_id_to_find = (block_id << 1) | 1;

        // find from the cache set with block id.
        let hit_element = self.lines.iter_mut().find(|p| {
            return p.tag == block_id_to_find;
        });

        if let Some(hit_element) = hit_element {
            let res = hit_element.clone();
            hit_element.tag = 0;
            return Some(res);
        } else {
            // it is possible to see this path. One case is that the cache line is evicted before updating the directory.
            return None;
        }
    }

    // get a shared copy of the cache line. Return true if the cache line's permission is changed or it is a miss. (Strong contention)
    pub fn request_sharer(&mut self, block_id: u64, ts: u64) -> bool {
        let block_id_to_find = (block_id << 1) | 1;
        // find from the cache set with block id.
        let hit_element = self.lines.iter_mut().find(|p| {
            return p.tag == block_id_to_find;
        });

        if let Some(hit_element) = hit_element {
            hit_element.modified = false; // remove the write permission.
            return true;
        } else {
            // it is possible to see this path. One case is that the cache line is evicted before updating the directory.
            return false;
        }
    }
}

#[repr(align(64))]
pub struct PrivateCache<const SET: usize, const WAY: usize> {
    cache: Box<[RwLock<PrivateCacheSet<WAY>>; SET]>,
}

impl<const SET: usize, const WAY: usize> PrivateCache<SET, WAY> {
    pub fn new() -> Self {
        Self {
            cache: crate::util::init_heap_array(|_| RwLock::new(PrivateCacheSet::new())),
        }
    }

    pub fn get_set(&self, block_id: u64) -> &RwLock<PrivateCacheSet<WAY>> {
        let set_id = block_id as usize % SET;
        return &self.cache[set_id];
    }
}

// There might be another way to design the private cache.
// - No locks for each set.
// - Each set has a ring buffer for the incoming invalidation request from other cores.
// - Before accessing each set, empty the ring buffer, which only requires pure atomic operations.
//   - the ring buffer is a fixed-size array, which has at most ASSO elements.
//   - accessing ring buffer is a pure read operations, including the read pointer
//   - pushing message to the ring buffer is an atomic add operation + a write operation.
// - A mutex is necessary for the directory when there is a private cache miss (it is really nice if we can take away this lock)
//   - coherence miss: Write lock, to clean others
//   - capacity/conflict miss, depending on the condition of the directory (rlock)
//        - The cache line is in others' private cache: write lock
//        - The cache line is in the shared cache: write lock, to create a new entry.
// - The shared LLC requires a lock for each set when the LLC is large, and can be replicated when the LLC is small to avoid contention.
