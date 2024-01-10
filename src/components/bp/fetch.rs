mod bimodal;
mod btb;
mod gshare;
mod ras;
mod tage;

use crate::{parameter::BP_GSHARE_SET, BP_RAS_COUNT};

use super::BranchResolveFlag;

struct FetchUnit {
    btb: btb::BTB<BP_GSHARE_SET>,
    ras: ras::ReturnAddressStacle<BP_RAS_COUNT>,
    tage: tage::TAGEPredictor,
}

impl FetchUnit {
    pub fn new() -> FetchUnit {
        FetchUnit {
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
