
use serde::{Deserialize, Serialize};
use spin::mutex::SpinMutex;
use crate::{components::cache_hierarchy::CacheBlockRequest};
use super::super::CCell;
use super::acc::AccTableEntry;
use super::util;
use zstd::{Decoder, Encoder};

// Store all three types of patterns even if not used
// TODO: can be optimized later
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PHTEntry<
    const N_BLK: usize,
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> {
    pub tag: u64,
    pub access_pattern: Vec<u8>,
    pub write_pattern: Vec<u8>,
    pub read_pattern: Vec<u8>,
    pub ts: u64,
    pub valid: bool,
}

impl<
    const N_BLK: usize,
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> PHTEntry<N_BLK, ROT, SEP_RDWR, SAT_CNT> {
    pub fn new() -> Self {
        Self {
            tag: 0,
            access_pattern: Vec::from_iter(
                std::iter::repeat(1).take(N_BLK)
            ),
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
        if SEP_RDWR {
            for i in 0..N_BLK {
                self.read_pattern[i] = if acc_entry.access_pattern[i] {
                    if acc_entry.read_pattern[i] { 2 } else { 1 }
                } else { 1 };
                self.write_pattern[i] = if acc_entry.access_pattern[i] {
                    if acc_entry.read_pattern[i] { 1 } else { 2 }
                } else { 1 };
            }
        } else {
            for i in 0..N_BLK {
                self.access_pattern[i] = if acc_entry.access_pattern[i] { 2 } else { 1 };
            }
        }
        if ROT {
            if SEP_RDWR {
                util::rotate_left_vec::<u8>(&mut self.write_pattern, acc_entry.offset as usize);
                util::rotate_left_vec::<u8>(&mut self.read_pattern, acc_entry.offset as usize);
            } else {
                util::rotate_left_vec::<u8>(&mut self.access_pattern, acc_entry.offset as usize);
            }
        }
    }

    fn update_pattern(&mut self, pattern: &[bool; N_BLK], pat_type: util::PatternType) {
        let target_pattern = match pat_type {
            util::PatternType::Access => &mut self.access_pattern,
            util::PatternType::Read => &mut self.read_pattern,
            util::PatternType::Write => &mut self.write_pattern,
        };
        if SAT_CNT {
            for (i, &bit) in pattern.iter().enumerate() {
                match (bit, target_pattern[i]) {
                    (true, v) if v < 3 => target_pattern[i] += 1,
                    (false, v) if v > 0 => target_pattern[i] -= 1,
                    _ => {}
                }
            }
        } else {
            for (i, &bit) in pattern.iter().enumerate() {
                target_pattern[i] = if bit { 2 } else { 1 };
            }
        }
    }

    pub fn update(&mut self, acc_entry: &AccTableEntry<N_BLK>) {
        assert!(self.valid, "Cannot update invalid PHT entry");
        if SEP_RDWR {
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
            if ROT {
                util::rotate_left_arr::<bool, N_BLK>(&mut write_pattern, acc_entry.offset as usize);
                util::rotate_left_arr::<bool, N_BLK>(&mut read_pattern, acc_entry.offset as usize);
            }
            self.update_pattern(&write_pattern, util::PatternType::Write);
            self.update_pattern(&read_pattern, util::PatternType::Read);
        } else {
            let mut access_pattern = [false; N_BLK];
            for i in 0..N_BLK {
                access_pattern[i] = acc_entry.access_pattern[i];
            }
            if ROT {
                util::rotate_left_arr::<bool, N_BLK>(&mut access_pattern, acc_entry.offset as usize);
            }
            self.update_pattern(&access_pattern, util::PatternType::Access);
        }
        self.ts = acc_entry.ts;
    }

    pub fn get_bit_vector(&self, is_read: bool) -> [bool; N_BLK] {
        let pattern = match SEP_RDWR {
            true => match is_read {
                true => &self.read_pattern,
                false => &self.write_pattern,
            }
            false => &self.access_pattern,
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
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> {
    pub entries: Vec<PHTEntry<N_BLK, ROT, SEP_RDWR, SAT_CNT>>,
}

impl<
    const PHT_WAYS: usize,
    const N_BLK: usize,
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> PHTSet<PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>
{
    pub fn new() -> Self {
        Self {
            entries: Vec::from_iter(
                std::iter::repeat(PHTEntry::<N_BLK, ROT, SEP_RDWR, SAT_CNT>::new()).take(PHT_WAYS)
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
    G: CCell<PHTSet<PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>> + std::fmt::Debug,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> {
    pub sets: Box<[G; PHT_SETS]>,
}

#[derive(Serialize, Deserialize)]
pub struct PHTPerCoreSerdeHelper<
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> {
    pub sets: Vec<PHTSet<PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>>
}

impl<
    G: CCell<PHTSet<PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>> + std::fmt::Debug,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> PHTPerCore<G, PHT_SETS, PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>
{
    pub fn new() -> Self {
        Self {
            sets: crate::util::init_heap_array(|_| G::new(PHTSet::<PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>::new())),
        }
    }

    fn from_serialize_helper(helper: PHTPerCoreSerdeHelper<PHT_SETS, PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>) -> Self {
        let mut sets = Vec::with_capacity(PHT_SETS);
        for set in helper.sets {
            sets.push(G::new(set));
        }
        Self {
            sets: sets.into_boxed_slice().try_into().unwrap(),
        }
    }

    fn to_serialize_helper(&self) -> PHTPerCoreSerdeHelper<PHT_SETS, PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT> {
        PHTPerCoreSerdeHelper {
            sets: self
                .sets
                .iter()
                .map(|set| set.inner().clone())
                .collect(),
        }
    }

    #[inline]
    fn get_base_pc_offset(&self, request: &CacheBlockRequest) -> (u64, u64, u64) {
        util::get_base_pc_offset::<N_BLK>(request)
    }

    #[inline]
    fn build_key(&self, pc: u64, offset: u64) -> u64 {
        util::build_key::<N_BLK, PHT_SETS, ROT>(pc, offset)
    }

    #[inline]
    fn get_address(&self, base: u64, offset: u64) -> u64 {
        util::get_address::<N_BLK>(base, offset)
    }

    pub fn lookup(&self, request: &CacheBlockRequest, ts: u64) -> Option<Vec<u64>> {
        let (base, pc, offset) = self.get_base_pc_offset(request);
        let key = self.build_key(pc, offset);
        let set_idx = key & ((1 << PHT_SETS.trailing_zeros()) - 1);
        let tag = key >> PHT_SETS.trailing_zeros();
        match self.sets[set_idx  as usize].inner().lookup(tag, !request.is_store(), ts) {
            Some(mut bitvec) => {
                if ROT {
                    util::rotate_right::<bool, N_BLK>(&mut bitvec, offset as usize);
                }
                let mut result = Vec::new();
                for (i, &bit) in bitvec.iter().enumerate() {
                    if bit {
                        result.push(self.get_address(base, i as u64));
                    }
                }
                Some(result)
            }
            None => None,
        }
    }

    pub fn insert(&self, entry: &AccTableEntry<N_BLK>) {
        let key = self.build_key(entry.pc, entry.offset);
        let set_idx = key & ((1 << PHT_SETS.trailing_zeros()) - 1);
        let tag = key >> PHT_SETS.trailing_zeros();
        self.sets[set_idx as usize].inner().insert(tag, entry);
    }
}

pub struct PHT<
    G: CCell<PHTSet<PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> {
    pub tables: Box<[PHTPerCore<G, PHT_SETS, PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>; CORE_COUNT]>,
}

impl<
    G: CCell<PHTSet<PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>> + std::fmt::Debug,
    const CORE_COUNT: usize,
    const PHT_SETS: usize,
    const PHT_WAYS: usize,
    const N_BLK: usize,
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> PHT<G, CORE_COUNT, PHT_SETS, PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>
{
    pub fn new() -> Self {
        Self {
            tables: crate::util::init_heap_array(|_| PHTPerCore::<G, PHT_SETS, PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>::new()),
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

        let helper: Vec<PHTPerCoreSerdeHelper<PHT_SETS, PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>> =
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
    const ROT: bool,
    const SEP_RDWR: bool,
    const SAT_CNT: bool,
> = PHT<SpinMutex<PHTSet<PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>>, CORE_COUNT, PHT_SETS, PHT_WAYS, N_BLK, ROT, SEP_RDWR, SAT_CNT>;