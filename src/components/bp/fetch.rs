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

mod bimodal;
pub mod btb;
mod gshare;
mod ras;
pub mod tage;

use crate::debug::statistics::{EventType, Statistics};

use crate::checkpoint::helpers::{FetchUnitHelper, PerCoreFetchUnitHelper};
use crate::parameter::{self, BP_RAS_COUNT};

use super::{BranchResolutionResult, BranchType};

#[derive(PartialEq)]
pub enum BranchPredictorResult {
    Match,
    Mispredict,
    NotActive,
}

#[repr(align(64))]
#[derive(Debug)]
pub struct PerCoreFetchUnit {
    btb: btb::BTB<{ parameter::BTB_SET }, { parameter::BTB_ASSO }>,
    ras: ras::ReturnAddressStack<BP_RAS_COUNT>,
    tage: tage::TAGEPredictor,
}

impl PerCoreFetchUnit {
    pub fn to_checkpoint_helper(&self) -> PerCoreFetchUnitHelper {
        PerCoreFetchUnitHelper {
            btb: self.btb.to_checkpoint_helper(),
            ras: self.ras.to_checkpoint_helper(),
            tage: self.tage.to_checkpoint_helper(),
        }
    }

    pub fn from_checkpoint_helper(helper: PerCoreFetchUnitHelper) -> Self {
        Self {
            btb: btb::BTB::from_checkpoint_helper(helper.btb),
            ras: ras::ReturnAddressStack::from_checkpoint_helper(helper.ras),
            tage: tage::TAGEPredictor::from_checkpoint_helper(helper.tage),
        }
    }
}

impl PerCoreFetchUnit {
    pub fn new() -> PerCoreFetchUnit {
        PerCoreFetchUnit {
            btb: btb::BTB::new(),
            ras: ras::ReturnAddressStack::new(),
            tage: tage::TAGEPredictor::new(),
        }
    }

    pub fn train(&mut self, pc: u64, result: BranchResolutionResult, target: u64, core_id: usize) {
        let is_os = pc >> 63 == 1;
        let btb_result = self.btb.train(pc, result, target);
        let btb_miss = btb_result.0 == BranchPredictorResult::Mispredict;

        let tage_miss = if btb_result.1 == BranchType::Conditional {
            self.tage.train(pc, result, target) == BranchPredictorResult::Mispredict
        } else if result.branch_type != BranchType::NonBranch {
            self.tage.update_history(pc, result.is_taken); // This has to be done for non-conditional branches.
            false // No way to train the TAGE predictor for non-conditional branches.
        } else {
            false
        };

        let ras_miss = self.ras.train(pc, result, target) == BranchPredictorResult::Mispredict;

        if btb_miss {
            Statistics::global_record(core_id as u32, EventType::BTBMiss, is_os);
        }

        if ras_miss {
            Statistics::global_record(core_id as u32, EventType::RASMiss, is_os);
        }

        if tage_miss {
            Statistics::global_record(core_id as u32, EventType::TageMiss, is_os);
        }

        Statistics::global_record(core_id as u32, EventType::BranchCount, is_os);

        // Determine the branch prediction result.
        match result.branch_type {
            BranchType::NonBranch => unreachable!(),
            BranchType::Conditional => {
                if tage_miss || btb_miss {
                    Statistics::global_record(core_id as u32, EventType::BPMiss, is_os);
                }
            }
            BranchType::Return => {
                if ras_miss && btb_miss {
                    Statistics::global_record(core_id as u32, EventType::BPMiss, is_os);
                }
            }
            _ => {
                if btb_miss {
                    Statistics::global_record(core_id as u32, EventType::BPMiss, is_os);
                }
            }
        }
    }
}

impl Default for PerCoreFetchUnit {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FetchUnit<const CORE_COUNT: usize> {
    pub private_units: [PerCoreFetchUnit; CORE_COUNT],
}

impl<const CORE_COUNT: usize> FetchUnit<CORE_COUNT> {
    pub fn new() -> Self {
        FetchUnit {
            private_units: std::array::from_fn(|_| PerCoreFetchUnit::new()),
        }
    }

    pub fn train(&mut self, core_id: usize, pc: u64, result: BranchResolutionResult, target: u64) {
        self.private_units[core_id].train(pc, result, target, core_id);
    }

    pub fn dump_training_trace(&self, folder_name: &str) {
        for i in 0..CORE_COUNT {
            let file_name = format!("{}/{}-bpred-training-history.json", folder_name, i);
            let file = std::fs::File::create(file_name).unwrap();
            serde_json::to_writer(file, &self.private_units[i].tage.training_trace).unwrap();
        }
    }

    pub fn to_checkpoint_helper(&self) -> FetchUnitHelper {
        FetchUnitHelper {
            private_units: self
                .private_units
                .iter()
                .map(|u| u.to_checkpoint_helper())
                .collect(),
        }
    }

    pub fn from_checkpoint_helper(helper: FetchUnitHelper) -> Self {
        let private_units: Vec<_> = helper
            .private_units
            .into_iter()
            .map(PerCoreFetchUnit::from_checkpoint_helper)
            .collect();
        Self {
            private_units: private_units.try_into().expect("FetchUnit size mismatch"),
        }
    }

    pub fn serialize_worker(&self, worker_id: usize, name: &str) {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, USE_RKYV_SERIALIZATION};

        let cores_per_worker = CORE_COUNT / CHECKPOINT_POOL_SIZE;
        let begin = worker_id * cores_per_worker;
        let end = begin + cores_per_worker;

        let helper = FetchUnitHelper {
            private_units: self.private_units[begin..end]
                .iter()
                .map(|u| u.to_checkpoint_helper())
                .collect(),
        };

        if USE_RKYV_SERIALIZATION {
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper).unwrap();
            crate::util::write_compressed(
                &format!("{}/fetch-worker-{}.rkyv.zstd", name, worker_id),
                &bytes,
            );
        } else {
            let bytes = serde_json::to_vec(&helper).unwrap();
            crate::util::write_compressed(
                &format!("{}/fetch-worker-{}.json.zstd", name, worker_id),
                &bytes,
            );
        }
    }

    pub fn deserialize_worker(&mut self, worker_id: usize, name: &str) -> bool {
        use crate::parameter::{CHECKPOINT_POOL_SIZE, USE_RKYV_SERIALIZATION};

        let cores_per_worker = CORE_COUNT / CHECKPOINT_POOL_SIZE;
        let begin = worker_id * cores_per_worker;

        if USE_RKYV_SERIALIZATION {
            let file = std::fs::File::open(format!(
                "{}/fetch-worker-{}.rkyv.zstd",
                name, worker_id
            ));

            if file.is_err() {
                return false;
            }

            let file = file.unwrap();
            let mut decoder = zstd::Decoder::new(file).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

            let helper: FetchUnitHelper =
                rkyv::from_bytes::<FetchUnitHelper, rkyv::rancor::Error>(&bytes).unwrap();

            for (i, unit) in helper.private_units.into_iter().enumerate() {
                self.private_units[begin + i] = PerCoreFetchUnit::from_checkpoint_helper(unit);
            }
        } else {
            let file = std::fs::File::open(format!(
                "{}/fetch-worker-{}.json.zstd",
                name, worker_id
            ));

            if file.is_err() {
                return false;
            }

            let file = file.unwrap();
            let decoder = zstd::Decoder::new(file).unwrap();

            let helper: FetchUnitHelper = serde_json::from_reader(decoder).unwrap();

            for (i, unit) in helper.private_units.into_iter().enumerate() {
                self.private_units[begin + i] = PerCoreFetchUnit::from_checkpoint_helper(unit);
            }
        }
        true
    }
}

impl<const CORE_COUNT: usize> Default for FetchUnit<CORE_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}
