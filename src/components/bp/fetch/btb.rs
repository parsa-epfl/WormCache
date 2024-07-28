use crate::components::bp::{BranchResolutionResult, BranchType};

use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use super::BranchPredictorResult;

#[derive(Deserialize, Serialize)]
struct BTBEntry {
    tag: u64, 
    target: u64,
    ts: u64, // zero means invalid.
    branch_type: BranchType,
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
                    tag: 0,
                    target: 0,
                    ts: 0,
                    branch_type: BranchType::NonBranch,
                })
            })),
            local_ts: 0,
        }
    }

    // return true if the target is predicted by the BTB.
    pub fn train(
        &mut self,
        pc: u64,
        result: BranchResolutionResult,
        target: u64,
    ) -> BranchPredictorResult {
        self.local_ts += 1;

        let index = (pc % SET as u64) as usize;

        // We need to check if the entry is already in the BTB. If yes, we update the timestamp and return.
        // This should be the common case.
        for entry in self.array[index].iter_mut() {
            if entry.tag == pc {
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
        self.array[index][min_index].tag = pc;
        self.array[index][min_index].target = target;
        self.array[index][min_index].ts = self.local_ts;
        self.array[index][min_index].branch_type = result.branch_type;

        BranchPredictorResult::Mispredict
    }
}

///// Serialization and Deserialization for QFlex.

#[derive(Serialize, Deserialize)]
pub struct BTBEntrySerializeHelper {
    #[serde(rename = "PC")]
    pc: u64,
    target: u64,
    #[serde(rename = "type")]
    type_: u64,
}

use crate::components::FlexusCompatibleSerializer;

impl FlexusCompatibleSerializer for BTBEntry {
    type HelperType = BTBEntrySerializeHelper;

    fn get_serialize_helper(&self) -> Self::HelperType {
        BTBEntrySerializeHelper {
            pc: self.tag,
            target: self.target,
            type_: self.branch_type as u64,
        }
    }
}

impl<const SET: usize, const ASSO: usize> FlexusCompatibleSerializer for BTB<SET, ASSO> {
    type HelperType = Vec<Vec<BTBEntrySerializeHelper>>;

    fn get_serialize_helper(&self) -> Self::HelperType {
        self.array
            .iter()
            .map(|set| {
                // set.iter()
                //     .map(|entry| entry.get_serialize_helper())
                //     .collect()
                let mut res = vec![];

                // filter and only keep the valid bit.
                for entry in set.iter() {
                    if entry.ts != 0 {
                        res.push(entry);
                    }
                }

                // sort by the timestamp. Small ts first.
                res.sort_by_key(|entry| entry.ts);

                res.into_iter().map(|entry| entry.get_serialize_helper()).collect()
            })
            .collect()
    }
}
