
use serde::{Deserialize, Serialize};
use spin::mutex::SpinMutex;
use crate::components::cache_hierarchy::CacheBlockRequest;
use super::super::CCell;
use super::acc::AccTableEntry;
use super::util;
use zstd::{Decoder, Encoder};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PHTEntry<
    const N_BLK: usize,
> {
    pub tag: u64,
    pub write_pattern: Vec<u8>,
    pub read_pattern: Vec<u8>,
    pub ts: u64,
    pub valid: bool,
}

impl<
    const N_BLK: usize,
> PHTEntry<N_BLK> {
    pub fn new() -> Self {
        Self {
            tag: 0,
            write_pattern: Vec::from_iter(
                std::iter::repeat(1).take(N_BLK)
            ),
            read_pattern: Vec::from_iter(
                std::iter::repeat(1).take(N_BLK)
            ),
            ts: 0,
            valid: false,
        }
    }

    pub fn set(&mut self, tag: u64, acc_entry: &AccTableEntry<N_BLK>) {
        self.tag = tag;
        self.ts = acc_entry.ts;
        self.valid = true;

        for i in 0..N_BLK {
            if acc_entry.access_pattern[i] {
                if acc_entry.read_pattern[i] {
                    self.read_pattern[i] = 2;
                    self.write_pattern[i] = 1;
                } else {
                    self.write_pattern[i] = 2;
                    self.read_pattern[i] = 1;
                }
            } else {
                self.read_pattern[i] = 1;
                self.write_pattern[i] = 1;
            }
        }
        // util::rotate_left::<u8, N_BLK>(&mut self.write_pattern, acc_entry.offset as usize);
        // util::rotate_left::<u8, N_BLK>(&mut self.read_pattern, acc_entry.offset as usize);
    }

    fn update_write_pattern(&mut self, write_pattern: &[bool; N_BLK]) {
        for (i, &bit) in write_pattern.iter().enumerate() {
            match (bit, self.write_pattern[i]) {
                (true, v) if v < 3 => self.write_pattern[i] += 1,
                (false, v) if v > 0 => self.write_pattern[i] -= 1,
                _ => {}
            }
        }
    }

    fn update_read_pattern(&mut self, read_pattern: &[bool; N_BLK]) {
        for (i, &bit) in read_pattern.iter().enumerate() {
            match (bit, self.read_pattern[i]) {
                (true, v) if v < 3 => self.read_pattern[i] += 1,
                (false, v) if v > 0 => self.read_pattern[i] -= 1,
                _ => {}
            }
        }
    }

    pub fn update(&mut self, acc_entry: &AccTableEntry<N_BLK>) {
        assert!(self.valid, "Cannot update invalid PHT entry");
        let mut write_pattern = [false; N_BLK];
        let mut read_pattern = [false; N_BLK];
        for i in 0..N_BLK {
            if acc_entry.access_pattern[i] {
                read_pattern[i] = acc_entry.read_pattern[i];
                write_pattern[i] = !acc_entry.read_pattern[i];
            } else {
                assert!(!acc_entry.read_pattern[i], "Read Bit cannot be set while Access Bit Unset");
                read_pattern[i] = false;
                write_pattern[i] = false;
            }
        }
        // util::rotate_left::<bool, N_BLK>(&mut write_pattern, acc_entry.offset as usize);
        // util::rotate_left::<bool, N_BLK>(&mut read_pattern, acc_entry.offset as usize);
        self.update_write_pattern(&write_pattern);
        self.update_read_pattern(&read_pattern);
        self.ts = acc_entry.ts;
    }

    pub fn get_bit_vector(&self, is_read: bool) -> [bool; N_BLK] {
        let pattern = if is_read {
            &self.read_pattern
        } else {
            &self.write_pattern
        };
        let mut bit_vector = [false; N_BLK];
        for (i, &value) in pattern.iter().enumerate() {
            bit_vector[i] = value >= 2;
        }
        bit_vector
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PHTSet<
    const PHT_WAYS: usize,
    const N_BLK: usize,
> {
    pub entries: Vec<PHTEntry<N_BLK>>,
}

impl<
    const PHT_WAYS: usize,
    const N_BLK: usize,
> PHTSet<PHT_WAYS, N_BLK>
{
    pub fn new() -> Self {
        Self {
            entries: Vec::from_iter(
                std::iter::repeat(PHTEntry::<N_BLK>::new()).take(PHT_WAYS)
            ),
        }
    }

    pub fn lookup(&mut self, tag: u64, is_read: bool, ts: u64) -> Option<[bool; N_BLK]> {
        for entry in self.entries.iter_mut() {
            if entry.tag == tag && entry.valid {
                entry.ts = ts; // Update timestamp
                return Some(entry.get_bit_vector(is_read));
            }
        }
        None
    }

    pub fn insert(&mut self, tag: u64, acc_entry: &AccTableEntry<N_BLK>) {
        let mut empty = false;
        let mut empty_idx = 0;
        let mut lru_idx = 0;
        let mut lru_ts = u64::MAX;
        assert!(acc_entry.valid, "Cannot insert invalid entry into PHT");
        
        for (i, entry) in self.entries.iter_mut().enumerate() {
            if entry.tag == tag && entry.valid {
                entry.update(acc_entry);
                return;
            }
            if !entry.valid & !empty {
                empty = true;
                empty_idx = i;
            }
            if entry.ts < lru_ts {
                lru_ts = entry.ts;
                lru_idx = i;
            }
        }
        if empty {
            self.entries[empty_idx].set(tag, acc_entry);
        } else {
            self.entries[lru_idx].set(tag, acc_entry);
        }
    }

}

#[derive(Debug)]
pub struct PHTPerCore<
    G: CCell<PHTSet<PHT_WAYS, N_BLK>> + std::fmt::Debug,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
> {
    pub sets: Box<[G; PHT_SETS]>,
}

#[derive(Serialize, Deserialize)]
pub struct PHTPerCoreSerdeHelper<
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
> {
    pub sets: Vec<PHTSet<PHT_WAYS, N_BLK>>
}

impl<
    G: CCell<PHTSet<PHT_WAYS, N_BLK>> + std::fmt::Debug,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
> PHTPerCore<G, PHT_SETS, PHT_WAYS, N_BLK>
{
    pub fn new() -> Self {
        Self {
            sets: crate::util::init_heap_array(|_| G::new(PHTSet::<PHT_WAYS, N_BLK>::new())),
        }
    }

    fn from_serialize_helper(helper: PHTPerCoreSerdeHelper<PHT_SETS, PHT_WAYS, N_BLK>) -> Self {
        let mut sets = Vec::with_capacity(PHT_SETS);
        for set in helper.sets {
            sets.push(G::new(set));
        }
        Self {
            sets: sets.into_boxed_slice().try_into().unwrap(),
        }
    }

    fn to_serialize_helper(&self) -> PHTPerCoreSerdeHelper<PHT_SETS, PHT_WAYS, N_BLK> {
        PHTPerCoreSerdeHelper {
            sets: self
                .sets
                .iter()
                .map(|set| set.inner().clone())
                .collect(),
        }
    }

    pub fn lookup(&self, request: &CacheBlockRequest, ts: u64) -> Option<Vec<u64>> {
        let (base, pc, offset) = util::get_base_pc_offset(request, N_BLK);
        let key = util::build_key(pc, offset, PHT_SETS);
        let set_idx = key & ((1 << PHT_SETS.trailing_zeros()) - 1);
        let tag = key >> PHT_SETS.trailing_zeros();
        match self.sets[set_idx  as usize].inner().lookup(tag, !request.is_store(), ts) {
            Some(bitvec) => {
                // util::rotate_right::<bool, N_BLK>(&mut bitvec, offset as usize);
                let mut result = Vec::new();
                for (i, &bit) in bitvec.iter().enumerate() {
                    if bit {
                        result.push(util::get_address(base, i as u64, N_BLK));
                    }
                }
                Some(result)
            }
            None => None,
        }
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>) {
        let key = util::build_key(entry.pc, entry.offset, PHT_SETS);
        let set_idx = key & ((1 << PHT_SETS.trailing_zeros()) - 1);
        let tag = key >> PHT_SETS.trailing_zeros();
        self.sets[set_idx as usize].inner().insert(tag, entry);
    }
}

pub struct PHT<
    G: CCell<PHTSet<PHT_WAYS, N_BLK>> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
> {
    pub tables: Box<[PHTPerCore<G, PHT_SETS, PHT_WAYS, N_BLK>; CORE_COUNT]>,
}

impl<
    G: CCell<PHTSet<PHT_WAYS, N_BLK>> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
> PHT<G, CORE_COUNT, PHT_SETS, PHT_WAYS, N_BLK>
{
    pub fn new() -> Self {
        Self {
            tables: crate::util::init_heap_array(|_| PHTPerCore::<G, PHT_SETS, PHT_WAYS, N_BLK>::new()),
        }
    }

    pub fn lookup(&self, request: &CacheBlockRequest, ts: u64) -> Option<Vec<u64>> {
        let core_id = request.core_id as usize;
        self.tables[core_id].lookup(request, ts)
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>, core_id: usize) {
        self.tables[core_id].insert(entry);
    }

    pub fn serialize(&self, name: &str, numa_node_id: usize) {
        let helper = self
            .tables
            .iter()
            .map(|table| table.to_serialize_helper())
            .collect::<Vec<_>>();

        let file = 
            std::fs::File::create(format!("{}/{}-{}.json.zstd", name, "pht", numa_node_id))
                .unwrap();

        let mut file = Encoder::new(file, 0).unwrap();

        serde_json::to_writer(&mut file, &helper).unwrap();

        file.finish().unwrap();
    }

    pub fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        let file =
            std::fs::File::open(format!("{}/{}-{}.json.zstd", name, "pht", numa_node_id));

        if file.is_err() {
            println!(
                "Cannot load the PHT. Error: {:?}",
                file.err()
            );
            return;
        }

        let file = file.unwrap();
        let mut file = Decoder::new(file).unwrap();

        let helper: Vec<PHTPerCoreSerdeHelper<PHT_SETS, PHT_WAYS, N_BLK>> =
            serde_json::from_reader(&mut file).unwrap();

        for (table, helper) in self.tables.iter_mut().zip(helper.into_iter()) {
            *table = PHTPerCore::from_serialize_helper(helper);
        }
    }

}

pub type ParallelPHT<
    const CORE_COUNT: usize,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
> = PHT<SpinMutex<PHTSet<PHT_WAYS, N_BLK>>, CORE_COUNT, PHT_SETS, PHT_WAYS, N_BLK>;