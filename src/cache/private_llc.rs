/**
 * This module defines the private LLC used for per-core LLC warmup.
 * It contains two information for each block: the timestamp, and the dirty bits
 */
use core::num::NonZeroUsize;
use lru::LruCache;

pub struct PrivateLLC {
    sets: Vec<LruCache<usize, bool>>,
    associativity: usize,
    warmed_count: usize,
}
