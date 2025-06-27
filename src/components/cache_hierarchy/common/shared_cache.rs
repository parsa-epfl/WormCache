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

use serde::Serialize;

// There are two possible operations for an exclusive shared cache
// 1. Empty to the cache, which means a write lock is required.
// 2. Read from the cache, depending on the result:
//    - Read is a hit: Read lock + write lock
//    - Read is a miss: Read lock
// 3. It will be probably OK to use Mutex.

#[derive(Debug, PartialEq, Eq)]
pub enum SharedCacheLookupResult {
    Hit(bool, Option<u64>), // (is_dirty, was_invalidated, invalidated_block_id), can be invalidated on a write
    Miss(Option<u64>),  // was anything evicted
    ColdMiss,
    Unknown(u32, bool), // timestamp difference, is_dirty
}

#[derive(Debug, PartialEq, Eq)]
pub enum SharedCacheLookupAndInsertResult {
    Hit(bool, Option<u64>), // (is_dirty, was_invalidated, invalidated_block_id)
    Miss,
    InsertedAndCold(bool), // (just_warmed)
    Inserted(Option<u64>),    // (was_evicted, evicted_block_id)
    Unknown(u32, bool), // timestamp difference, is_dirty
}

pub trait SharedCache {
    fn new() -> Self;

    // Check whether the cache line is in the cache. Do not update the cache.
    fn peek(&self, request: &CacheBlockRequest) -> bool; // (is_hit)

    fn invalidate(&self, core_id: u32, block_id: u64, ts: u64) -> SharedCacheLookupResult;

    // Conduct a normal lookup operation to the shared cache, including:
    // 1. Peek
    // 2. For read, throw dirty information
    // 3. For write, invalid the cache line.
    fn lookup(&self, request: &CacheBlockRequest, ts: u64) -> SharedCacheLookupResult;

    // Return evicted_block_id
    fn insert(
        &self,
        core_id: u32,
        block_id: u64,
        ts: u64,
        is_modified: bool,
        increase_touched_count: bool,
    ) -> Option<u64>;

    // A combine with lookup and insert. If the cache line is not in the cache and it is a read, insert it.
    fn lookup_and_insert_on_miss(
        &self,
        request: &CacheBlockRequest,
        ts: u64,
        increase_touched_count: bool,
    ) -> SharedCacheLookupResult;

    fn warmed_sets_count(&self) -> usize;

    fn warmed_slots_count(&self) -> usize;

    fn information() -> String;

    fn dump_access_frequency(&self, file_name: &str);

    fn serialize(&self, name: &str, numa_node_id: usize);
    fn deserialize(&mut self, name: &str, numa_node_id: usize); // this is in-place deserialization.
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Serialize)]
pub struct SerializedSharedCacheBlock {
    pub tag: u64,
    pub dirty: bool,
    pub writable: bool,
}

pub mod statistics;

mod set_and_line;

pub use set_and_line::SharedCacheBlock;
pub use set_and_line::SharedCacheSet;

mod replicated;
mod single;

pub use replicated::ReplicatedSharedCache;
pub use single::ParallelSingleSharedCache;
pub use single::SerialSingleSharedCache;
pub use single::SingleSharedCache;
pub use single::SingleSharedCacheSerdeHelper;

use crate::components::cache_hierarchy::CacheBlockRequest;

#[cfg(test)]
mod warm_counter_test;
