// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this
//    list of conditions and the following disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.
//
// 3. Neither the name of the PARSA, EPFL
//    nor the names of its contributors may be used to endorse or promote
//    products derived from this software without specific prior written
//    permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

// the entrypoint of the functional model of the simulator
// this program takes a trace file as input and outputs the cache state.
// the trace file is encoded in binary and continuous records in the following data structure

use std::env;
use std::io::BufReader;
use std::fs::File;
use worm_cache::components::cache_hierarchy::hierarchy::ParallelMemoryHierarchy;
use worm_cache::components::cache_hierarchy::CacheBlockRequest;
use worm_cache::components::cache_hierarchy::common::{CacheAccessType, InfiniteDirectory, ParallelHarvardPrivateCache, ParallelLRUSharedCache};
use worm_cache::components::cache_hierarchy::common::statistics::{SharedCacheSetMissStatistics, ZeroSharedCacheSetStatistics};
use worm_cache::components::cache_hierarchy::MemoryHierarchy;
use worm_cache::components::cache_hierarchy::common::CacheHierarchyAccessResult;

use worm_cache::components::cache_hierarchy::mmu::NoMMU;
use worm_cache::parameter;
// use worm_cache::components::memory_ts::{PrivateCacheParameters, TimestampMemoryHierarchy};

const ALLOCATED_CORE_COUNT: usize = if parameter::MEASURE_HALF_OF_CORES {
    parameter::CORE_COUNT / 2
} else {
    parameter::CORE_COUNT
};

pub struct DummyParser;
pub trait SharedCacheStatisticsParser<const ENABLE_STATISTICS: bool> {
    type Output;
}

impl SharedCacheStatisticsParser<true> for DummyParser {
    type Output = SharedCacheSetMissStatistics;
}

impl SharedCacheStatisticsParser<false> for DummyParser {
    type Output = ZeroSharedCacheSetStatistics;
}
type SharedCacheStatisticsWithPlugin =
    <DummyParser as SharedCacheStatisticsParser<{ parameter::ENABLE_STATISTICS }>>::Output;

type MH = ParallelMemoryHierarchy<
    NoMMU,
    ParallelHarvardPrivateCache<
        { ALLOCATED_CORE_COUNT },
        { parameter::HARVARD_PRI_I_CACHE_SET },
        { parameter::HARVARD_PRI_I_CACHE_ASSO },
        { parameter::HARVARD_PRI_D_CACHE_SET },
        { parameter::HARVARD_PRI_D_CACHE_ASSO },
    >,
    ParallelLRUSharedCache<
        SharedCacheStatisticsWithPlugin,
        { parameter::SHARED_CACHE_SET },
        { parameter::SHARED_CACHE_ASSO },
        { parameter::SHARED_CACHE_EXCLUSIVE },
    >,
    InfiniteDirectory<{ parameter::INFINITE_DIRECTORY_SHARED_COUNT }>,
    { parameter::SHARED_CACHE_FILL_WITH_PRIVATE_CACHE },
    { parameter::SHARED_CACHE_FILL_ON_CLEAN_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_DIRTY_EVICTION },
    { parameter::SHARED_CACHE_FILL_ON_REPLICA_CREATION },
    { ALLOCATED_CORE_COUNT },
    { parameter::N_ACC },
    { parameter::N_FILTER },
    { parameter::PHT_SETS },
    { parameter::PHT_WAYS },
    { parameter::N_BLK },
    { parameter::ROT },
    { parameter::SEP_RDWR },
    { parameter::SAT_CNT },
    { parameter::PERFECT_PHT },
>;

pub struct Metric {
    all: usize,
    l1_miss: usize,
    llc_miss: usize,
}

impl Metric {
    pub fn new() -> Self {
        Self {
            all: 0,
            l1_miss: 0,
            llc_miss: 0,
        }
    }
    pub fn record(&mut self, result: &CacheHierarchyAccessResult) {
        self.all += 1;
        match result {
            CacheHierarchyAccessResult::HitInSelfPrivateCache => {},
            CacheHierarchyAccessResult::MissDueToPermission => {self.l1_miss += 1},
            CacheHierarchyAccessResult::HitInOtherPrivateCache => {self.l1_miss += 1},
            CacheHierarchyAccessResult::HitInSharedCache => {self.l1_miss += 1},
            CacheHierarchyAccessResult::Miss => {self.l1_miss += 1; self.llc_miss += 1},
            CacheHierarchyAccessResult::Unknown => {},
        }
    }
    pub fn calc_mpa(&self) -> (f64, f64) {
        if self.all == 0 {
            return (0.0, 0.0);
        }
        let l1_mpa = (self.l1_miss as f64 / self.all as f64) * 100.0;
        let llc_mpa = (self.llc_miss as f64 / self.all as f64) * 100.0;
        (l1_mpa, llc_mpa)
    }
}

pub struct MetricTracker {
    data: Metric,
    instr: Metric,
}

impl MetricTracker {
    pub fn new() -> Self {
        Self {
            data: Metric::new(),
            instr: Metric::new(),
        }
    }
    fn record(&mut self, access_type: &CacheAccessType, result: &CacheHierarchyAccessResult) {
        match access_type {
            CacheAccessType::InstructionFetch => {
                self.instr.record(result);
            },
            CacheAccessType::DataRead | CacheAccessType::DataWrite | CacheAccessType::PageWalkRead => {
                self.data.record(result);
            },
            CacheAccessType::PrefetchRead | CacheAccessType::PrefetchWrite => {
                assert!(false, "Prefetch accesses should not be recorded in the metrics");
            },
        }
    }
    fn print(&self) {
        let (l1d_mpa, llcd_mpa) = self.data.calc_mpa();
        let (l1i_mpa, llci_mpa) = self.instr.calc_mpa();
        println!("L1D_MPA: {:.2}%, LLCD_MPA: {:.2}%, L1I_MPA: {:.2}%, LLCI_MPA: {:.2}%", l1d_mpa, llcd_mpa, l1i_mpa, llci_mpa);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let filename = if args.len() > 1 {
        &args[1]
    } else {
        "trace.log"
    };

    let file = match File::open(filename) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open file {}: {}", filename, e);
            return;
        }
    };

    // Skip the first 25 lines directly from the BufReader
    let mut buf_reader = BufReader::new(file);
    use std::io::BufRead;
    for _ in 0..25 {
        let mut dummy = String::new();
        let _ = buf_reader.read_line(&mut dummy);
    }

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(buf_reader);

    let mh = MH::new();
    let mut stats = MetricTracker::new();

    let mut prev_ts = 0;
    for (idx, result) in rdr.records().enumerate() {
        if (idx+1) % 100_000_000 == 0 {
            println!("Processed {} records", (idx+1));
            stats.print();
        }
        match result {
            Ok(record) => {
                if record.len() != 7 {
                    eprintln!("Invalid record length: expected 7, got {}", record.len());
                    continue;
                }
                // println!("Processing record: {:?}", record);
                let ts = record[0].parse::<u64>().unwrap_or(0);
                if ts == 0 {
                    eprintln!("Invalid timestamp: {:?}", record);
                    continue;
                }
                let recording = (idx + 1) > 1_000_000_000;
                let core_id = record[1].parse::<u32>().unwrap();
                let block_id = record[2].parse::<u64>().unwrap();
                let access_code = record[3].parse::<u8>().unwrap();
                let is_os = record[4].parse::<bool>().unwrap();
                let pc = record[5].parse::<u64>().unwrap();
                // let code = record[6].parse::<u8>().unwrap();
                if prev_ts != 0 && (ts as f64) > 1.5*(prev_ts as f64) {
                    eprintln!("Warning: Timestamp gap detected: {} -> {}", prev_ts, ts);
                    continue;
                }
                prev_ts = ts;

                let access_type = match access_code {
                    0 => CacheAccessType::DataRead,
                    1 => CacheAccessType::DataWrite,
                    2 => CacheAccessType::InstructionFetch,
                    3 => CacheAccessType::PrefetchRead,
                    4 => CacheAccessType::PrefetchWrite,
                    5 => CacheAccessType::PageWalkRead,
                    _ => unreachable!("Invalid access type"),
                };
                let is_instr = access_type == CacheAccessType::InstructionFetch;
                let is_data = access_type == CacheAccessType::DataRead || access_type == CacheAccessType::DataWrite || access_type == CacheAccessType::PageWalkRead;
                assert!(is_instr || is_data, "Invalid access type for recording metrics");
                let req = CacheBlockRequest{
                    core_id,
                    block_id,
                    access_type,
                    is_os,
                    pc,
                };
                let (result, _) = mh.access_memory_pblock_id(&req, ts);
                if recording {
                    stats.record(&access_type, &result);
                }
                if parameter::ADJACENT_LINE_PREFETCHING && is_instr {
                    let mut prefetch_request = req.clone();
                    prefetch_request.block_id += 1;
                    prefetch_request.access_type = prefetch_request.get_prefetch_type();
                    mh.access_memory_pblock_id(&prefetch_request, ts);
                }
                if parameter::SMS_PREFETCHING && is_data {
                    mh.prefetch_blocks(&req, ts);
                    mh.record_access(&req, ts);
                }
            }
            Err(e) => {
                eprintln!("Error reading record: {}", e);
            }
        }
    }
    stats.print();
}
