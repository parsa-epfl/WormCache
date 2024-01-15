mod bimodal;
mod btb;
mod gshare;
mod ras;
mod tage;

use crate::{parameter::BP_GSHARE_SET, BP_RAS_COUNT, parameter::CORE_COUNT};
use std::cell::UnsafeCell;

use super::BranchResolveFlag;

#[repr(align(64))]
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

    pub fn serialize(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(self.btb.serialize());
        v.extend(self.ras.serialize());
        v.extend(self.tage.serialize());
        v
    }
}

pub struct FetchUnit {
    pub cores: [PerCoreFetchUnit; CORE_COUNT],
}

impl FetchUnit {
    pub fn new() -> FetchUnit {
        FetchUnit {
            cores: [PerCoreFetchUnit::new(); CORE_COUNT],
        }
    }

    pub fn train(&mut self, core_id: usize, pc: u64, result: BranchResolveFlag, target: u64) {
        self.cores[core_id].train(pc, result, target);
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut v = Vec::new();
        for core in self.cores.iter() {
            v.extend(core.serialize());
        }
        v
    }
}