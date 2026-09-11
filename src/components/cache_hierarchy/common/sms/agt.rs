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

use super::acc::{AccTable, AccTableEntry};
use super::filter::FilterTable;
use crate::components::cache_hierarchy::CacheBlockRequest;

#[derive(Debug)]
struct AGTPerCore<const N_ACC: usize, const N_FILTER: usize, const N_BLK: usize> {
    acc_table: AccTable<N_ACC, N_BLK>,
    filter_table: FilterTable<N_FILTER, N_BLK>,
}

impl<const N_ACC: usize, const N_FILTER: usize, const N_BLK: usize>
    AGTPerCore<N_ACC, N_FILTER, N_BLK>
{
    fn new() -> Self {
        Self {
            acc_table: AccTable::new(),
            filter_table: FilterTable::new(),
        }
    }

    fn record(&self, request: &CacheBlockRequest, ts: u64) -> Option<AccTableEntry<N_BLK>> {
        if self.acc_table.poke_and_update(&request, ts) {
            None
        } else {
            match self.filter_table.poke_and_update(&request, ts) {
                Some(entry) => self.acc_table.insert(&entry),
                None => None,
            }
        }
    }

    fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        self.filter_table.evict(request);
        self.acc_table.evict(request)
    }
}

pub struct AGT<
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const N_BLK: usize,
> {
    tables: Box<[AGTPerCore<N_ACC, N_FILTER, N_BLK>; CORE_COUNT]>,
}

impl<const CORE_COUNT: usize, const N_ACC: usize, const N_FILTER: usize, const N_BLK: usize>
    AGT<CORE_COUNT, N_ACC, N_FILTER, N_BLK>
{
    pub fn new() -> Self {
        Self {
            tables: crate::util::init_heap_array(|_| AGTPerCore::<N_ACC, N_FILTER, N_BLK>::new()),
        }
    }

    pub fn record(&self, request: &CacheBlockRequest, ts: u64) -> Option<AccTableEntry<N_BLK>> {
        let core_id = request.core_id as usize;
        self.tables[core_id].record(request, ts)
    }

    pub fn evict(&self, request: &CacheBlockRequest) -> Option<AccTableEntry<N_BLK>> {
        let core_id = request.core_id as usize;
        self.tables[core_id].evict(request)
    }
}

pub type ParallelAGT<
    const CORE_COUNT: usize,
    const N_ACC: usize,
    const N_FILTER: usize,
    const N_BLK: usize,
> = AGT<CORE_COUNT, N_ACC, N_FILTER, N_BLK>;
