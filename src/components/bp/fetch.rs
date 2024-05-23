mod bimodal;
mod btb;
mod gshare;
mod ras;
mod tage;

use crate::components::debug::statistics::{EventType, Statistics};
use serde::Serialize;

use crate::parameter::{self, BP_RAS_COUNT};

use super::BranchResolveFlag;

#[derive(PartialEq)]
pub enum BranchPredictorResult {
    Match,
    Mispredict,
    NotActive,
}

#[repr(align(64))]
#[derive(Serialize)]
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

    pub fn train(&mut self, pc: u64, result: BranchResolveFlag, target: u64, core_id: usize) {
        if self.btb.train(pc, result, target) == BranchPredictorResult::Mispredict {
            Statistics::global_record(core_id as u32, EventType::BTBMiss);
        }

        if self.ras.train(pc, result, target) == BranchPredictorResult::Mispredict {
            Statistics::global_record(core_id as u32, EventType::RASMiss);
        }

        if self.tage.train(pc, result, target) == BranchPredictorResult::Mispredict {
            Statistics::global_record(core_id as u32, EventType::TageMiss);
        }
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

    pub fn train(&mut self, core_id: usize, pc: u64, result: BranchResolveFlag, target: u64) {
        self.private_units[core_id].train(pc, result, target, core_id);
    }
}
