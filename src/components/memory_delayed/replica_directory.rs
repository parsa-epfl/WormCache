use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::MutexGuard;

use std::collections::btree_map::BTreeMap;

use crate::util;

#[derive(Debug)]
pub enum DirectoryEntry {
    DirtyExclusive(u32, u64), // core_id, ts. This point is different from Exclusive.
    CleanExclusive(u32, u64), // core_id, ts
    Shared(HashMap<u32, u64>), // core_id, ts
    Evicted(u64),             // ts, the last writer's timestamp.
}

pub enum GetModifyResult {
    Successful(Vec<u32>),            // evicted sharers.
    SuccessfulWithSharers(Vec<u32>), // evicted sharers.
    Rejected,                        // not successful.
}

pub enum GetReadResult {
    Exclusive,
    Successful,
    SuccessfulWithMessage(u32), // Need to send a message to the owner to create replica.
    Rejected,
}

pub enum DropResult {
    NoSharer,
    NewExclusive(u32),
    MoreSharers,
}

impl DirectoryEntry {
    pub fn new(is_store: bool, core_id: u32, ts: u64) -> Self {
        if is_store {
            DirectoryEntry::DirtyExclusive(core_id, ts)
        } else {
            DirectoryEntry::CleanExclusive(core_id, ts)
        }
    }

    pub fn get_modify(&mut self, core_id: u32, ts: u64, is_writing: bool) -> GetModifyResult {
        match self {
            DirectoryEntry::DirtyExclusive(owner, owner_ts) => {
                if *owner != core_id && *owner_ts < ts {
                    // owner replacement.
                    let previous_owner = *owner;
                    *self = if is_writing {
                        DirectoryEntry::DirtyExclusive(core_id, ts)
                    } else {
                        DirectoryEntry::CleanExclusive(core_id, ts)
                    };
                    GetModifyResult::Successful(vec![previous_owner])
                } else {
                    assert!(*owner != core_id);
                    GetModifyResult::Rejected
                }
            }

            DirectoryEntry::CleanExclusive(owner, owner_ts) => {
                if *owner != core_id && *owner_ts < ts {
                    // owner replacement.
                    let previous_owner = *owner;
                    *self = if is_writing {
                        DirectoryEntry::DirtyExclusive(core_id, ts)
                    } else {
                        DirectoryEntry::CleanExclusive(core_id, ts)
                    };
                    GetModifyResult::Successful(vec![previous_owner])
                } else {
                    assert!(*owner != core_id);
                    GetModifyResult::Rejected
                }
            }

            DirectoryEntry::Shared(sharers) => {
                let mut result = vec![];
                for (reader, reader_ts) in sharers.iter() {
                    if *reader_ts < ts && *reader != core_id {
                        result.push(*reader);
                    }
                }

                // remove other sharers.
                for invalid_sharers in result.iter() {
                    sharers.remove(invalid_sharers);
                }

                // if there is no sharer left or the only sharer is the core_id, then we can get exclusive.
                if sharers.is_empty() || (sharers.len() == 1 && sharers.contains_key(&core_id)) {
                    *self = if is_writing {
                        DirectoryEntry::DirtyExclusive(core_id, ts)
                    } else {
                        DirectoryEntry::CleanExclusive(core_id, ts)
                    };
                    return GetModifyResult::Successful(result);
                } else {
                    sharers.insert(core_id, ts);
                    return GetModifyResult::SuccessfulWithSharers(result);
                }
            }

            DirectoryEntry::Evicted(evicted_ts) => {
                if *evicted_ts > ts {
                    GetModifyResult::Rejected
                } else {
                    *self = if is_writing {
                        DirectoryEntry::DirtyExclusive(core_id, ts)
                    } else {
                        DirectoryEntry::CleanExclusive(core_id, ts)
                    };
                    GetModifyResult::Successful(vec![])
                }
            }
        }
    }

    // Return whether a replica is generated.
    pub fn get_read(&mut self, core_id: u32, ts: u64) -> GetReadResult {
        return match self {
            DirectoryEntry::DirtyExclusive(owner, owner_ts) => {
                if *owner != core_id && *owner_ts < ts {
                    let original_owner = *owner;
                    let mut sharers = HashMap::new();
                    sharers.insert(*owner, *owner_ts);
                    sharers.insert(core_id, ts);
                    *self = DirectoryEntry::Shared(sharers);
                    GetReadResult::SuccessfulWithMessage(original_owner)
                } else {
                    assert!(core_id != *owner);
                    GetReadResult::Rejected
                }
            }

            DirectoryEntry::CleanExclusive(owner, owner_ts) => {
                assert!(core_id != *owner);
                let original_owner = *owner;
                let mut sharers = HashMap::new();
                sharers.insert(*owner, *owner_ts);
                sharers.insert(core_id, ts);
                *self = DirectoryEntry::Shared(sharers);
                GetReadResult::SuccessfulWithMessage(original_owner)
            }

            DirectoryEntry::Shared(sharers) => {
                sharers.insert(core_id, ts);
                GetReadResult::Successful
            }

            DirectoryEntry::Evicted(previous_owner_ts) => {
                if *previous_owner_ts > ts {
                    GetReadResult::Rejected
                } else {
                    // This is the speculation of the coherence protocol.
                    *self = DirectoryEntry::CleanExclusive(core_id, ts);
                    GetReadResult::Exclusive
                }
            }
        };
    }

    // Return whether there are still sharers left.
    pub fn drop(&mut self, core_id: u32) -> DropResult {
        return match self {
            DirectoryEntry::DirtyExclusive(owner, owner_ts) => {
                if *owner == core_id {
                    *self = DirectoryEntry::Evicted(*owner_ts);
                    DropResult::NoSharer
                } else {
                    panic!("It is impossible to issue evict a block that is not in the directory.");
                }
            }

            DirectoryEntry::CleanExclusive(owner, owner_ts) => {
                if *owner == core_id {
                    *self = DirectoryEntry::Evicted(*owner_ts);
                    DropResult::NoSharer
                } else {
                    panic!("It is impossible to issue evict a block that is not in the directory.");
                }
            }

            DirectoryEntry::Shared(sharers) => {
                sharers.remove(&core_id);

                let sharer_number = sharers.len();

                if sharer_number == 1 {
                    let owner = *sharers.keys().next().unwrap();
                    let ts = *sharers.values().next().unwrap();

                    *self = DirectoryEntry::CleanExclusive(owner, ts);

                    DropResult::NewExclusive(owner)
                } else if sharer_number == 0 {
                    *self = DirectoryEntry::Evicted(0);
                    DropResult::NoSharer
                } else {
                    DropResult::MoreSharers
                }
            }

            DirectoryEntry::Evicted(_) => DropResult::NoSharer,
        };
    }

    pub fn is_active(&self) -> bool {
        return match self {
            DirectoryEntry::DirtyExclusive(_, _) => true,
            DirectoryEntry::CleanExclusive(_, _) => true,
            DirectoryEntry::Shared(_) => true,
            DirectoryEntry::Evicted(_) => false,
        };
    }
}

#[derive(Debug)]
#[repr(align(64))]
pub struct DirectorySet<const WAYS: usize> {
    // tags: Box<[u64; WAYS]>,
    // entries: Box<[DirectoryEntry; WAYS]>,
    entries: BTreeMap<u64, DirectoryEntry>,
}

impl<const WAYS: usize> DirectorySet<WAYS> {
    pub fn new() -> Self {
        Self {
            // tags: util::init_heap_array(|_| 0),
            // entries: util::init_heap_array(|_| DirectoryEntry::new()),
            entries: BTreeMap::new(),
        }
    }

    // pub fn valid(&self, block_id: u64) -> bool {
    //     let internal_tag = block_id << 1 | 1;
    //     return match self.entries.get(&internal_tag) {
    //         Some(entry) => match entry {
    //             DirectoryEntry::Evicted(_) => false,
    //             _ => true,
    //         },
    //         None => false,
    //     };
    // }

    pub fn exists(&self, block_id: u64) -> bool {
        let internal_tag = block_id << 1 | 1;
        return self.entries.contains_key(&internal_tag);
    }

    pub fn evicted(&self, block_id: u64) -> bool {
        let internal_tag = block_id << 1 | 1;
        return match self.entries.get(&internal_tag) {
            Some(entry) => match entry {
                DirectoryEntry::Evicted(_) => true,
                _ => false,
            },
            None => false,
        };
    }

    pub fn create(
        &mut self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_store: bool,
    ) -> &mut DirectoryEntry {
        let internal_tag = block_id << 1 | 1;

        // return match self.entries.get_mut(&internal_tag) {
        //     None => {
        //         let entry = self.entries.entry(internal_tag).or_insert(DirectoryEntry::new(is_store, core_id, ts));
        //         entry
        //     },

        //     Some(entry) => {
        //         assert!(!entry.is_active());
        //         *entry = DirectoryEntry::new(is_store, core_id, ts);
        //         entry
        //     },
        // };

        let entry = self
            .entries
            .insert(internal_tag, DirectoryEntry::new(is_store, core_id, ts));
        if let Some(old_entry) = entry {
            assert!(!old_entry.is_active());
        }

        return self.entries.get_mut(&internal_tag).unwrap();
    }

    pub fn get_mut(&mut self, block_id: u64) -> &mut DirectoryEntry {
        let internal_tag = block_id << 1 | 1;

        let hit = self.entries.get_mut(&internal_tag);

        if let Some(index) = hit {
            index
        } else {
            panic!("No such entry found.")
        }
    }

    pub fn invalidate(&mut self, block_id: u64) {
        // let hit = self
        //     .tags
        //     .iter()
        //     .enumerate()
        //     .find(|entry| (*entry.1) == (block_id << 1 | 1));

        // if let Some((index, _)) = hit {
        //     self.tags[index] = 0;
        // }
        let internal_tag = block_id << 1 | 1;
        self.entries.remove(&internal_tag);
    }
}

#[derive(Debug)]
pub struct ReplicaDirectory<const SETS: usize, const WAYS: usize> {
    pub sets: Box<[Mutex<DirectorySet<WAYS>>; SETS]>,
}

impl<const SETS: usize, const WAYS: usize> ReplicaDirectory<SETS, WAYS> {
    pub fn new() -> Self {
        Self {
            sets: util::init_heap_array(|_| Mutex::new(DirectorySet::new())),
        }
    }

    pub fn get_set(&self, block_id: u64) -> MutexGuard<'_, DirectorySet<WAYS>> {
        let index = (block_id as usize) % SETS;
        let guard = self.sets[index].lock().unwrap();
        guard
    }
}
