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

use std::env;
use std::{fs::File, io::BufReader, io::Write, process::exit};
use worm_cache::{
    components::cache_hierarchy::{
        common::{agt::ParallelAGT, pht::ParallelPHT, CacheAccessType},
        CacheBlockRequest,
    },
    parameter,
};

const ALLOCATED_CORE_COUNT: usize = if parameter::MEASURE_HALF_OF_CORES {
    parameter::CORE_COUNT / 2
} else {
    parameter::CORE_COUNT
};

type AGT = ParallelAGT<
    { ALLOCATED_CORE_COUNT },
    { parameter::N_ACC },
    { parameter::N_FILTER },
    { parameter::N_BLK },
>;

type PHT = ParallelPHT<
    { ALLOCATED_CORE_COUNT },
    { parameter::PHT_SETS },
    { parameter::PHT_WAYS },
    { parameter::N_BLK },
    { parameter::ROT },
    { parameter::SEP_RDWR },
    { parameter::SAT_CNT },
    { parameter::PERFECT_PHT },
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

    let buf_rdr = BufReader::new(file);
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(buf_rdr);

    let agt = AGT::new();
    let mut pht = PHT::new();
    let core_id = 0;

    let out_file = "sms_ipc_out.log";
    let mut out_f = match File::create(out_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create output file {}: {}", out_file, e);
            return;
        }
    };

    pht.deserialize("/root/sms-exps/pf/da/snapshot_2.uarch", core_id as usize);

    for record in rdr.records() {
        match record {
            Ok(record) => {
                let record_str = record.iter().collect::<Vec<&str>>().join(",") + "\n";
                if record.len() != 5 {
                    eprintln!(
                        "Invalid record length: {} for record: {}",
                        record.len(),
                        record_str
                    );
                    exit(1);
                }
                println!("Processing record: {}", record_str);
                let op = record[0].parse::<u32>().unwrap();
                let block_id = record[1].parse::<u64>().unwrap();
                let pc = record[2].parse::<u64>().unwrap();
                let is_store = record[3].parse::<u32>().unwrap() != 0;
                let ts = record[4].parse::<u64>().unwrap();

                let access_type = if is_store {
                    CacheAccessType::DataWrite
                } else {
                    CacheAccessType::DataRead
                };
                let req = CacheBlockRequest {
                    core_id,
                    block_id,
                    access_type,
                    is_os: false,
                    pc,
                };
                match op {
                    0 => {
                        out_f.write(record_str.as_bytes()).unwrap();
                        let addrs = pht.lookup(&req, ts);
                        for addr in &addrs {
                            if *addr == block_id {
                                continue;
                            }
                            out_f
                                .write(format!("3,{},{},0,{}\n", addr, pc, ts).as_bytes())
                                .unwrap();
                        }
                    }
                    1 => {
                        out_f.write(record_str.as_bytes()).unwrap();
                        match agt.record(&req, ts) {
                            Some(entry) => pht.insert(&entry, core_id as usize),
                            None => {}
                        }
                    }
                    2 => {
                        out_f.write(record_str.as_bytes()).unwrap();
                        match agt.evict(&req) {
                            Some(entry) => pht.insert(&entry, core_id as usize),
                            None => {}
                        }
                    }
                    3 => {}
                    _ => {
                        eprintln!("Unknown operation encoutered! op: {}", op);
                        exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading record: {}", e);
                continue;
            }
        }
    }
}
