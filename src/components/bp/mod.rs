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

pub mod fetch;

use std::io::Write;

use super::Plugin;
use crate::{parameter, qemu_api};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use zstd::{Decoder, Encoder};

// Use Arena to allocate the BranchMetaData.
// https://crates.io/crates/bumpalo

#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Archive, RkyvDeserialize, RkyvSerialize,
)]
pub enum BranchType {
    NonBranch = 0,
    Conditional = 1,
    Unconditional = 2,
    DirectCall = 3,
    IndirectBranch = 4,
    IndirectCall = 5,
    Return = 6,
}

impl BranchType {
    pub fn is_call(&self) -> bool {
        matches!(self, BranchType::DirectCall | BranchType::IndirectCall)
    }

    pub fn is_return(&self) -> bool {
        matches!(self, BranchType::Return)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BranchResolutionResult {
    pub branch_type: BranchType,
    pub is_taken: bool,
}

impl BranchResolutionResult {
    fn from_u32(value: u32) -> BranchResolutionResult {
        let is_taken = value & 1 == 1;
        let result_value = value >> 1;

        BranchResolutionResult {
            is_taken,
            branch_type: match result_value {
                0 => unreachable!(),
                1 => BranchType::Conditional,
                2 => {
                    assert!(is_taken);
                    BranchType::Unconditional
                }
                3 => {
                    assert!(is_taken);
                    BranchType::DirectCall
                }
                4 => {
                    assert!(is_taken);
                    BranchType::IndirectBranch
                }
                5 => {
                    assert!(is_taken);
                    BranchType::IndirectCall
                }
                6 => {
                    assert!(is_taken);
                    BranchType::Return
                }
                _ => unreachable!(),
            },
        }
    }
}

const ALLOCATED_CORE: usize = if parameter::MEASURE_HALF_OF_CORES {
    parameter::CORE_COUNT / 2
} else {
    parameter::CORE_COUNT
};

static mut FETCH_UNIT: *mut fetch::FetchUnit<{ ALLOCATED_CORE }> = std::ptr::null_mut();

/**
 * Whether to record BBV inside the fetch unit.
 *
 * By default, this should be turned off for serious performance measurement.
 */
const RECORDING_BBV: bool = false;

mod bbv;

static mut BBV_RECORDER: *mut bbv::BBVRecorder<{ ALLOCATED_CORE }> = std::ptr::null_mut();

unsafe extern "C" fn branch_resolved_cb(vcpu_index: u32, pc: u64, target: u64, flags: u32) {
    unsafe {
        if parameter::BYPASSING_OS_SIMULATION && (pc >> 48) != 0 {
            // Fine, this is an OS simulation code. We can bypass it.
            return;
        }

        if parameter::MEASURE_HALF_OF_CORES && vcpu_index >= parameter::CORE_COUNT as u32 / 2 {
            return;
        }

        let result = BranchResolutionResult::from_u32(flags);
        (*FETCH_UNIT).train(vcpu_index as usize, pc, result, target);

        if RECORDING_BBV {
            (*BBV_RECORDER).record(vcpu_index as usize, target);
        }
    }
}

pub struct BranchPredictorPlugin {}

impl Plugin for BranchPredictorPlugin {
    fn init(_plugin_id: u64, options: &FxHashMap<String, String>) {
        println!("BranchPredictorPlugin initialized.");

        // get the mode name.
        let mode = String::new();
        let mode = options.get("mode").unwrap_or(&mode);

        assert_ne!(
            mode, "vtime",
            "Pure vtime is enabled. BP should be disabled."
        );

        assert!(unsafe {
            qemu_api::qemu_plugin_register_vcpu_branch_resolved_cb(Some(branch_resolved_cb))
        });

        unsafe {
            FETCH_UNIT = Box::into_raw(Box::new(fetch::FetchUnit::new()));

            if RECORDING_BBV {
                BBV_RECORDER = Box::into_raw(Box::new(bbv::BBVRecorder::new()));
            }
        }
    }

    unsafe fn on_translation(_: *mut crate::qemu_api::qemu_plugin_tb) {
        // The callback is already inserted into the TB during init.
    }

    fn serialize(name: &str) {
        use crate::parameter::USE_RKYV_SERIALIZATION;

        let helper = unsafe { (*FETCH_UNIT).to_checkpoint_helper() };

        if USE_RKYV_SERIALIZATION {
            let file = std::fs::File::create(format!("{}/fetch.rkyv.zstd", name)).unwrap();
            let mut encoder = Encoder::new(file, 0).unwrap();

            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
            encoder.write_all(&bytes).unwrap();
            encoder.finish().unwrap();
        } else {
            let file = std::fs::File::create(format!("{}/fetch.json.zstd", name)).unwrap();
            let mut encoder = Encoder::new(file, 0).unwrap();

            serde_json::to_writer(&mut encoder, &helper).unwrap();
            encoder.finish().unwrap();
        }

        if RECORDING_BBV {
            unsafe {
                (*BBV_RECORDER).save_and_clear(name);
            }
        }
    }

    fn deserialize(name: &str) {
        use crate::checkpoint::helpers::FetchUnitHelper;
        use crate::parameter::USE_RKYV_SERIALIZATION;

        if USE_RKYV_SERIALIZATION {
            let file = std::fs::File::open(format!("{}/fetch.rkyv.zstd", name));

            if file.is_err() {
                println!(
                    "Cannot load the fetch unit state (rkyv). Error: {:?}",
                    file.err()
                );
                return;
            }

            let file = file.unwrap();
            let mut decoder = Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            let helper: FetchUnitHelper =
                rkyv::from_bytes::<FetchUnitHelper, rkyv::rancor::Error>(&bytes).unwrap();
            unsafe {
                *FETCH_UNIT = fetch::FetchUnit::from_checkpoint_helper(helper);
            }
        } else {
            let file = std::fs::File::open(format!("{}/fetch.json.zstd", name));

            if file.is_err() {
                println!("Cannot load the fetch unit state. Error: {:?}", file.err());
                return;
            }

            let file = file.unwrap();
            let decoder = Decoder::new(file).unwrap();

            let helper: FetchUnitHelper = serde_json::from_reader(decoder).unwrap();
            unsafe {
                *FETCH_UNIT = fetch::FetchUnit::from_checkpoint_helper(helper);
            }
        }
    }
}
