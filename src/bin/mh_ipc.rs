use std::env;
use std::io::{BufReader};
use std::fs::File;
use worm_cache::components::cache_hierarchy::hierarchy::ParallelMemoryHierarchy;
use worm_cache::components::cache_hierarchy::CacheBlockRequest;
use worm_cache::components::cache_hierarchy::common::{CacheAccessType, InfiniteDirectory, ParallelHarvardPrivateCache, ParallelLRUSharedCache};
use worm_cache::components::cache_hierarchy::common::statistics::{SharedCacheSetMissStatistics, ZeroSharedCacheSetStatistics};
use worm_cache::components::cache_hierarchy::MemoryHierarchy;
use worm_cache::components::cache_hierarchy::common::CacheHierarchyAccessResult;
use std::process::exit;

use worm_cache::components::cache_hierarchy::mmu::NoMMU;
use worm_cache::parameter;

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
        {ALLOCATED_CORE_COUNT}, 
        {parameter::HARVARD_PRI_I_CACHE_SET}, 
        {parameter::HARVARD_PRI_I_CACHE_ASSO}, 
        {parameter::HARVARD_PRI_D_CACHE_SET}, 
        {parameter::HARVARD_PRI_D_CACHE_ASSO}
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
>;

fn main() {
    let args: Vec<String> = env::args().collect();
    let filename = if args.len() > 1 {
        &args[1]
    } else {
        "trace.log"
    };
    let uarch_state_filename: &str = if args.len() > 2 {
        &args[2]
    } else {
        "snapshot.uarch"
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

    let mut ts: u64 = match std::fs::read_to_string(format!("{}/timestamp", uarch_state_filename)) {
        Ok(contents) => match contents.trim().parse::<u64>() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to parse timestamp from ts.txt: {}", e);
                exit(1);
            }
        },
        Err(e) => {
            eprintln!("Failed to open ts.txt: {}", e);
            exit(1);
        }
    };
    println!("Starting from timestamp: {}", ts);

    let mut mh = MH::new();
    let core_id = 0;
    mh.deserialize(uarch_state_filename, core_id as usize);

    let mut hit_count = 0;
    let mut miss_count = 0;
    for (iter, record) in rdr.records().enumerate() {
        if (iter + 1) % 100_000 == 0 {
            let mr = (miss_count as f64 / (hit_count + miss_count) as f64) * 100.0;
            println!("Processed {} records. Hits: {}, Misses: {}, Miss Rate: {:.1}%", iter + 1, hit_count, miss_count, mr);
        }
        match record {
            Ok(record) => {
                let record_str = record.iter().collect::<Vec<&str>>().join(",");
                if record.len() != 5 {
                    eprintln!("Invalid record length: {} for record: {}", record.len(), record_str);
                    exit(1);
                }
                // println!("Processing record: {}", record_str);
                let op = record[0].parse::<u32>().unwrap();
                let block_id = record[1].parse::<u64>().unwrap();
                let pc = record[2].parse::<u64>().unwrap();
                let is_store = record[3].parse::<u32>().unwrap() != 0;
                // let ts = record[4].parse::<u64>().unwrap();

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
                        println!("{}", record_str);
                        let (result, _) = mh.access_memory_pblock_id(&req, ts);
                        mh.prefetch_blocks(&req, ts);
                        match result {
                            CacheHierarchyAccessResult::HitInSelfPrivateCache => {
                                hit_count += 1;
                            },
                            _ => {
                                miss_count += 1;
                            }
                        }
                    }
                    1 => {
                        println!("{}", record_str);
                        mh.record_access(&req, ts);
                    }
                    2 => {}
                    3 => {}
                    _ => {
                        eprintln!("Unknown operation encoutered! op: {}", op);
                        exit(1);
                    }
                }
                ts += 1;
            }
            Err(e) => {
                eprintln!("Error reading record: {}", e);
                continue;
            }
        }
    }
    let mr = (miss_count as f64 / (hit_count + miss_count) as f64) * 100.0;
    println!("Final Hits: {}, Misses: {}", hit_count, miss_count);
    println!("Final Miss Rate: {:.1}%", mr);
}