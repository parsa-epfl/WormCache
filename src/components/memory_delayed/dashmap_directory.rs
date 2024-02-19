use dashmap::mapref::one::RefMut;
use dashmap::DashMap;

use bitvec::prelude::*;
use bitvec::BitArr;

pub type SharerList = BitArr!(for crate::parameter::CORE_COUNT, in u64, Lsb0);

// pub struct DirectoryEntry {
//     owner: Option<CoreId>,
//     sharers: SharerList,
// }

pub struct DirectoryEntry {
    pub ts: u64,
    pub sharers: SharerList,
    // pub history: Vec<(u32, bool, u64, bool)>, // (core_id, is_store, ts, l1miss) // This entry is for debugging.
}

// pub struct DirectorySet {
//     entries: HashMap<u64, DirectoryEntry>,
// }

// impl DirectorySet {
//     pub fn new() -> Self {
//         Self {
//             entries: HashMap::new(),
//         }
//     }

//     pub fn get_or_create(&mut self, block_id: u64) -> &mut DirectoryEntry {
//         self.entries.entry(block_id).or_insert(DirectoryEntry {
//             ts: 0,
//             sharers: SharerList::ZERO,
//             history: Vec::new(),
//         })
//     }

//     pub fn remove(&mut self, block_id: &u64) {
//         self.entries.remove(block_id);
//     }
// }

// Probably the Directory should be infinitely sized.
pub struct Directory<const SETS: usize> {
    // entries: [Mutex<DirectorySet>; SETS], // map :: block_id -> DirectoryEntry
    //                                       // entries: DashMap<u64, DirectoryEntry>,
    entries: DashMap<u64, DirectoryEntry>,
}

impl<const SETS: usize> Directory<SETS> {
    pub fn new() -> Self {
        Self {
            // entries: std::array::from_fn(|_| Mutex::new(DirectorySet::new())),
            entries: DashMap::with_shard_amount(SETS),
        }
    }

    pub fn get_or_create(&self, block_id: u64) -> RefMut<'_, u64, DirectoryEntry> {
        self.entries
            .entry(block_id)
            .or_insert_with(|| DirectoryEntry {
                ts: 0,
                sharers: SharerList::ZERO,
                // history: Vec::new(),
            })
    }

    pub fn mark_as_useless(&self, block_id: u64) {
        // self.entries.remove(&block_id);
        // let index = (block_id as usize) % 2048;
        // let mut guard = self.entries[index].lock().unwrap();
        // guard.remove(&block_id);
    }

    pub fn print_statistics(&self) {
        // print the number of entries which only has one sharer.
        let mut count = 0;
        for entry in self.entries.iter() {
            if entry.value().sharers.count_ones() == 1 {
                count += 1;
            }
        }

        println!("Number of entries which only has one sharer: {}", count);

        // Print the number of entries which is empty.
        let mut count = 0;
        for entry in self.entries.iter() {
            if entry.value().sharers.count_ones() == 0 {
                count += 1;
            }
        }

        println!("Number of entries which is empty: {}", count);

        // Print the total number of entries.
        println!("Total number of entries: {}", self.entries.len());
    }
}
