use once_cell::sync::OnceCell;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use std::fs;
use std::sync;
use std::thread;
use chrono::Local;

use worm_cache::cache::parallel::{BlockState, CacheEntry, ParallelCache};
use worm_cache::cache::CacheReturnResult;

const THREAD_COUNT: usize = 64;
const LLC_SET: usize = 64 * 1024;

static SEEDS: OnceCell<Vec<u32>> = once_cell::sync::OnceCell::new();
static THREAD_BARRIER: OnceCell<sync::Barrier> = once_cell::sync::OnceCell::new();
static CACHE: OnceCell<ParallelCache<CacheEntry>> = once_cell::sync::OnceCell::new();

fn main() {
    let mut f = fs::read_to_string("./llc_counter.log").unwrap();
    let seeds: Vec<_> = f
        .split(" ")
        .map(|s| return s.parse::<u32>().unwrap())
        .collect();
    SEEDS.set(seeds).unwrap();
    THREAD_BARRIER
        .set(sync::Barrier::new(THREAD_COUNT + 1))
        .unwrap();
    CACHE.set(ParallelCache::new(LLC_SET, 8)).unwrap();

    let threads_handlers: Vec<_> = (0..THREAD_COUNT)
        .map(|_| {
            return thread::spawn(|| {
                // step1, generate 10M requests
                let dist = WeightedIndex::new(SEEDS.get().unwrap()).unwrap();
                let mut rng = thread_rng();
                let seeds: Vec<_> = dist.sample_iter(&mut rng).take(1000 * 1000).collect();
                let local_cache = CACHE.get().unwrap();
                THREAD_BARRIER.wait();
                let mut cnt = 0u64;
                // now, send the request to the cache and start timing.
                for s in seeds {
                    match local_cache.update(s, BlockState::Exclusive) {
                        CacheReturnResult::Miss => cnt += 1,
                        CacheReturnResult::Hit => {}
                        CacheReturnResult::MissWithEviction(_) => {}
                        CacheReturnResult::MissWithDirtyEviction(_) => {}
                        CacheReturnResult::MissWithWrongPermission => {}
                    }
                }
                THREAD_BARRIER.wait();
                println!("Misses: {}", cnt);
            });
        })
        .collect();

    THREAD_BARRIER.wait();
    let before_exp = Local::now();
    THREAD_BARRIER.wait();
    let after_exp = Local::now();

    threads_handlers.into_iter().for_each(|x|{
        x.join().unwrap();
    });

    println!("Finish inteval: {}", after_exp - before_exp);

}
