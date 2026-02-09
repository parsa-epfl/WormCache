use crate::components::cache_hierarchy::CacheBlockRequest;
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

    pub fn lookup(&self, request: &CacheBlockRequest, ts: u64) -> Option<i64> {
        self.tables[request.core_id as usize].lock().lookup(request, ts)
    }
}