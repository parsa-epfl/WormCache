mod bimodal;
mod btb;
mod gshare;
mod ras;
mod tage;

use crate::components::debug::statistics::{EventType, Statistics};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_with::serde_as;

use crate::parameter::{self, BP_RAS_COUNT};

use super::{BranchResolutionResult, BranchType};

use crate::components::FlexusCompatibleSerializer;

#[derive(PartialEq)]
pub enum BranchPredictorResult {
    Match,
    Mispredict,
    NotActive,
}

#[repr(align(64))]
#[derive(Serialize, Deserialize)]
pub struct PerCoreFetchUnit {
    btb: btb::BTB<{ parameter::BTB_SET }, { parameter::BTB_ASSO }>,
    ras: ras::ReturnAddressStacle<BP_RAS_COUNT>,
    tage: tage::TAGEPredictor,
}

impl PerCoreFetchUnit {
    pub fn new() -> PerCoreFetchUnit {
        PerCoreFetchUnit {
            btb: btb::BTB::new(),
            ras: ras::ReturnAddressStacle::new(),
            tage: tage::TAGEPredictor::new(),
        }
    }

    pub fn train(&mut self, pc: u64, result: BranchResolutionResult, target: u64, core_id: usize) {
        let is_os = pc >> 63 == 1;
        let btb_miss = self.btb.train(pc, result, target) == BranchPredictorResult::Mispredict;
        let tage_miss = self.tage.train(pc, result, target) == BranchPredictorResult::Mispredict;
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
        match result.branch_type.clone() {
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

    pub fn get_flexus_checkpoint(&self) -> serde_json::Value {
        let serialized_btb = self.btb.get_serialize_helper();

        let serialized_tage = self.tage.get_serialize_helper();

        json!({
            "btb": serialized_btb,
            "tage": serialized_tage,
        })
    }
}

impl Default for PerCoreFetchUnit {
    fn default() -> Self {
        Self::new()
    }
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct FetchUnit<const CORE_COUNT: usize> {
    #[serde_as(as = "[_; CORE_COUNT]")]
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
}

impl<const CORE_COUNT: usize> Default for FetchUnit<CORE_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}
