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
    {parameter::N_ACC},
    {parameter::N_FILTER},
    {parameter::PHT_SETS},
    {parameter::PHT_WAYS},
    {parameter::N_BLK},
    {parameter::ROT},
    { parameter::SEP_RDWR },
    { parameter::SAT_CNT },
    {parameter::PERFECT_PHT},
    {parameter::RPT_SETS},
    {parameter::RPT_WAYS},
    {parameter::N_PC},
    {parameter::LOOKAHEAD},
>;

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

    let (mut data_all, mut old_data_l1d_miss, mut old_data_llc_miss, mut new_data_l1d_miss, mut new_data_llc_miss): (usize, usize, usize, usize, usize) = (0, 0, 0, 0, 0);
    let (mut instr_all, mut old_instr_l1i_miss, mut old_instr_llc_miss, mut new_instr_l1i_miss, mut new_instr_llc_miss): (usize, usize, usize, usize, usize) = (0, 0, 0, 0, 0);

    let mut prev_ts = 0;
    for (idx, result) in rdr.records().enumerate() {
        if (idx+1) % 100_000_000 == 0 {
            println!("Processed {} records", (idx+1));
            let old_l1d_mr = old_data_l1d_miss as f64 / data_all as f64 * 100.0;
            let new_l1d_mr = new_data_l1d_miss as f64 / data_all as f64 * 100.0;
            let old_llc_mr_data = old_data_llc_miss as f64 / data_all as f64 * 100.0;
            let new_llc_mr_data = new_data_llc_miss as f64 / data_all as f64 * 100.0;
            let old_l1i_mr = old_instr_l1i_miss as f64 / instr_all as f64 * 100.0;
            let new_l1i_mr = new_instr_l1i_miss as f64 / instr_all as f64 * 100.0;
            let old_llc_mr_instr = old_instr_llc_miss as f64 / instr_all as f64 * 100.0;
            let new_llc_mr_instr = new_instr_llc_miss as f64 / instr_all as f64 * 100.0;

            println!("Old L1D Miss Rate: {:.2}%, New L1D Miss Rate: {:.2}%, Old LLC Miss Rate (Data): {:.2}%, New LLC Miss Rate (Data): {:.2}%", old_l1d_mr, new_l1d_mr, old_llc_mr_data, new_llc_mr_data);
            println!("Old L1I Miss Rate: {:.2}%, New L1I Miss Rate: {:.2}%, Old LLC Miss Rate (Instr): {:.2}%, New LLC Miss Rate (Instr): {:.2}%", old_l1i_mr, new_l1i_mr, old_llc_mr_instr, new_llc_mr_instr);
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
                let cnt = (idx + 1) > 1_000_000_000;
                let core_id = record[1].parse::<u32>().unwrap();
                let block_id = record[2].parse::<u64>().unwrap();
                let access_code = record[3].parse::<u8>().unwrap();
                let is_os = record[4].parse::<bool>().unwrap();
                let pc = record[5].parse::<u64>().unwrap();
                let code = record[6].parse::<u8>().unwrap();
                if prev_ts != 0 && (ts as f64) > 1.5*(prev_ts as f64) {
                    eprintln!("Warning: Timestamp gap detected: {} -> {}", prev_ts, ts);
                    continue;
                }
                prev_ts = ts;

                let is_data = access_code == 0 || access_code == 1;
                let is_instr: bool = access_code == 2;
                if is_data && cnt {
                    data_all += 1;
                    match code {
                        0 => {},
                        1 => {old_data_l1d_miss += 1},                          // Only L1 Miss
                        2 => {old_data_l1d_miss += 1; old_data_llc_miss += 1},  // L1 Miss + LLC Miss
                        3 => {old_data_l1d_miss += 1},                          // Hit in other private cache (treated as L1 Miss)
                        4 => {old_data_l1d_miss += 1},                          // Miss due to permission (treated as L1 Miss)
                        5 => {},                                                // Unknown, we don't know where it misses, so we don't count it in the miss statistics
                        _ => unreachable!("Invalid code"),
                    };
                }
                if is_instr && cnt {
                    instr_all += 1;
                    match code {
                        0 => {},
                        1 => {old_instr_l1i_miss += 1},                          // Only L1 Miss
                        2 => {old_instr_l1i_miss += 1; old_instr_llc_miss += 1},  // L1 Miss + LLC Miss
                        3 => {old_instr_l1i_miss += 1},                          // Hit in other private cache (treated as L1 Miss)
                        4 => {old_instr_l1i_miss += 1},                          // Miss due to permission (treated as L1 Miss)
                        5 => {},                                                // Unknown, we don't know where it misses, so we don't count it in the miss statistics
                        _ => unreachable!("Invalid code"),
                    };
                }
                let access_type = match access_code {
                    0 => CacheAccessType::DataRead,
                    1 => CacheAccessType::DataWrite,
                    2 => CacheAccessType::InstructionFetch,
                    3 => CacheAccessType::PrefetchRead,
                    4 => CacheAccessType::PrefetchWrite,
                    5 => CacheAccessType::PageWalkRead,
                    _ => unreachable!("Invalid access type"),
                };
                let req = CacheBlockRequest{
                    core_id,
                    block_id,
                    access_type,
                    is_os,
                    pc,
                };
                let (result, _) = mh.access_memory_pblock_id(&req, ts);
                if parameter::ADJACENT_LINE_PREFETCHING && is_instr {
                    let mut prefetch_request = req.clone();
                    prefetch_request.block_id += 1;
                    prefetch_request.access_type = prefetch_request.get_prefetch_type();
                    mh.access_memory_pblock_id(&prefetch_request, ts);
                }
                if parameter::SMS_PREFETCHING && is_data {
                    mh.prefetch_blocks_sms(&req, ts);
                    mh.record_access(&req, ts);
                }
                if parameter::STRIDE_PREFETCHING && is_data {
                    mh.prefetch_blocks_stride(&req, ts);
                }
                let new_code: u8 = match result {
                    CacheHierarchyAccessResult::HitInSelfPrivateCache => 0,
                    CacheHierarchyAccessResult::HitInSharedCache => 1,
                    CacheHierarchyAccessResult::Miss => 2,
                    CacheHierarchyAccessResult::HitInOtherPrivateCache => 3,
                    CacheHierarchyAccessResult::MissDueToPermission => 4,
                    CacheHierarchyAccessResult::Unknown => 5,
                };
                if is_data && cnt {
                    match new_code {
                        0 => {},
                        1 => {new_data_l1d_miss += 1},                          // Only L1 Miss
                        2 => {new_data_l1d_miss += 1; new_data_llc_miss += 1},  // L1 Miss + LLC Miss
                        3 => {new_data_l1d_miss += 1},                          // Hit in other private cache (treated as L1 Miss)
                        4 => {new_data_l1d_miss += 1},                          // Miss due to permission (treated as L1 Miss)
                        5 => {},                                                // Unknown, we don't know where it misses, so we don't count it in the miss statistics
                        _ => unreachable!("Invalid code"),
                    };
                }
                if is_instr && cnt {
                    match new_code {
                        0 => {},
                        1 => {new_instr_l1i_miss += 1},                          // Only L1 Miss
                        2 => {new_instr_l1i_miss += 1; new_instr_llc_miss += 1},  // L1 Miss + LLC Miss
                        3 => {new_instr_l1i_miss += 1},                          // Hit in other private cache (treated as L1 Miss)
                        4 => {new_instr_l1i_miss += 1},                          // Miss due to permission (treated as L1 Miss)
                        5 => {},                                                // Unknown, we don't know where it misses, so we don't count it in the miss statistics
                        _ => unreachable!("Invalid code"),
                    };
                }
            }
            Err(e) => {
                eprintln!("Error reading record: {}", e);
            }
        }
    }
    let old_l1d_mr = old_data_l1d_miss as f64 / data_all as f64 * 100.0;
    let new_l1d_mr = new_data_l1d_miss as f64 / data_all as f64 * 100.0;
    let old_llc_mr_data = old_data_llc_miss as f64 / data_all as f64 * 100.0;
    let new_llc_mr_data = new_data_llc_miss as f64 / data_all as f64 * 100.0;
    let old_l1i_mr = old_instr_l1i_miss as f64 / instr_all as f64 * 100.0;
    let new_l1i_mr = new_instr_l1i_miss as f64 / instr_all as f64 * 100.0;
    let old_llc_mr_instr = old_instr_llc_miss as f64 / instr_all as f64 * 100.0;
    let new_llc_mr_instr = new_instr_llc_miss as f64 / instr_all as f64 * 100.0;

    println!("Old L1D Miss Rate: {:.2}%, New L1D Miss Rate: {:.2}%, Old LLC Miss Rate (Data): {:.2}%, New LLC Miss Rate (Data): {:.2}%", old_l1d_mr, new_l1d_mr, old_llc_mr_data, new_llc_mr_data);
    println!("Old L1I Miss Rate: {:.2}%, New L1I Miss Rate: {:.2}%, Old LLC Miss Rate (Instr): {:.2}%, New LLC Miss Rate (Instr): {:.2}%", old_l1i_mr, new_l1i_mr, old_llc_mr_instr, new_llc_mr_instr);
}
