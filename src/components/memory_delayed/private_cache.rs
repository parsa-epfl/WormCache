use crossbeam_queue::SegQueue;
use std::collections::HashMap;

mod mfifo;
pub use mfifo::*;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PrivateCacheState {
    Invalid,
    CleanShared,
    DirtyShared,
    CleanExclusive,
    DirtyExclusive,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PrivateCacheLine {
    pub state: PrivateCacheState,
    pub tag: u64,
    pub ts: u64,
    pub is_instruction: bool,
}

// Migrate some functions to this struct, with lock permission.
#[derive(Debug)]
pub struct PrivateCacheSet<const WAY: usize> {
    lines: [PrivateCacheLine; WAY],
    invalidation_fifo: SegQueue<(u64, u64, MessageType)>,
    invalidation_entries: HashMap<u64, u64>, // block id -> ts_invalid. Ts is the time when the block is invalid due to coherence.
                                             // If one element appears in `invalid_entries`, it must be invalidated by others.
                                             // If the current core finds an element in this list but not in its own cache, we can compare the timestamp.
                                             // If the access is earlier than the invalidation, it must be in the cache. After all, the access must be later than the refill, which is done by the current core itself.
}

impl<const WAY: usize> PrivateCacheSet<WAY> {
    pub fn new() -> Self {
        Self {
            lines: [PrivateCacheLine {
                state: PrivateCacheState::Invalid,
                tag: 0,
                ts: 0,
                is_instruction: false,
            }; WAY],
            invalidation_fifo: SegQueue::new(),
            invalidation_entries: HashMap::new(),
        }
    }

    #[inline]
    fn handle_message(&mut self) {
        // most of the case, this branch is not taken.
        if self.invalidation_fifo.is_empty() {
            return;
        }

        // if not, we have to handle it carefully.
        while let Some((block_id, ts, message_type)) = self.invalidation_fifo.pop() {
            // find from the cache set with block id.
            let hit_element = self
                .lines
                .iter_mut()
                .find(|p| p.tag == block_id && p.state != PrivateCacheState::Invalid);

            if let Some(hit_element) = hit_element {
                match message_type {
                    MessageType::Invalidate => {
                        if hit_element.state != PrivateCacheState::Invalid {
                            hit_element.state = PrivateCacheState::Invalid;
                            self.add_invalidation_record(block_id, ts);
                        }
                    }
                    MessageType::CreateSharer => match hit_element.state {
                        PrivateCacheState::Invalid => {}
                        PrivateCacheState::CleanShared => {}
                        PrivateCacheState::DirtyShared => {}
                        PrivateCacheState::CleanExclusive => {
                            hit_element.state = PrivateCacheState::CleanShared;
                        }
                        PrivateCacheState::DirtyExclusive => {
                            hit_element.state = PrivateCacheState::DirtyShared;
                        }
                    },
                }
            } else {
                // it is possible to see this path. One case is that the cache line is evicted before updating the directory.
            }
        }
    }

    #[inline]
    // When there is an invalidation message due to coherence, we need to keep its time being invalid.
    fn add_invalidation_record(&mut self, block_id: u64, ts: u64) {
        // keep the one with smaller ts.
        let evicted_ts = self.invalidation_entries.get_mut(&block_id);
        match evicted_ts {
            Some(previous_ts) => {
                if *previous_ts > ts {
                    *previous_ts = ts;
                }
            }
            None => {
                self.invalidation_entries.insert(block_id, ts);
            }
        }
    }

    #[inline]
    // When there is an option to check hit / miss, we need to check whether this message is invalid by a recent access.
    fn check_invalidation_record(&mut self, block_id: u64, ts: u64) -> bool {
        // check the invalidation record.
        let evicted_ts = self.invalidation_entries.get(&block_id);
        match evicted_ts {
            Some(previous_ts) => {
                if *previous_ts > ts {
                    // the eviction happens earlier than the access.
                    return false;
                } else {
                    // the eviction happens later than the access. We treat it as hit.
                    return true;
                }
            }
            None => {
                // no record. So it is definitely a cache miss.
                return false;
            }
        }
    }

    #[inline]
    // When there is a refill, we need to see if we have to remove the invalidation history of this block.
    fn remove_invalidation_record(&mut self, block_id: u64, ts: u64) {
        let evicted_ts = self.invalidation_entries.get(&block_id);
        match evicted_ts {
            Some(previous_ts) => {
                if *previous_ts <= ts {
                    self.invalidation_entries.remove(&block_id);
                }
            }
            None => {}
        }
    }

    // This function check the cache and update the cache if it is a cache hit. Otherwise, it return false.
    pub fn poke_and_update(
        &mut self,
        block_id: u64,
        ts: u64,
        is_store: bool,
        is_instruction_fetch: bool,
    ) -> bool {
        self.handle_message();
        let hit_element = self.lines.iter_mut().find(|p| {
            return p.tag == block_id && p.state != PrivateCacheState::Invalid;
        });

        if let Some(line) = hit_element {
            // hit, only update the value of the ts if the incoming ts is larger.
            if line.ts < ts {
                line.ts = ts;
            }
            // if it is an instruction fetch, we need to update the is_instruction field.
            line.is_instruction = is_instruction_fetch;

            // update the permission.
            return match line.state {
                PrivateCacheState::Invalid => unreachable!(),
                PrivateCacheState::CleanShared => {
                    if is_store {
                        false
                    } else {
                        true
                    }
                }
                PrivateCacheState::DirtyShared => {
                    if is_store {
                        false
                    } else {
                        true
                    }
                }
                PrivateCacheState::CleanExclusive => true,
                PrivateCacheState::DirtyExclusive => true,
            };
        } else {
            // now we have to check the eviction list. if we find it and the current access has earlier ts, it is a cache hit.
            return self.check_invalidation_record(block_id, ts);
        }
    }

    pub fn refill(
        &mut self,
        _core_id: u32,
        block_id: u64,
        ts: u64,
        is_instruction: bool,
        state: PrivateCacheState,
    ) -> Option<PrivateCacheLine> {
        // find from the cache set with block id.
        let hit_element = self.lines.iter_mut().find(|p| {
            return p.tag == block_id && p.state != PrivateCacheState::Invalid;
        });

        // Note that it is possible to see a hit element. This is mainly from coherence message.
        if let Some(hit_cache_line) = hit_element {
            hit_cache_line.ts = ts;
            // It must be a coherence miss, so the incoming state must be with write permission.
            assert!(state == PrivateCacheState::DirtyExclusive);
            hit_cache_line.state = state;
            hit_cache_line.is_instruction = is_instruction;

            self.remove_invalidation_record(block_id, ts);
            return None;
        }
        // find the first invalid element.
        let invalid_element = self.lines.iter_mut().find(|p| {
            return p.state == PrivateCacheState::Invalid;
        });

        if let Some(invalid_element) = invalid_element {
            invalid_element.ts = ts;
            invalid_element.tag = block_id;
            invalid_element.state = state;
            invalid_element.is_instruction = is_instruction;

            self.remove_invalidation_record(block_id, ts);
            return None;
        } else {
            // find the oldest element.
            let oldest_element = self.lines.iter_mut().min_by(|p, q| {
                return p.ts.cmp(&q.ts);
            });

            match oldest_element {
                Some(oldest_element) => {
                    // Here we need to be careful. In case we have order violation, we don't know the result of this cache hit / miss.
                    // TODO: If the refill timestamp is smaller, we should increase the time of order violation and not to update the cache.
                    let res = oldest_element.clone();
                    assert!(res.ts <= ts);
                    oldest_element.ts = ts;
                    oldest_element.tag = block_id;
                    oldest_element.state = state;
                    oldest_element.is_instruction = is_instruction;

                    self.remove_invalidation_record(block_id, ts);

                    // If the one being invalidated is already in the invalidation list, we don't have to do anything.
                    if self.check_invalidation_record(block_id, ts) {
                        return None;
                    }
                    return Some(res);
                }
                None => {
                    unreachable!("PrivateCache::insert: no element in the cache set.");
                }
            }
        }
    }

    pub fn send_message(&self, block_id: u64, ts: u64, message_type: MessageType) {
        self.invalidation_fifo.push((block_id, ts, message_type));
    }

    pub fn clean_expired_eviction(&mut self) {
        self.invalidation_entries.clear();
    }
}

use serde::ser::SerializeStruct;

impl<const WAY: usize> Serialize for PrivateCacheSet<WAY> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("PrivateCacheSet", 2)?;
        state.serialize_field("lines", &self.lines.as_slice())?;
        state.serialize_field("invalidation_fifo", &self.invalidation_entries)?;
        state.end()
    }
}

#[repr(align(64))]
pub struct PrivateCache<const SET: usize, const WAY: usize> {
    cache: Box<[PrivateCacheSet<WAY>; SET]>,
}

impl<const SET: usize, const WAY: usize> PrivateCache<SET, WAY> {
    pub fn new() -> Self {
        Self {
            cache: crate::util::init_heap_array(|_| PrivateCacheSet::new()),
        }
    }

    pub fn get_set(&mut self, block_id: u64) -> &mut PrivateCacheSet<WAY> {
        let set_id = block_id as usize % SET;
        return &mut self.cache[set_id];
    }

    // This function is called in the boundary of the quantum.
    pub fn clean_expired_eviction(&mut self) {
        for set in self.cache.iter_mut() {
            set.clean_expired_eviction();
        }
    }

    pub fn send_message(&self, block_id: u64, ts: u64, message_type: MessageType) {
        let set_id = block_id as usize % SET;
        self.cache[set_id].send_message(block_id, ts, message_type);
    }

    // This function is only for testing. 
    pub fn contains_block(&mut self, block_id: u64) -> bool {
        let set_id = block_id as usize % SET;
        let set = &mut self.cache[set_id];
        // It has to handle the invalidation message.
        set.handle_message();
        return set
            .lines
            .iter()
            .any(|p| p.tag == block_id && p.state != PrivateCacheState::Invalid);
    }

    // This function is only for testing.
    pub fn get_block_state(&mut self, block_id: u64) -> PrivateCacheState {
        let set_id = block_id as usize % SET;
        let set = &mut self.cache[set_id];
        // It has to handle the invalidation message.
        set.handle_message();
        let hit_element = set
            .lines
            .iter()
            .find(|p| p.tag == block_id && p.state != PrivateCacheState::Invalid);
        if let Some(hit_element) = hit_element {
            return hit_element.state;
        } else {
            return PrivateCacheState::Invalid;
        }
    }
}

impl<const SET: usize, const WAY: usize> Serialize for PrivateCache<SET, WAY> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("PrivateCacheSet", 1)?;
        state.serialize_field("cache", &self.cache.as_slice())?;
        state.end()
    }
}

// There might be another way to design the private cache.
// - No locks for each set.
// - Each set has a ring buffer for the incoming invalidation request from other cores.
// - Before accessing each set, empty the ring buffer, which only requires pure atomic operations.
//   - the ring buffer is a fixed-size array, which has at most ASSO elements.
//   - accessing ring buffer is a pure atomic operation.
//   - pushing message to the ring buffer is an atomic add operation + a write operation.
// - A mutex is necessary for the directory when there is a private cache miss (it is really nice if we can take away this lock)
//   - coherence miss: Write lock, to clean others
//   - capacity/conflict miss, depending on the condition of the directory (rlock)
//        - The cache line is in others' private cache: write lock
//        - The cache line is in the shared cache: write lock, to create a new entry.
// - The shared LLC requires a lock for each set when the LLC is large, and can be replicated when the LLC is small to avoid contention.
