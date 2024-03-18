#[derive(Debug, Clone, Copy)]
pub struct PrivateCacheLine {
    tag: u64, // the last bit is the valid bit.
    ts: u64,
    write_ts: u64,
    is_instruction: bool,
    modified: bool, // This is false also means that you cannot write to this cache line. There is no need to maintain a Clean and writable state. This state can be inferred by the coherence protocol.
}

impl PrivateCacheLine {
    #[inline]
    pub fn block_id(&self) -> u64 {
        return self.tag >> 1;
    }

    #[inline]
    pub fn is_modified(&self) -> bool {
        return self.modified;
    }

    #[inline]
    pub fn write_ts(&self) -> u64 {
        return self.write_ts;
    }

    #[inline]
    pub fn access_ts(&self) -> u64 {
        return self.ts;
    }

    #[inline]
    pub fn is_instruction(&self) -> bool {
        return self.is_instruction;
    }
}

// Migrate some functions to this struct, with lock permission.
#[derive(Debug)]
pub struct PrivateCacheSet {
    pub lines: Vec<PrivateCacheLine>, // I am still wondering if I should turn its length into constant. After all, it is constant.
}

#[derive(PartialEq, Eq)]
pub enum PrivateCachePokeResult {
    Hit,
    Miss,
    PermissionViolation,
}

impl PrivateCacheSet {
    pub fn new(asso: usize) -> Self {
        Self {
            lines: Vec::from_iter(
                std::iter::repeat(PrivateCacheLine {
                    tag: 0,
                    ts: 0,
                    write_ts: 0,
                    is_instruction: false,
                    modified: false,
                })
                .take(asso),
            ),
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
                    // This should be not possible. You can never refill a cache line using the old timestamp from the same core.
                    assert!(res.ts <= ts);
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
