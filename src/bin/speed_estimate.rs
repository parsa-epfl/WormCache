use once_cell::sync::OnceCell;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use std::env;
use std::fs;
use std::process::exit;
use std::sync;
use std::thread;
use std::time::Instant;

use worm_cache::cache::parallel::{BlockState, ParallelCache};
use worm_cache::cache::CacheReturnResult;

const LLC_SET: usize = 64 * 1024;

static SEEDS: OnceCell<Vec<u32>> = once_cell::sync::OnceCell::new();
static THREAD_BARRIER: OnceCell<sync::Barrier> = once_cell::sync::OnceCell::new();
static CACHE: OnceCell<ParallelCache> = once_cell::sync::OnceCell::new();
static REPEAT_TIME: OnceCell<usize> = once_cell::sync::OnceCell::new();

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        println!("Usage: {} <thread_number> <repeat_time>", args[0]);
        exit(-1)
    }

    let thread_count = args[1].parse::<usize>().unwrap();
    let repeat_time = args[2].parse::<usize>().unwrap();
    REPEAT_TIME.set(repeat_time).unwrap();

    let f = fs::read_to_string("./llc_counter.log").unwrap();

    let seeds: Vec<_> = f
        .split(" ")
        .filter_map(|s| {
            return match s.parse::<u32>() {
                Ok(v) => Some(v),
                Err(_) => None,
            };
        })
        .collect();
    SEEDS.set(seeds).unwrap();
    println!("Seed count: {}", SEEDS.get().unwrap().len());

    THREAD_BARRIER
        .set(sync::Barrier::new(thread_count))
        .unwrap();
    CACHE.set(ParallelCache::new(LLC_SET, 8)).unwrap();

    let threads_handlers: Vec<_> = (0..thread_count)
        .map(|_| {
            return thread::spawn(|| {
                let repeat_time = *REPEAT_TIME.get().unwrap();
                // step1, generate 10M requests
                let dist = WeightedIndex::new(SEEDS.get().unwrap()).unwrap();
                let mut rng = thread_rng();
                let seeds: Vec<_> = (0..64 * 1024 * 1024).map(|_|{
                    let t1 = dist.sample(&mut rng);
                    let t2 = dist.sample(&mut rng);
                    return t1 * LLC_SET + t2;
                }).collect();
                let local_cache = CACHE.get().unwrap();
                THREAD_BARRIER.wait();
                let mut cnt = 0u64;
                // now, send the request to the cache and start timing.
                let t = Instant::now();
                for _ in 0..repeat_time {
                    for s in seeds.iter() {
                        match local_cache.update(*s, BlockState::Exclusive) {
                            CacheReturnResult::Miss => cnt += 1,
                            CacheReturnResult::Hit => {}
                            CacheReturnResult::MissWithEviction(_) => {}
                            CacheReturnResult::MissWithDirtyEviction(_) => {}
                            CacheReturnResult::MissWithWrongPermission => {}
                        }
                        std::thread::sleep(std::time::Duration::from_micros(1));
                    }
                }
                let el = t.elapsed();
                THREAD_BARRIER.wait();
                println!("Duration: {}ms, miss: {}", el.as_millis(), cnt);
            });
        })
        .collect();

    threads_handlers.into_iter().for_each(|x| {
        x.join().unwrap();
    });
}
