#![allow(dead_code)]

use crate::components::bp::{BranchResolutionResult, BranchType};

use serde::{Deserialize, Serialize};

use super::BranchPredictorResult;

// The maximum size of the global history register is 64 bits.
#[derive(Deserialize, Serialize)]
struct GShare<const S: usize> {
    pub history: u64,
    pub table: Vec<u8>,
}

impl<const S: usize> GShare<S> {
    pub fn new() -> GShare<S> {
        GShare {
            history: 0,
            table: Vec::from_iter(std::iter::repeat(0).take(S)),
        }
    }

    pub fn train(
        &mut self,
        pc: u64,
        result: BranchResolutionResult,
        _target: u64,
    ) -> BranchPredictorResult {
        if result.branch_type != BranchType::Conditional {
            self.history = (self.history << 1) | 1;
            return BranchPredictorResult::NotActive;
        }
        let taken = result.is_taken;
        let index = ((pc ^ self.history) % S as u64) as usize;
        let prediction = self.get_prediction(index);
        if taken {
            self.saturaing_add(index);
        } else {
            self.saturating_sub(index)
        }
        self.history = (self.history << 1) | (if taken { 1 } else { 0 });

        if prediction == taken {
            BranchPredictorResult::Match
        } else {
            BranchPredictorResult::Mispredict
        }
    }

    fn saturaing_add(&mut self, index: usize) {
        let v = self.table[index];
        if v < 3 {
            self.table[index] = v + 1;
        }
    }

    fn saturating_sub(&mut self, index: usize) {
        let v = self.table[index];
        if v > 0 {
            self.table[index] = v - 1;
        }
    }

    fn get_prediction(&self, index: usize) -> bool {
        self.table[index] >= 2
    }
}
