mod bimodal;
mod btb;
mod gshare;
mod ras;
mod tage;

use serde::Serialize;

use crate::{parameter::BP_GSHARE_SET, parameter::BP_RAS_COUNT, parameter::CORE_COUNT};

use super::BranchResolveFlag;

#[repr(align(64))]
#[derive(Serialize)]
pub struct PerCoreFetchUnit {
    btb: btb::BTB<BP_GSHARE_SET>,
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

    pub fn train(&mut self, pc: u64, result: BranchResolveFlag, target: u64) {
        self.btb.train(pc, result, target);
        self.ras.train(pc, result, target);
        self.tage.train(pc, result, target);
    }
}

pub struct FetchUnit {
    pub private_units: [PerCoreFetchUnit; CORE_COUNT],
}

impl FetchUnit {
    pub fn new() -> FetchUnit {
        FetchUnit {
            private_units: std::array::from_fn(|_| PerCoreFetchUnit::new()),
        }
    }

    pub fn train(&mut self, core_id: usize, pc: u64, result: BranchResolveFlag, target: u64) {
        self.private_units[core_id].train(pc, result, target);
    }
}
