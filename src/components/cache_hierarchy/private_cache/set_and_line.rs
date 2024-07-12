use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateCacheLine {
    block_id_with_v: u64, // the last bit is the valid bit.
    ts: u64,
    write_ts: u64,
    is_instruction: bool,
    writeable: bool,
    modified: bool,   // TODO: modified can be combined with write_ts.
    access_v_ts: u64, // The timestamp is generated assuming all instructions take exactly 1ns.
    write_v_ts: u64,
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

    #[inline]
    pub fn access_virtual_timestamp(&self) -> u64 {
        self.access_v_ts
    }

    #[inline]
    pub fn write_virtual_timestamp(&self) -> u64 {
        self.write_v_ts
    }
}

// Migrate some functions to this struct, with lock permission.
#[derive(Debug)]
#[repr(align(64))]
pub struct PrivateCacheSet {
    pub lines: Vec<PrivateCacheLine>, // I am still wondering if I should turn its length into constant. After all, it is constant.
    pub touched_count: usize,
    pub recent_invalid_slot_index: Option<usize>,

    pub hit_time: usize,
    pub hit_index_acc: usize,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum EvictedSlot {
    Invalid(usize),
    Valid(usize, u64),
    Same(usize), // The same hit slot is used for the eviction. This only happens when there is a permission violation.
}

impl EvictedSlot {
    pub fn get_slot_index(&self) -> usize {
        match self {
            EvictedSlot::Invalid(idx) => *idx,
            EvictedSlot::Valid(idx, _) => *idx,
            EvictedSlot::Same(idx) => *idx,
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum PrivateCachePokeResult {
    Hit,
    Miss(EvictedSlot), // the potential element for eviction
    PermissionViolation(EvictedSlot),
}

impl PrivateCachePokeResult {
    pub fn permission_violation(&self) -> bool {
        matches!(self, PrivateCachePokeResult::PermissionViolation(_))
    }
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
                    access_v_ts: 0,
                    write_v_ts: 0,
                })
                .take(asso),
            ),
            touched_count: 0,
            recent_invalid_slot_index: None,

            hit_time: 0,
            hit_index_acc: 0,
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

    #[inline]
    pub fn find_eviction_index(&self) -> EvictedSlot {
        // TODO: This part can be accelerated using SIMD instructions.
        let mut minimal_ts = u64::MAX;
        let mut minimal_index = 0;

        for (index, line) in self.lines.iter().enumerate() {
            if line.ts < minimal_ts {
                minimal_ts = line.ts;
                minimal_index = index;
            }
        }

        if minimal_ts == 0 {
            // there is one invalid slot.
            EvictedSlot::Invalid(minimal_index)
        } else {
            EvictedSlot::Valid(minimal_index, self.lines[minimal_index].block_id())
        }
    }

    #[inline]
    pub fn poke(&self, block_id: u64) -> Option<PrivateCacheLine> {
        let block_id_to_find = (block_id << 1) | 1;

        // find from the cache set with block id.
        let hit_element = self
            .lines
            .iter()
            .find(|p| p.block_id_with_v == block_id_to_find);

        hit_element.cloned()
    }

    #[inline]
    // This function check the cache and update the cache if it is a cache hit. Otherwise, it return false.
    pub fn poke_and_update(
        &mut self,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        is_store: bool,
        is_instruction_fetch: bool,
    ) -> PrivateCachePokeResult {
        // clean the guard of the recent slot so that it can be used for checking whether there is an invalidation from other core.
        self.recent_invalid_slot_index = None;

        let hit_idx = self.index_of(block_id);

        if let Some(idx) = hit_idx {
            let line = &mut self.lines[idx];
            if is_store {
                if line.writeable {
                    line.ts = ts;
                    line.write_ts = ts;
                    line.modified = true;
                    line.access_v_ts = v_ts;
                    line.write_v_ts = v_ts;
                    return PrivateCachePokeResult::Hit;
                } else {
                    return PrivateCachePokeResult::PermissionViolation(EvictedSlot::Same(idx));
                }
            }
            assert!(line.ts <= ts); // This is a strong assumption. (The cache line should be updated with the latest timestamp.
            line.ts = ts;
            line.access_v_ts = v_ts;
            line.is_instruction = is_instruction_fetch;

            self.hit_index_acc += idx;
            self.hit_time += 1;

            PrivateCachePokeResult::Hit
        } else {
            PrivateCachePokeResult::Miss(self.find_eviction_index())
        }
    }

    // This function should be use in pair with `poke_and_update`.
    // Return Some if it evicts an valid and different cache line, and the content is the modified bit of the evicted cache line.
    // Return None if it does not evict any valid cache line.
    #[inline]
    pub fn fill_with_potential_eviction_slot(
        &mut self,
        potential_slot: EvictedSlot,
        block_id: u64,
        ts: u64,
        v_ts: u64,
        is_instruction: bool,
        writable: bool,
        modified: bool,
    ) -> Option<(bool, u64)> {
        // (modified, write_ts)
        assert!(ts != 0); // ts should not be 0. 0 is reserved for invalid blocks.

        assert!(self.index_of(block_id).is_none());

        // increase the touched count.
        if !matches!(potential_slot, EvictedSlot::Same(_)) && self.touched_count < self.lines.len()
        {
            self.touched_count += 1;
        }

        let (res, idx_of_slot_to_fill) = match potential_slot {
            EvictedSlot::Invalid(idx) => (None, idx),
            EvictedSlot::Valid(idx, _) => match self.recent_invalid_slot_index {
                Some(idx) => (None, idx),
                None => (
                    Some((self.lines[idx].modified, self.lines[idx].write_ts)),
                    idx,
                ),
            },
            EvictedSlot::Same(idx) => {
                // This is actually an upgrade, not a cache fill.
                assert!(self.recent_invalid_slot_index.is_some());

                // Usually, the recent_invalid_slot_index should be identical to the idx.

                // It is possible that this index has been evicted by other cores?
                // No. The only core that can cause eviction is the core itself.

                // Is it possible that this cache line is invalidated by others?
                // Yes. It is possible.
                // Is it possible that multiple invalidation happens between the peek and the fill?
                // Yes. It is possible.

                // Under the following case, the recent_invalid_slot_index does not have to be identical to the idx.
                // - The cache line is peeked, and it lacks memory operation.
                // - Another core invalidates this cache line, with a past timestamp.
                // - A third core invalidates another cache line (and update the recent_invalid_slot_index).
                // - The cache line now is filled. The recent_invalid_slot_index is not identical to the idx.

                // As a result, if recent_invalid_slot_index is not equal to the idx, the idx must be invalid.

                if self.recent_invalid_slot_index.unwrap() != idx {
                    assert_eq!(self.lines[idx].block_id_with_v & 0x1, 0);
                    assert_eq!(self.lines[idx].ts, 0);
                }

                (None, idx)
            }
        };

        // take the guard
        self.recent_invalid_slot_index = None;

        let block_id_with_v = (block_id << 1) | 1;

        // fill.
        assert!(self.lines[idx_of_slot_to_fill].ts <= ts); // Timestamp of each core should be monotonic.

        // Replace.
        self.lines[idx_of_slot_to_fill].ts = ts;
        self.lines[idx_of_slot_to_fill].block_id_with_v = block_id_with_v;
        self.lines[idx_of_slot_to_fill].is_instruction = is_instruction;
        self.lines[idx_of_slot_to_fill].writeable = writable;
        self.lines[idx_of_slot_to_fill].modified = modified;
        self.lines[idx_of_slot_to_fill].access_v_ts = v_ts;
        if modified {
            self.lines[idx_of_slot_to_fill].write_ts = ts;
            self.lines[idx_of_slot_to_fill].write_v_ts = v_ts;
        } else {
            self.lines[idx_of_slot_to_fill].write_ts = 0; // no one has written this cache line, so its timestamp should be zero.
            self.lines[idx_of_slot_to_fill].write_v_ts = 0;
        }

        res
    }

    #[inline]
    pub fn invalidate(&mut self, index: usize) {
        self.lines[index].block_id_with_v = 0;
        self.lines[index].ts = 0; // set ts to 0 so that this place will be find by the minimal ts. Good for replacement.
        self.lines[index].access_v_ts = 0;
        self.recent_invalid_slot_index = Some(index);
    }

    #[inline]
    pub fn request_sharer(&mut self, index: usize, _ts: u64) -> Option<bool> {
        // This function should not upgrade the timestamp of the cache line, because it can change the eviction target here.
        let line = &mut self.lines[index];
        line.writeable = false;
        let res = line.modified;
        line.modified = false;
        Some(res)
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
        set.fill_with_potential_eviction_slot(
            EvictedSlot::Invalid(i),
            i as u64,
            ts,
            ts,
            false,
            false,
            false,
        );
        ts += 1;
    }

    assert!(set.is_fully_touched());

    // now, we invalid set 0.
    let idx = set.index_of(0).unwrap();
    assert_eq!(
        set.lines[idx],
        PrivateCacheLine {
            block_id_with_v: 1,
            ts: 1,
            access_v_ts: 1,
            write_ts: 0,
            write_v_ts: 0,
            is_instruction: false,
            writeable: false,
            modified: false,
        }
    );

    set.invalidate(idx);

    // Now if we refill, we will hit the first place.
    let evict_slot = set.find_eviction_index();
    assert_eq!(evict_slot, EvictedSlot::Invalid(0));
    set.fill_with_potential_eviction_slot(evict_slot, 9, ts, ts, false, false, false);

    // And the cache line 0 should be replaced.
    assert_eq!(
        set.lines[0],
        PrivateCacheLine {
            block_id_with_v: 9 << 1 | 1,
            ts: ts,
            access_v_ts: ts,
            write_ts: 0,
            write_v_ts: 0,
            is_instruction: false,
            writeable: false,
            modified: false,
        }
    );

    ts += 1;

    // If we now insert another one, line[1] will be replaced.
    let evict_slot = set.find_eviction_index();
    assert_eq!(evict_slot, EvictedSlot::Valid(1, 1));
    set.fill_with_potential_eviction_slot(evict_slot, 10, ts, ts, false, false, false);
}
