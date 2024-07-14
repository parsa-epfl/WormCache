use crate::components::bp::BranchResolveFlag;

use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use super::BranchPredictorResult;

#[derive(Deserialize, Serialize)]
struct BTBEntry {
    tag_and_valid: u64, // the upper 63 bits are the tag, and the lowest bit is the valid bit
    target: u64,
    ts: u64,
}

#[serde_as]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BTB<const SET: usize, const ASSO: usize> {
    #[serde_as(as = "Vec<[_; ASSO]>")]
    array: Vec<[BTBEntry; ASSO]>,
    local_ts: u64,
}

impl<const SET: usize, const ASSO: usize> BTB<SET, ASSO> {
    pub fn new() -> Self {
        BTB {
            array: Vec::from_iter((0..SET).map(|_| {
                std::array::from_fn(|_| BTBEntry {
                    tag_and_valid: 0,
                    target: 0,
                    ts: 0,
                })
            })),
            local_ts: 0,
        }
    }

    // return true if the target is predicted by the BTB.
    pub fn train(
        &mut self,
        pc: u64,
        result: BranchResolveFlag,
        target: u64,
    ) -> BranchPredictorResult {
        if result == BranchResolveFlag::NotTaken {
            return BranchPredictorResult::NotActive;
        }
        self.local_ts += 1;

        let index = (pc % SET as u64) as usize;

        // We need to check if the entry is already in the BTB. If yes, we update the timestamp and return.
        // This should be the common case.
        for entry in self.array[index].iter_mut() {
            if entry.tag_and_valid == pc {
                entry.ts = self.local_ts;

                let miss = entry.target != target;

                entry.target = target; // also update the target.
                return if miss {
                    BranchPredictorResult::Mispredict
                } else {
                    BranchPredictorResult::Match
                };
            }
        }

        // Find the entry with the minimum timestamp. Ts is zero means it is not valid.
        let mut min_index = 0;
        let mut min_ts = u64::MAX;

        for (i, entry) in self.array[index].iter().enumerate() {
            if entry.ts < min_ts {
                min_ts = entry.ts;
                min_index = i;
            }
        }

        // always replace the entry with the minimum timestamp
        self.array[index][min_index].tag_and_valid = pc;
        self.array[index][min_index].target = target;
        self.array[index][min_index].ts = self.local_ts;

        BranchPredictorResult::Mispredict
    }
}
