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

use crate::parameter as param;
use param::CORE_COUNT;

#[repr(align(64))]
#[derive(Debug, Clone, Copy)]
pub struct PerCoreICount {
    user_icount: u64,
    kernel_icount: u64,
}

impl PerCoreICount {
    pub fn new() -> PerCoreICount {
        PerCoreICount {
            user_icount: 0,
            kernel_icount: 0,
        }
    }
}

#[derive(Debug)]
pub struct ICountPlugin {
    data: [UnsafeCell<PerCoreICount>; CORE_COUNT],
}

impl ICountPlugin {
    pub fn get_icounts(&self) -> [(u64, u64); CORE_COUNT] {
        // (user_icount, kernel_icount)
        let mut res = [(0, 0); CORE_COUNT];
        for i in 0..CORE_COUNT {
            unsafe {
                res[i] = (
                    (*self.data[i].get()).user_icount,
                    (*self.data[i].get()).kernel_icount,
                );
            }
        }
        res
    }

    pub fn increase_user_icount(&self, core_id: u8, icount: u64) {
        unsafe {
            let core_id = core_id as usize;
            (*self.data[core_id].get()).user_icount += icount;
        }
    }

    pub fn increase_kernel_icount(&self, core_id: u8, icount: u64) {
        unsafe {
            let core_id = core_id as usize;
            (*self.data[core_id].get()).kernel_icount += icount;
        }
    }

    pub fn new() -> ICountPlugin {
        ICountPlugin {
            data: std::array::from_fn(|_| UnsafeCell::new(PerCoreICount::new())),
        }
    }

    pub fn total_user_icount(&self) -> u64 {
        let mut res = 0;

        const CORE_COUNT: usize = if param::MEASURE_HALF_OF_CORES {
            param::CORE_COUNT / 2
        } else {
            param::CORE_COUNT
        };

        for i in 0..CORE_COUNT {
            unsafe {
                res += (*self.data[i].get()).user_icount;
            }
        }
        res
    }

    pub fn total_icount(&self) -> (u64, u64) {
        // (u, k)
        let mut res = (0, 0);

        const CORE_COUNT: usize = if param::MEASURE_HALF_OF_CORES {
            param::CORE_COUNT / 2
        } else {
            param::CORE_COUNT
        };

        for i in 0..CORE_COUNT {
            unsafe {
                res.0 += (*self.data[i].get()).user_icount;
                res.1 += (*self.data[i].get()).kernel_icount;
            }
        }

        res
    }
}
