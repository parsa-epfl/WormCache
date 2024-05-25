use std::collections::LinkedList;

use crate::components::bp::BranchResolveFlag;
use serde::{Deserialize, Serialize};

use super::BranchPredictorResult;

#[derive(Deserialize, Serialize)]
pub struct ReturnAddressStacle<const S: usize> {
    stack: LinkedList<u64>,
}

impl<const S: usize> ReturnAddressStacle<S> {
    pub fn new() -> ReturnAddressStacle<S> {
        ReturnAddressStacle {
            stack: LinkedList::new(),
        }
    }

    pub fn train(
        &mut self,
        pc: u64,
        result: BranchResolveFlag,
        target: u64,
    ) -> BranchPredictorResult {
        if result != BranchResolveFlag::Call && result != BranchResolveFlag::Return {
            return BranchPredictorResult::NotActive;
        }
        if result == BranchResolveFlag::Call {
            self.push_and_evict(pc + 4);
            return BranchPredictorResult::NotActive;
        } else if result == BranchResolveFlag::Return {
            let miss = self.stack.back() != Some(&target);
            self.stack.pop_back();
            return if miss {
                BranchPredictorResult::Mispredict
            } else {
                BranchPredictorResult::Match
            };
        }

        BranchPredictorResult::NotActive
    }

    fn push_and_evict(&mut self, pc: u64) {
        if self.stack.len() == S {
            self.stack.pop_front();
        }
        self.stack.push_back(pc);
    }
}
