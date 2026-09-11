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

use crate::checkpoint::helpers::MMUHelper;

use super::{AbstractMMU, MMUFlushMode, MMUTranslationResult};

pub struct NoMMU {}

impl AbstractMMU for NoMMU {
    fn new() -> Self {
        Self {}
    }
    fn translate_and_refill(
        &mut self,
        _core_id: u32,
        va: u64,
        _: u64,
        _: bool,
    ) -> MMUTranslationResult {
        MMUTranslationResult::Hit(va, 0)
    }

    fn lookup(&mut self, _: u64, _: u64, _: bool) -> Option<u64> {
        None
    }

    fn flush(&mut self, _mode: MMUFlushMode) {}

    fn serialize(&self) -> MMUHelper {
        MMUHelper::NoMMU(crate::checkpoint::helpers::NoMMUHelper {})
    }

    fn deserialize(&mut self, value: MMUHelper) {
        match value {
            MMUHelper::NoMMU(_) => {}
            _ => panic!("Expected NoMMU helper, got different variant"),
        }
    }
}
