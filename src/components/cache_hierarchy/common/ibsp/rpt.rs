use crate::components::cache_hierarchy::CacheBlockRequest;
use crate::checkpoint::helpers::{RPTEntryHelper, RPTPerCoreHelper, RPTSetHelper};
use super::util;
use spin::mutex::SpinMutex;

#[derive(Debug, Clone, Copy)]
struct RPTEntry{
    tag: u64,
    last_block: u64,
    last_stride: i64,   // Stride can be negative, so signed integer
    ts: u64,
}

impl RPTEntry {
    pub const fn new() -> Self {
        Self {
            tag: 0,
            last_block: 0,
            last_stride: 0,
            ts: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct RPTSet<
    const RPT_WAYS: usize,
> {
    entries: Vec<RPTEntry>,
}

impl<
    const RPT_WAYS: usize
> RPTSet<RPT_WAYS>
{
    pub fn new() -> Self {
        Self {
            entries: Vec::from_iter(
                std::iter::repeat(RPTEntry::new()).take(RPT_WAYS)
            ),
        }
    }

    pub fn to_checkpoint_helper(&self) -> RPTSetHelper {
        RPTSetHelper {
            entries: self
                .entries
                .iter()
                .map(|entry| RPTEntryHelper {
                    tag: entry.tag,
                    last_block: entry.last_block,
                    last_stride: entry.last_stride,
                    ts: entry.ts,
                })
                .collect(),
        }
    }

    pub fn from_checkpoint_helper(helper: RPTSetHelper) -> Self {
        assert_eq!(helper.entries.len(), RPT_WAYS, "RPT set size mismatch");
        Self {
            entries: helper
                .entries
                .into_iter()
                .map(|entry| RPTEntry {
                    tag: entry.tag,
                    last_block: entry.last_block,
                    last_stride: entry.last_stride,
                    ts: entry.ts,
                })
                .collect(),
        }
    }

    pub fn lookup(&mut self, tag: u64, ts: u64) -> Option<(u64, u64, i64)> {
        for (idx, entry) in &mut self.entries.iter_mut().enumerate() {
            if entry.tag == tag {
                entry.ts = ts;
                return Some((idx as u64, entry.last_block, entry.last_stride));
            }
        }
        None
    }

    pub fn insert(&mut self, tag: u64, block_id: u64, ts: u64) {
        let lru_idx = self.entries.iter().enumerate().min_by_key(|(_, entry)| entry.ts).map(|(idx, _)| idx).unwrap_or(0);
        self.entries[lru_idx] = RPTEntry {
            tag,
            last_block: block_id,
            last_stride: 0,
            ts,
        };
    }
}

pub struct RPTPerCore<
    const RPT_SETS: usize,      // Number of sets in the RPT
    const RPT_WAYS: usize,      // Assosciative ways in each set
    const N_PC: usize,          // Number of PC bits used to index into the table
> {
    sets: Vec<RPTSet<RPT_WAYS>>,
}

impl<
    const RPT_SETS: usize,
    const RPT_WAYS: usize,
    const N_PC: usize,
> RPTPerCore<RPT_SETS, RPT_WAYS, N_PC>
{
    pub fn new() -> Self {
        Self {
            sets: Vec::from_iter(
                std::iter::repeat(RPTSet::<RPT_WAYS>::new()).take(RPT_SETS)
            ),
        }
    }

    pub fn to_checkpoint_helper(&self) -> RPTPerCoreHelper {
        RPTPerCoreHelper {
            sets: self
                .sets
                .iter()
                .map(RPTSet::to_checkpoint_helper)
                .collect(),
        }
    }

    pub fn from_checkpoint_helper(helper: RPTPerCoreHelper) -> Self {
        assert_eq!(helper.sets.len(), RPT_SETS, "RPT table size mismatch");
        Self {
            sets: helper
                .sets
                .into_iter()
                .map(RPTSet::from_checkpoint_helper)
                .collect(),
        }
    }

    pub fn lookup(&mut self, request: &CacheBlockRequest, ts: u64) -> Option<i64> {
        let (tag, set_idx) = util::get_tag_setidx::<N_PC, RPT_SETS>(request.pc);
        match self.sets[set_idx as usize].lookup(tag, ts) {
            Some((way_idx, last_block, last_stride)) => {
                let new_stride: i64 = request.block_id as i64 - last_block as i64;
                self.sets[set_idx as usize].entries[way_idx as usize].last_block = request.block_id;
                self.sets[set_idx as usize].entries[way_idx as usize].last_stride = new_stride;
                if new_stride == last_stride {
                    return Some(new_stride);
                }
            }
            None => {
                self.sets[set_idx as usize].insert(tag, request.block_id, ts);
            },
        }
        None
    }

}

pub struct RPT<
    const CORE_COUNT: usize,    // Number of cores in the system
    const RPT_SETS: usize,      // Number of sets in the RPT
    const RPT_WAYS: usize,      // Assosciative ways in each set
    const N_PC: usize,          // Number of PC bits used to index into the table
> {
    tables: Box<[SpinMutex<RPTPerCore<RPT_SETS, RPT_WAYS, N_PC>>; CORE_COUNT]>,
}

impl<
    const CORE_COUNT: usize,
    const RPT_SETS: usize,
    const RPT_WAYS: usize,
    const N_PC: usize,
> RPT<CORE_COUNT, RPT_SETS, RPT_WAYS, N_PC>
{
    pub fn new() -> Self {
        Self {
            tables: Box::new([(); CORE_COUNT].map(|_| SpinMutex::new(RPTPerCore::<RPT_SETS, RPT_WAYS, N_PC>::new()))),
        }
    }

    pub fn to_checkpoint_helper(&self) -> Vec<RPTPerCoreHelper> {
        self.tables
            .iter()
            .map(|table| table.lock().to_checkpoint_helper())
            .collect()
    }

    pub fn from_checkpoint_helper(&mut self, helpers: Vec<RPTPerCoreHelper>) {
        assert_eq!(helpers.len(), CORE_COUNT, "RPT core count mismatch");
        for (table, helper) in self.tables.iter().zip(helpers) {
            *table.lock() = RPTPerCore::from_checkpoint_helper(helper);
        }
    }

    pub fn serialize(&self, name: &str, numa_node_id: usize) {
        use crate::parameter::USE_RKYV_SERIALIZATION;
        use zstd::Encoder;

        let helper = self.to_checkpoint_helper();
        let extension = if USE_RKYV_SERIALIZATION { "rkyv" } else { "json" };
        let file = std::fs::File::create(format!("{}/rpt-{}.{}.zstd", name, numa_node_id, extension))
            .unwrap();
        let mut encoder = Encoder::new(file, 0).unwrap();
        if USE_RKYV_SERIALIZATION {
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
            std::io::Write::write_all(&mut encoder, &bytes).unwrap();
        } else {
            serde_json::to_writer(&mut encoder, &helper).unwrap();
        }
        encoder.finish().unwrap();
    }

    pub fn deserialize(&mut self, name: &str, numa_node_id: usize) {
        use crate::parameter::USE_RKYV_SERIALIZATION;
        use zstd::Decoder;

        let extension = if USE_RKYV_SERIALIZATION { "rkyv" } else { "json" };
        let file = std::fs::File::open(format!("{}/rpt-{}.{}.zstd", name, numa_node_id, extension));
        let Ok(file) = file else {
            println!("Cannot load the RPT state. Checkpoint file is missing.");
            return;
        };
        let mut decoder = Decoder::new(file).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();
        let helper = if USE_RKYV_SERIALIZATION {
            rkyv::from_bytes::<Vec<RPTPerCoreHelper>, rkyv::rancor::Error>(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        self.from_checkpoint_helper(helper);
    }

    pub fn serialize_worker(&self, worker_id: usize, name: &str, numa_node_id: usize) {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, USE_RKYV_SERIALIZATION};

        let cores_per_worker = CORE_COUNT / CHECKPOINT_POOL_SIZE;
        let begin = worker_id * cores_per_worker;
        let end = begin + cores_per_worker;
        let helpers: Vec<_> = self.tables[begin..end]
            .iter()
            .map(|table| table.lock().to_checkpoint_helper())
            .collect();

        if USE_RKYV_SERIALIZATION {
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helpers).unwrap();
            crate::util::write_compressed(
                &format!("{}/rpt-{}-worker-{}.rkyv.zstd", name, numa_node_id, worker_id),
                &bytes,
            );
        } else {
            let bytes = serde_json::to_vec(&helpers).unwrap();
            crate::util::write_compressed(
                &format!("{}/rpt-{}-worker-{}.json.zstd", name, numa_node_id, worker_id),
                &bytes,
            );
        }
    }

    pub fn deserialize_worker(
        &mut self,
        worker_id: usize,
        name: &str,
        numa_node_id: usize,
    ) -> bool {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, USE_RKYV_SERIALIZATION};

        let cores_per_worker = CORE_COUNT / CHECKPOINT_POOL_SIZE;
        let begin = worker_id * cores_per_worker;
        let end = begin + cores_per_worker;
        let (extension, is_rkyv) = if USE_RKYV_SERIALIZATION {
            ("rkyv", true)
        } else {
            ("json", false)
        };
        let path = format!(
            "{}/rpt-{}-worker-{}.{}.zstd",
            name, numa_node_id, worker_id, extension
        );
        let Ok(file) = std::fs::File::open(path) else {
            return false;
        };
        let mut decoder = zstd::Decoder::new(file).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();
        let helpers: Vec<RPTPerCoreHelper> = if is_rkyv {
            rkyv::from_bytes::<Vec<RPTPerCoreHelper>, rkyv::rancor::Error>(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        assert_eq!(helpers.len(), cores_per_worker, "RPT worker core count mismatch");
        for (table, helper) in self.tables[begin..end].iter().zip(helpers) {
            *table.lock() = RPTPerCore::from_checkpoint_helper(helper);
        }
        true
    }

    pub fn lookup(&self, request: &CacheBlockRequest, ts: u64) -> Option<i64> {
        self.tables[request.core_id as usize].lock().lookup(request, ts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_helper_roundtrips_rpt_state() {
        let mut rpt = RPTPerCore::<2, 2, 16>::new();
        rpt.sets[1].entries[0] = RPTEntry {
            tag: 0x1234,
            last_block: 0x5678,
            last_stride: -4,
            ts: 99,
        };

        let restored =
            RPTPerCore::<2, 2, 16>::from_checkpoint_helper(rpt.to_checkpoint_helper());
        let entry = restored.sets[1].entries[0];

        assert_eq!(entry.tag, 0x1234);
        assert_eq!(entry.last_block, 0x5678);
        assert_eq!(entry.last_stride, -4);
        assert_eq!(entry.ts, 99);
    }
}