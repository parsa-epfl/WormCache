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

//! Basic Block Vector (BBV) Recorder
//!
//! This module provides per-core recording of basic block execution frequencies.
//! The recorder tracks how many times each basic block (identified by its PC) is executed.

use rustc_hash::FxHashMap;

/// Per-core basic block vector recorder.
/// Records the execution count of each basic block identified by its PC.
#[repr(align(64))]
#[derive(Debug, Default)]
pub struct PerCoreBBVRecorder {
    /// Maps basic block PC to execution count
    bb_counts: FxHashMap<u64, u64>,
}

impl PerCoreBBVRecorder {
    pub fn new() -> Self {
        Self {
            bb_counts: FxHashMap::default(),
        }
    }

    /// Record an execution of the basic block at the given PC.
    #[inline]
    pub fn record(&mut self, pc: u64) {
        *self.bb_counts.entry(pc).or_insert(0) += 1;
    }

    /// Clear all recorded data.
    pub fn clear(&mut self) {
        self.bb_counts.clear();
    }

    /// Get the recorded basic block counts.
    pub fn counts(&self) -> &FxHashMap<u64, u64> {
        &self.bb_counts
    }
}

/// Multi-core BBV recorder with per-core instances to avoid locking.
pub struct BBVRecorder<const CORE_COUNT: usize> {
    pub private_recorders: [PerCoreBBVRecorder; CORE_COUNT],
}

impl<const CORE_COUNT: usize> BBVRecorder<CORE_COUNT> {
    pub fn new() -> Self {
        Self {
            private_recorders: std::array::from_fn(|_| PerCoreBBVRecorder::new()),
        }
    }

    /// Record an execution of the basic block at the given PC for the specified core.
    #[inline]
    pub fn record(&mut self, core_id: usize, pc: u64) {
        self.private_recorders[core_id].record(pc);
    }

    /// Save per-core BBV data to JSON files and clear the recorded data.
    /// Each core's data is saved to a separate file: `{folder}/{core_id:03}-bbv.json`
    pub fn save_and_clear(&mut self, folder: &str) {
        for (core_id, recorder) in self.private_recorders.iter_mut().enumerate() {
            let file_name = format!("{}/{:03}-bbv.json", folder, core_id);
            let file = std::fs::File::create(&file_name).unwrap();
            serde_json::to_writer(file, recorder.counts()).unwrap();
            recorder.clear();
        }
    }
}

impl<const CORE_COUNT: usize> Default for BBVRecorder<CORE_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}
