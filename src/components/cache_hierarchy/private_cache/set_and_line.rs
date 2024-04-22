use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct PrivateCacheLine {
    block_id_with_v: u64, // the last bit is the valid bit.
    ts: u64,
    write_ts: u64,
    is_instruction: bool,
    writeable: bool,
    modified: bool,
}

impl PrivateCacheLine {
    #[inline]
    pub fn block_id(&self) -> u64 {
        return self.block_id_with_v >> 1;
    }

    #[inline]
    pub fn is_modified(&self) -> bool {
        return self.modified;
    }

    #[inline]
    pub fn has_write_permission(&self) -> bool {
        return self.writeable;
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
#[repr(align(64))]
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
                    block_id_with_v: 0,
                    ts: 0,
                    write_ts: 0,
                    is_instruction: false,
                    writeable: false,
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
            return p.block_id_with_v == block_id_to_find;
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
            return p.block_id_with_v == block_id_to_find;
        });

        if let Some(line) = hit_element {
            if is_store {
                if line.writeable {
                    line.ts = ts;
                    line.write_ts = ts;
                    line.modified = true;
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
        writable: bool,
        modified: bool,
    ) -> Option<PrivateCacheLine> {
        let block_id_to_find = (block_id << 1) | 1;

        // find from the cache set with block id.
        let hit_element = self.lines.iter_mut().find(|p| {
            return p.block_id_with_v == block_id_to_find;
        });

        assert!(hit_element.is_none());
        // find the first invalid element.
        let invalid_element = self.lines.iter_mut().find(|p| {
            return (p.block_id_with_v & 0x1) == 0;
        });

        if let Some(invalid_element) = invalid_element {
            invalid_element.ts = ts;
            invalid_element.block_id_with_v = block_id_to_find;
            invalid_element.is_instruction = is_instruction;
            invalid_element.writeable = writable;
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
                    oldest_element.block_id_with_v = block_id_to_find;
                    oldest_element.is_instruction = is_instruction;
                    oldest_element.writeable = writable;
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
            return p.block_id_with_v == block_id_to_find;
        });

        if let Some(hit_element) = hit_element {
            let res = hit_element.clone();
            hit_element.block_id_with_v = 0;
            return Some(res);
        } else {
            // it is possible to see this path. One case is that the cache line is evicted before updating the directory.
            return None;
        }
    }

    // get a shared copy of the cache line. Return true if the cache line's permission is changed or it is a miss. (Strong contention)
    pub fn request_sharer(&mut self, block_id: u64, ts: u64) -> Option<bool> {
        let block_id_to_find = (block_id << 1) | 1;
        // find from the cache set with block id.
        let hit_element = self.lines.iter_mut().find(|p| {
            return p.block_id_with_v == block_id_to_find;
        });

        if let Some(hit_element) = hit_element {
            hit_element.writeable = false; // remove the write permission.
            let res = hit_element.modified;
            hit_element.modified = false; // this has something to do with the owned state.
            return Some(res);
        } else {
            // it is possible to see this path. One case is that the cache line is evicted before updating the directory.
            return None;
        }
    }
}

// Serialization function of the cache line.

#[derive(Serialize)]
pub struct SerializedCacheLine {
    tag: u64,
    writable: bool,
    dirty: bool,
}

impl PrivateCacheSet {
    pub fn serialize(&self, number_of_set: usize) -> Vec<SerializedCacheLine> {
        // 1. sort the line by its timestamp. smaller timestamp goes first
        // 2. filter out the invalid lines.
        // 3. tag should be removed with the valid bit and the index bit.

        let mut sorted_lines = self.lines.clone();
        sorted_lines.sort_by(|a, b| a.ts.cmp(&b.ts));

        let set_bits = (number_of_set as u64).trailing_zeros();

        return sorted_lines
            .iter()
            .filter(|line| line.block_id_with_v & 0x1 == 1)
            .map(|line| SerializedCacheLine {
                tag: (line.block_id_with_v >> 1) >> set_bits,
                writable: line.modified,
                dirty: line.modified,
            })
            .collect();
    }
}
