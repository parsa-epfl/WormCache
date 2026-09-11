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
use std::io::Write;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::parameter::{ENABLE_STATISTICS, RECORD_NOC_TRAFFIC, SIMULATED_CORE_COUNT};

#[derive(Clone, Copy)]
pub enum AccessReason {
    Directory = 0,
    LLC = 1,
    DRAM = 2,
    PrivateCacheDemandBlock = 3,
    PrivateCacheInvalidateDueToGetX = 4,
    PrivateCacheDowngrade = 5,
    PrivateCacheInvalidateDueToDirectoryEviction = 6,
}

impl AccessReason {
    pub const COUNT: usize = 7;
}

#[repr(align(64))]
struct PerAccessorCounts {
    counts: [[u64; AccessReason::COUNT]; SIMULATED_CORE_COUNT],
}

pub struct NocTraffic {
    per_accessor: [UnsafeCell<PerAccessorCounts>; SIMULATED_CORE_COUNT],
}

unsafe impl Sync for NocTraffic {}

impl NocTraffic {
    #[inline]
    fn record(&self, accessor_id: u32, destination_id: u32, reason: AccessReason) {
        unsafe {
            (*self.per_accessor[accessor_id as usize].get()).counts[destination_id as usize]
                [reason as usize] += 1;
        }
    }

    pub fn get_header() -> String {
        "destination_id,accessor_id,directory,llc,dram,private_cache_demand_block,\
         private_cache_invalidate_due_to_getx,private_cache_downgrade,\
         private_cache_invalidate_due_to_directory_eviction"
            .to_string()
    }

    fn save_to_csv_inner(&self, file_name: &str) {
        let mut file = std::fs::File::create(file_name).unwrap();
        file.write_fmt(format_args!("{}\n", Self::get_header()))
            .unwrap();
        for destination_id in 0..SIMULATED_CORE_COUNT {
            for accessor_id in 0..SIMULATED_CORE_COUNT {
                unsafe {
                    let counts = &(*self.per_accessor[accessor_id].get()).counts[destination_id];
                    file.write_fmt(format_args!(
                        "{},{},{},{},{},{},{},{},{}\n",
                        destination_id,
                        accessor_id,
                        counts[AccessReason::Directory as usize],
                        counts[AccessReason::LLC as usize],
                        counts[AccessReason::DRAM as usize],
                        counts[AccessReason::PrivateCacheDemandBlock as usize],
                        counts[AccessReason::PrivateCacheInvalidateDueToGetX as usize],
                        counts[AccessReason::PrivateCacheDowngrade as usize],
                        counts[AccessReason::PrivateCacheInvalidateDueToDirectoryEviction as usize],
                    ))
                    .unwrap();
                }
            }
        }
        file.flush().unwrap();
    }

    #[inline]
    pub fn global_record(accessor_id: u32, destination_id: u32, reason: AccessReason) {
        if ENABLE_STATISTICS && RECORD_NOC_TRAFFIC {
            global().record(accessor_id, destination_id, reason);
        }
    }

    pub fn save_to_csv(file_name: &str) {
        global().save_to_csv_inner(file_name);
    }
}

static GLOBAL_NOC_TRAFFIC: AtomicPtr<NocTraffic> = AtomicPtr::new(std::ptr::null_mut());

#[inline]
fn global() -> &'static NocTraffic {
    unsafe { &*GLOBAL_NOC_TRAFFIC.load(Ordering::Relaxed) }
}

/// Allocates the global NocTraffic directly on the heap.
/// Must be called once before any recording begins.
pub fn init() {
    let ptr = unsafe { Box::into_raw(Box::<NocTraffic>::new_zeroed().assume_init()) };
    GLOBAL_NOC_TRAFFIC.store(ptr, Ordering::Relaxed);
}
