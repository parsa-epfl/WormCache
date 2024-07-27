use rustc_hash::FxHashMap as HashMap;
use serde::Deserialize;
use spin::mutex::SpinMutex;
use spin::mutex::SpinMutexGuard;

use bitvec::prelude::*;
use bitvec::BitArr;
use serde::Serialize;
use zstd::{Decoder, Encoder};

use crate::parameter;
use crate::util;

const SHARED_LIST_LENGTH: usize = if parameter::USE_UNIFIED_CACHE {
    parameter::CORE_COUNT
} else {
    parameter::CORE_COUNT * 2
};

pub type SharerList = BitArr!(for SHARED_LIST_LENGTH, in u64, Lsb0);

// pub struct DirectoryEntry {
//     owner: Option<CoreId>,
//     sharers: SharerList,
// }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirectoryEntry {
    pub lru_ts: u64,
    pub sharers: SharerList,
    pub recent_writer_ts: u64, // This field is to avoid the eviction causes the write history to be lost.
    pub recent_writer_vts: u64,
    pub insertion_ts: u64,
}

impl DirectoryEntry {
    #[inline]
    pub fn update_lru_ts(&mut self, ts: u64) {
        if ts > self.lru_ts {
            self.lru_ts = ts;
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(align(64))]
pub struct DirectorySet<const SET: usize> {
    entries: HashMap<u64, DirectoryEntry>,
    pub index: usize,
}

impl<const SET: usize> DirectorySet<SET> {
    pub fn new(index: usize) -> Self {
        Self {
            entries: HashMap::<u64, DirectoryEntry>::default(),
            index,
        }
    }

    const LOG2_SET: usize = SET.trailing_zeros() as usize;

    pub fn get_or_create(&mut self, block_id: u64) -> &mut DirectoryEntry {
        let internal_id = block_id >> Self::LOG2_SET;

        self.entries.entry(internal_id).or_insert(DirectoryEntry {
            lru_ts: 0,
            sharers: SharerList::ZERO,
            recent_writer_ts: 0,
            insertion_ts: 0,
            recent_writer_vts: 0,
        });

        self.entries.get_mut(&internal_id).unwrap()
    }

    pub fn erase(&mut self, block_id: u64) {
        let internal_id = block_id >> Self::LOG2_SET;
        self.entries.remove(&internal_id);
    }
}

// Probably the Directory should be infinitely sized.
pub struct Directory<const SET: usize> {
    entries: Box<[SpinMutex<DirectorySet<SET>>; SET]>,
}

#[derive(Serialize, Deserialize)]
struct DirectorySerdeHelper<const SET: usize> {
    entries: Vec<DirectorySet<SET>>,
}

impl<const SET: usize> Default for Directory<SET> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SET: usize> Directory<SET> {
    pub fn new() -> Self {
        Self {
            entries: util::init_heap_array(|idx| SpinMutex::new(DirectorySet::new(idx))),
        }
    }

    pub fn fetch_one_entry(&self, block_id: u64) -> SpinMutexGuard<'_, DirectorySet<SET>> {
        let set_id = (block_id as usize) % SET;
        self.entries[set_id].lock()
    }

    pub fn fetch_two_entries(
        &self,
        block_id_0: u64,
        block_id_1: u64,
    ) -> (
        SpinMutexGuard<'_, DirectorySet<SET>>,
        Option<SpinMutexGuard<'_, DirectorySet<SET>>>,
    ) {
        let index_0 = (block_id_0 as usize) % SET;
        let index_1 = (block_id_1 as usize) % SET;

        match index_0.cmp(&index_1) {
            std::cmp::Ordering::Equal => (self.fetch_one_entry(block_id_0), None),
            std::cmp::Ordering::Less => {
                let g0 = self.fetch_one_entry(block_id_0);
                let g1 = self.fetch_one_entry(block_id_1);
                (g0, Some(g1))
            }
            std::cmp::Ordering::Greater => {
                let g1 = self.fetch_one_entry(block_id_1);
                let g0 = self.fetch_one_entry(block_id_0);
                (g0, Some(g1))
            }
        }
    }

    pub fn run_gc(&self) {
        // clean all entries that has zero sharers.
        for set in self.entries.iter() {
            let mut set = set.lock();
            set.entries.retain(|_, entry| entry.sharers.any());
        }
    }

    fn to_serialize_helper(&self) -> DirectorySerdeHelper<SET> {
        let entries = self
            .entries
            .iter()
            .map(|set| set.lock().clone())
            .collect::<Vec<_>>();

        DirectorySerdeHelper { entries }
    }

    fn from_serialize_helper(helper: DirectorySerdeHelper<SET>) -> Self {
        let entries = helper
            .entries
            .into_iter()
            .map(|set| SpinMutex::new(set))
            .collect::<Vec<_>>();

        Self {
            entries: entries.try_into().unwrap(),
        }
    }

    pub fn serialize(&self, name: &str, numa_node_id: usize) {
        self.run_gc();
        let file = std::fs::File::create(format!("{}/directory-{}.json.zstd", name, numa_node_id))
            .unwrap();

        let mut file = Encoder::new(file, 0).unwrap();

        let helper = self.to_serialize_helper();
        serde_json::to_writer_pretty(&mut file, &helper).unwrap();

        file.finish().unwrap();
    }

    pub fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        let file = std::fs::File::open(format!("{}/directory-{}.json.zstd", name, numa_node_id));

        if file.is_err() {
            println!("Cannot load the directory state. Error: {:?}", file.err());
            return;
        }

        let file = file.unwrap();

        let file = Decoder::new(file).unwrap();

        let helper: DirectorySerdeHelper<SET> = serde_json::from_reader(file).unwrap();
        *self = Self::from_serialize_helper(helper);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct SerializedDirectoryEntry {
    pub tag: u64,
    pub sharers: SharerList,
}

use serde::ser::{SerializeStruct, Serializer};

impl Serialize for SerializedDirectoryEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("SerializedDirectoryEntry", 2)?;
        state.serialize_field("tag", &self.tag)?;
        state.serialize_field(
            "sharers",
            &self
                .sharers
                .iter()
                .rev()
                .map(|b| if *b { "1" } else { "0" })
                .collect::<Vec<_>>(),
        )?;
        state.end()
    }
}

impl<const SET: usize> Directory<SET> {
    pub fn dump_snapshot(&self, snapshot_folder: &str) {
        let file = std::fs::File::create(format!("{}/directory.json", snapshot_folder)).unwrap();

        let entries = self
            .entries
            .iter()
            .flat_map(|set| {
                let set = set.lock();
                set.entries
                    .iter()
                    .filter_map(|(tag, entry)| {
                        if entry.sharers.not_any() {
                            return None;
                        }
                        Some(SerializedDirectoryEntry {
                            tag: (*tag) * (SET as u64) + set.index as u64,
                            sharers: entry.sharers,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        serde_json::to_writer_pretty(&file, &entries).unwrap();
    }
}
