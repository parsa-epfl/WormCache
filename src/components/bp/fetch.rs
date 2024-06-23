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
        let is_os = pc >> 63 == 1;
        if self.btb.train(pc, result, target) == BranchPredictorResult::Mispredict {
            Statistics::global_record(core_id as u32, EventType::BTBMiss, is_os);
        }

        if self.ras.train(pc, result, target) == BranchPredictorResult::Mispredict {
            Statistics::global_record(core_id as u32, EventType::RASMiss, is_os);
        }

        if self.tage.train(pc, result, target) == BranchPredictorResult::Mispredict {
            Statistics::global_record(core_id as u32, EventType::TageMiss, is_os);
        }

        Statistics::global_record(core_id as u32, EventType::BranchCount, is_os);
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

    pub fn train(&mut self, core_id: usize, pc: u64, result: BranchResolveFlag, target: u64) {
        self.private_units[core_id].train(pc, result, target, core_id);
    }
}

impl<const CORE_COUNT: usize> Default for FetchUnit<CORE_COUNT> {
    fn default() -> Self {
        Self::new()
    }
}
