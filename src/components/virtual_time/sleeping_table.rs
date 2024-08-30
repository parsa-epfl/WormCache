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

use std::cell::UnsafeCell;

#[derive(Debug)]
#[repr(align(64))]
struct IsSleeping {
    is_sleeping: bool,
}

pub struct SleepingTable<const CORE_COUNT: usize> {
    is_sleeping: [UnsafeCell<IsSleeping>; CORE_COUNT],
}

impl<const CORE_COUNT: usize> SleepingTable<CORE_COUNT> {
    pub fn new() -> Self {
        Self {
            is_sleeping: std::array::from_fn(|_| {
                UnsafeCell::new(IsSleeping { is_sleeping: false })
            }),
        }
    }

    pub fn set_sleeping(&self, core_id: usize, is_sleeping: bool) {
        // self.is_sleeping[core_id].is_sleeping = is_sleeping;
        unsafe {
            (*self.is_sleeping[core_id].get()).is_sleeping = is_sleeping;
        }
    }

    pub fn has_slept(&self, core_id: usize) -> bool {
        unsafe { (*self.is_sleeping[core_id].get()).is_sleeping }
    }

    pub fn clean_sleeping(&self) {
        for i in 0..CORE_COUNT {
            self.set_sleeping(i, false);
        }
    }
}
