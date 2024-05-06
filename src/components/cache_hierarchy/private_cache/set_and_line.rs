use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
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
        self.block_id_with_v >> 1
    }

    #[inline]
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    #[inline]
    pub fn has_write_permission(&self) -> bool {
        self.writeable
    }

    #[inline]
    pub fn write_ts(&self) -> u64 {
        self.write_ts
    }

    #[inline]
    pub fn access_ts(&self) -> u64 {
        self.ts
    }

    #[inline]
    pub fn is_instruction(&self) -> bool {
        self.is_instruction
    }
}

// Migrate some functions to this struct, with lock permission.
#[derive(Debug)]
#[repr(align(64))]
pub struct PrivateCacheSet {
    pub lines: Vec<PrivateCacheLine>, // I am still wondering if I should turn its length into constant. After all, it is constant.
    pub touched_count: usize,
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
            touched_count: 0,
        }
    }

    #[inline]
    pub fn index_of(&self, block_id: u64) -> Option<usize> {
        // TODO: Replace this function with SIMD instructions.
        // This requires the following changes:
        // - Aligned data layout for the tags and the ts.
        // - Explicit loop size, which means refactoring the interface of the private cache to the hierarchy.

        let block_id_to_find = (block_id << 1) | 1;

        // find from the cache set with block id.
        let hit_element = self
            .lines
            .iter()
            .position(|p| p.block_id_with_v == block_id_to_find);

        hit_element
    }

    pub fn poke(&self, block_id: u64) -> Option<PrivateCacheLine> {
        let block_id_to_find = (block_id << 1) | 1;

        // find from the cache set with block id.
        let hit_element = self
            .lines
            .iter()
            .find(|p| p.block_id_with_v == block_id_to_find);

        hit_element.cloned()
    }

    // This function check the cache and update the cache if it is a cache hit. Otherwise, it return false.
    pub fn poke_and_update(
        &mut self,
        block_id: u64,
        ts: u64,
        is_store: bool,
        is_instruction_fetch: bool,
    ) -> PrivateCachePokeResult {
        let hit_idx = self.index_of(block_id);

        if let Some(idx) = hit_idx {
            let line = &mut self.lines[idx];
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
            assert!(line.ts <= ts); // This is a strong assumption. (The cache line should be updated with the latest timestamp.
            line.ts = ts;
            line.is_instruction = is_instruction_fetch;
            PrivateCachePokeResult::Hit
        } else {
            PrivateCachePokeResult::Miss
        }
    }

    pub fn fill(
        &mut self,
        block_id: u64,
        ts: u64,
        is_instruction: bool,
        writable: bool,
        modified: bool,
        increase_touched_count: bool,
    ) -> Option<PrivateCacheLine> {
        assert!(ts != 0); // ts should not be 0. 0 is reserved for invalid blocks.

        assert!(self.index_of(block_id).is_none());

        // increase the touched count.
        if increase_touched_count && self.touched_count < self.lines.len() {
            self.touched_count += 1;
        }

        // TODO: This part can be accelerated using SIMD instructions.
        let mut minimal_ts = u64::MAX;
        let mut minimal_index = 0;

        for (index, line) in self.lines.iter_mut().enumerate() {
            if line.ts < minimal_ts {
                minimal_ts = line.ts;
                minimal_index = index;
            }
        }

        let block_id_with_v = (block_id << 1) | 1;

        let res = if (self.lines[minimal_index].block_id_with_v & 0x1) == 1 {
            Some(self.lines[minimal_index].clone())
        } else {
            None
        };

        // Replace.
        self.lines[minimal_index].ts = ts;
        self.lines[minimal_index].block_id_with_v = block_id_with_v;
        self.lines[minimal_index].is_instruction = is_instruction;
        self.lines[minimal_index].writeable = writable;
        self.lines[minimal_index].modified = modified;
        if modified {
            self.lines[minimal_index].write_ts = ts;
        }

        res
    }

    #[inline]
    pub fn invalidate(&mut self, index: usize) {
        self.lines[index].block_id_with_v = 0;
        self.lines[index].ts = 0; // set ts to 0 so that this place will be find by the minimal ts. Good for replacement.
    }

    pub fn request_sharer(&mut self, index: usize, _ts: u64) -> Option<bool> {
        let line = &mut self.lines[index];
        line.writeable = false;
        let res = line.modified;
        line.modified = false;
        Some(res)
    }

    #[inline]
    pub fn invalidate_by_block_id(&mut self, block_id: u64) -> Option<PrivateCacheLine> {
        // find from the cache set with block id.
        let hit_idx = self.index_of(block_id);

        if let Some(idx) = hit_idx {
            let hit_element = &mut self.lines[idx];
            let res = hit_element.clone();
            hit_element.block_id_with_v = 0;
            hit_element.ts = 0; // set ts to 0 so that this place will be find by the minimal ts. Good for replacement.
            Some(res)
        } else {
            // it is possible to see this path. One case is that the cache line is evicted before updating the directory.
            None
        }
    }

    // get a shared copy of the cache line. Return true if the cache line's permission is changed or it is a miss. (Strong contention)
    #[inline]
    pub fn request_sharer_by_block_id(&mut self, block_id: u64, _ts: u64) -> Option<bool> {
        // find from the cache set with block id.
        let hit_idx = self.index_of(block_id);

        if let Some(hit_idx) = hit_idx {
            let hit_element = &mut self.lines[hit_idx];
            hit_element.writeable = false; // remove the write permission.
            let res = hit_element.modified;
            hit_element.modified = false; // this has something to do with the owned state.
            Some(res)
        } else {
            // it is possible to see this path. One case is that the cache line is evicted before updating the directory.
            None
        }
    }

    #[inline]
    pub fn is_fully_touched(&self) -> bool {
        self.touched_count == self.lines.len()
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

#[test]
fn minimum_can_find_invalid() {
    let mut set = PrivateCacheSet::new(8);
    let mut ts = 1;

    // push 8 elements inside.
    for i in 0..8 {
        set.fill(i, ts, false, false, false, true);
        ts += 1;
    }

    assert!(set.is_fully_touched());

    // now, we invalid set 0.
    assert_eq!(
        set.invalidate_by_block_id(0),
        Some(PrivateCacheLine {
            block_id_with_v: 1,
            ts: 1,
            write_ts: 0,
            is_instruction: false,
            writeable: false,
            modified: false,
        })
    );

    // Now if we refill, we will hit the first place.
    assert_eq!(set.fill(9, ts, false, false, false, true), None);

    // And the cache line 0 should be replaced.
    assert_eq!(
        set.lines[0],
        PrivateCacheLine {
            block_id_with_v: 9 << 1 | 1,
            ts: ts,
            write_ts: 0,
            is_instruction: false,
            writeable: false,
            modified: false,
        }
    );

    ts += 1;

    // If we now insert another one, line[1] will be replaced.
    assert_eq!(
        set.fill(10, ts, false, false, false, true),
        Some(PrivateCacheLine {
            block_id_with_v: 1 << 1 | 1,
            ts: 1,
            write_ts: 0,
            is_instruction: false,
            writeable: false,
            modified: false,
        })
    );
}
