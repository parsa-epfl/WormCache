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

use crate::parameter::CORE_COUNT;

#[repr(align(64))]
#[derive(Debug, Clone, Copy)]
pub struct PerCoreICount {
    icount: u64,
    last_icount: u64,
}

impl PerCoreICount {
    pub fn new() -> PerCoreICount {
        PerCoreICount {
            icount: 0,
            last_icount: 0,
        }
    }

    pub fn reset(&mut self) {
        self.icount = 0;
    }
}

impl Default for PerCoreICount {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct ICountPlugin {
    data: [UnsafeCell<PerCoreICount>; CORE_COUNT],
}

impl ICountPlugin {
    pub fn new() -> ICountPlugin {
        ICountPlugin {
            data: std::array::from_fn(|_| UnsafeCell::new(PerCoreICount::new())),
        }
    }

    pub fn get_icount(&self, core_id: u8) -> u64 {
        unsafe { (*self.data[core_id as usize as usize].get()).icount }
    }

    pub fn increase_icount(&self, core_id: u8, icount: u64) {
        unsafe {
            let core_id = core_id as usize;
            (*self.data[core_id].get()).icount += (*self.data[core_id].get()).last_icount;
            (*self.data[core_id].get()).last_icount = icount;

            // the reason why we do so is because this function is called before instructions are actually executed.
            // therefore, when getting the icount, the instruction should not see the icount of the current translation block.
        }
    }
}

impl Default for ICountPlugin {
    fn default() -> Self {
        Self::new()
    }
}
