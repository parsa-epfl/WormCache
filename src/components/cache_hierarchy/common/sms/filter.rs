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

use rustc_hash::FxHashMap;
use spin::mutex::SpinMutex;

use super::acc::AccTableEntry;
use super::util;
use crate::components::cache_hierarchy::CacheBlockRequest;

#[derive(Debug, Clone, Copy)]
struct FilterTableDataEntry {
    pub pc: u64,
    pub offset: u64,
    pub is_read: bool,
    pub ts: u64,
}

#[derive(Debug)]
pub struct FilterTable<
    const N_FILTER: usize, // Number of entries in the filter table
    const N_BLK: usize,    // Number of blocks per spatial region
> {
    entries: SpinMutex<FxHashMap<u64, FilterTableDataEntry>>,
}

impl<const N_FILTER: usize, const N_BLK: usize> FilterTable<N_FILTER, N_BLK> {
    pub fn new() -> Self {
        Self {
            entries: SpinMutex::new(FxHashMap::with_capacity_and_hasher(
                N_FILTER,
                Default::default(),
            )),
        }
    }

    #[inline]
    fn get_base_pc_offset(&self, request: &CacheBlockRequest) -> (u64, u64, u64) {
        util::get_base_pc_offset::<N_BLK>(request)
    }

    fn get_lru_key(entries: &FxHashMap<u64, FilterTableDataEntry>) -> Option<u64> {
        entries
            .iter()
            .min_by_key(|(_, data)| data.ts)
            .map(|(key, _)| *key)
    }

    pub fn poke_and_update(
        &self,
        request: &CacheBlockRequest,
        ts: u64,
    ) -> Option<AccTableEntry<N_BLK>> {
        let (base, pc, offset) = self.get_base_pc_offset(request);
        let is_read = !request.is_store();

        let mut entries = self.entries.lock();

        if let Some(existing) = entries.get_mut(&base) {
            // Entry found, check further
            if offset == existing.offset {
                existing.pc = pc;
                existing.is_read = is_read;
                existing.ts = ts;
                return None;
            } else {
                // Must be promoted to acc entry
                let mut acc_entry = AccTableEntry::<N_BLK>::new();
                acc_entry.tag = base;
                acc_entry.pc = existing.pc;
                acc_entry.offset = existing.offset;
                acc_entry.access_pattern[existing.offset as usize] = true;
                acc_entry.access_pattern[offset as usize] = true;
                acc_entry.read_pattern[existing.offset as usize] = existing.is_read;
                acc_entry.read_pattern[offset as usize] = is_read;
                acc_entry.ts = ts;
                acc_entry.valid = true;
                entries.remove(&base);
                return Some(acc_entry);
            }
        }

        // New entry must be allocated
        let new_data = FilterTableDataEntry {
            pc,
            offset,
            is_read,
            ts,
        };

        // If we have space, insert directly
        if entries.len() < N_FILTER {
            entries.insert(base, new_data);
            return None;
        }

        // Need to evict LRU entry
        if let Some(lru_key) = Self::get_lru_key(&entries) {
            entries.remove(&lru_key);
            entries.insert(base, new_data);
        }

        None
    }

    pub fn evict(&self, request: &CacheBlockRequest) {
        let (base, _, _) = self.get_base_pc_offset(request);
        let mut entries = self.entries.lock();
        entries.remove(&base);
    }
}
