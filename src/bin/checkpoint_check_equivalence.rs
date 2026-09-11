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

use std::io::Read;
use std::path::Path;

use worm_cache::checkpoint::helpers::{
    DirectoryHelper, FetchUnitHelper, HarvardPrivateCacheHelper, MMUsHelper, PHTPerCoreHelper,
    SharedCacheHelper, UnifiedPrivateCacheHelper,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <sequential_dir> <parallel_dir>", args[0]);
        eprintln!("Example: {} snap1.uarch snap1.uarch_par", args[0]);
        std::process::exit(1);
    }
    let seq_dir = &args[1];
    let par_dir = &args[2];

    let mut ok = true;
    ok &= check_harvard(seq_dir, par_dir);
    ok &= check_unified(seq_dir, par_dir);
    ok &= check_pht(seq_dir, par_dir);
    ok &= check_fetch(seq_dir, par_dir);
    ok &= check_mmus(seq_dir, par_dir);
    ok &= check_directory(seq_dir, par_dir);
    ok &= check_llc(seq_dir, par_dir);

    if ok {
        println!("All components are EQUIVALENT.");
    } else {
        std::process::exit(1);
    }
}

fn load_rkyv_bytes(path: &str) -> Vec<u8> {
    let file = std::fs::File::open(path).unwrap();
    let mut decoder = zstd::Decoder::new(file).unwrap();
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).unwrap();
    bytes
}

fn detect_worker_count(dir: &str, stem: &str, kind: &str) -> usize {
    let mut count = 0;
    loop {
        let rkyv = format!("{}/{}-{}-{}.rkyv.zstd", dir, stem, kind, count);
        let json = format!("{}/{}-{}-{}.json.zstd", dir, stem, kind, count);
        if Path::new(&rkyv).exists() || Path::new(&json).exists() {
            count += 1;
        } else {
            break;
        }
    }
    count
}

// ============== Vec-of-per-core components (harvard, unified, pht) ==============

macro_rules! check_vec_component {
    ($func:ident, $ty:ty, $fname:expr) => {
        fn $func(seq_dir: &str, par_dir: &str) -> bool {
            let name = $fname;

            let rkyv = format!("{}/{}-0.rkyv.zstd", seq_dir, name);
            let json = format!("{}/{}-0.json.zstd", seq_dir, name);
            let seq: Vec<$ty> = if Path::new(&rkyv).exists() {
                let bytes = load_rkyv_bytes(&rkyv);
                rkyv::from_bytes::<Vec<$ty>, rkyv::rancor::Error>(&bytes).unwrap()
            } else if Path::new(&json).exists() {
                let file = std::fs::File::open(&json).unwrap();
                let decoder = zstd::Decoder::new(file).unwrap();
                serde_json::from_reader(decoder).unwrap()
            } else {
                return true;
            };

            let stem = format!("{}-0", name);
            let worker_count = detect_worker_count(par_dir, &stem, "worker");

            let mut par: Vec<$ty> = Vec::new();
            for wid in 0..worker_count {
                let rkyv_w = format!("{}/{}-0-worker-{}.rkyv.zstd", par_dir, name, wid);
                let json_w = format!("{}/{}-0-worker-{}.json.zstd", par_dir, name, wid);
                let worker: Vec<$ty> = if Path::new(&rkyv_w).exists() {
                    let bytes = load_rkyv_bytes(&rkyv_w);
                    rkyv::from_bytes::<Vec<$ty>, rkyv::rancor::Error>(&bytes).unwrap()
                } else if Path::new(&json_w).exists() {
                    let file = std::fs::File::open(&json_w).unwrap();
                    let decoder = zstd::Decoder::new(file).unwrap();
                    serde_json::from_reader(decoder).unwrap()
                } else {
                    break;
                };
                par.extend(worker);
            }

            if seq != par {
                println!("{}: DIFFER", name);
                return false;
            }
            println!("{}: EQUIVALENT ({} entries)", name, seq.len());
            true
        }
    };
}

check_vec_component!(check_harvard, HarvardPrivateCacheHelper, "harvard");
check_vec_component!(check_unified, UnifiedPrivateCacheHelper, "unified");
check_vec_component!(check_pht, PHTPerCoreHelper, "pht");

// ============== Wrapper-object components (fetch, mmus, directory, llc) ==============

macro_rules! check_obj_component {
    ($func:ident, $outer_ty:ty, $inner_ty:ty, $fname:expr, $field:ident, $kind:expr, $seq_suffix:expr) => {
        fn $func(seq_dir: &str, par_dir: &str) -> bool {
            let name = $fname;
            let seq_suffix = $seq_suffix;

            let rkyv = format!("{}/{}{}.rkyv.zstd", seq_dir, name, seq_suffix);
            let json = format!("{}/{}{}.json.zstd", seq_dir, name, seq_suffix);
            let seq_outer: $outer_ty = if Path::new(&rkyv).exists() {
                let bytes = load_rkyv_bytes(&rkyv);
                rkyv::from_bytes::<$outer_ty, rkyv::rancor::Error>(&bytes).unwrap()
            } else if Path::new(&json).exists() {
                let file = std::fs::File::open(&json).unwrap();
                let decoder = zstd::Decoder::new(file).unwrap();
                serde_json::from_reader(decoder).unwrap()
            } else {
                return true;
            };

            let seq_vec: Vec<$inner_ty> = seq_outer.$field;

            let kind = $kind;
            let suffix = if name == "fetch" { "" } else { "-0" };
            let stem = format!("{}{}", name, suffix);
            let worker_count = detect_worker_count(par_dir, &stem, kind);

            let mut par_vec: Vec<$inner_ty> = Vec::new();
            for wid in 0..worker_count {
                let rkyv_w = if name == "fetch" {
                    format!("{}/{}-{}-{}.rkyv.zstd", par_dir, name, kind, wid)
                } else {
                    format!("{}/{}{}-{}-{}.rkyv.zstd", par_dir, name, suffix, kind, wid)
                };
                let json_w = if name == "fetch" {
                    format!("{}/{}-{}-{}.json.zstd", par_dir, name, kind, wid)
                } else {
                    format!("{}/{}{}-{}-{}.json.zstd", par_dir, name, suffix, kind, wid)
                };

                let worker_outer: $outer_ty = if Path::new(&rkyv_w).exists() {
                    let bytes = load_rkyv_bytes(&rkyv_w);
                    rkyv::from_bytes::<$outer_ty, rkyv::rancor::Error>(&bytes).unwrap()
                } else if Path::new(&json_w).exists() {
                    let file = std::fs::File::open(&json_w).unwrap();
                    let decoder = zstd::Decoder::new(file).unwrap();
                    serde_json::from_reader(decoder).unwrap()
                } else {
                    break;
                };

                par_vec.extend(worker_outer.$field);
            }

            if seq_vec != par_vec {
                println!("{}: DIFFER", name);
                return false;
            }
            println!("{}: EQUIVALENT ({} entries)", name, seq_vec.len());
            true
        }
    };
}

check_obj_component!(check_fetch, FetchUnitHelper, worm_cache::checkpoint::helpers::PerCoreFetchUnitHelper, "fetch", private_units, "worker", "");
check_obj_component!(check_mmus, MMUsHelper, worm_cache::checkpoint::helpers::MMUHelper, "mmus", mmus, "worker", "-0");
check_obj_component!(check_directory, DirectoryHelper, worm_cache::checkpoint::helpers::DirectorySetHelper, "directory", sets, "shard", "-0");
check_obj_component!(check_llc, SharedCacheHelper, worm_cache::checkpoint::helpers::SharedCacheSetHelper, "llc", blocks, "shard", "-0");
