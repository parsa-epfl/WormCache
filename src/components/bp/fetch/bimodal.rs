#![allow(dead_code)]

use crate::components::bp::BranchResolveFlag;

use serde::{Deserialize, Serialize};

use super::BranchPredictorResult;

#[derive(Deserialize, Serialize)]
struct BimodalPredictor<const S: usize> {
    array: Vec<u8>,
    // 0, 1 -> NT
    // 2, 3 -> T
}

impl<const S: usize> BimodalPredictor<S> {
    pub fn new() -> BimodalPredictor<S> {
        BimodalPredictor {
            array: Vec::from_iter(std::iter::repeat(0).take(S)),
        }
    }

    fn saturaing_add(&mut self, index: usize) {
        let v = self.array[index];
        if v < 3 {
            self.array[index] = v + 1;
        }
    }

    fn saturating_sub(&mut self, index: usize) {
        let v = self.array[index];
        if v > 0 {
            self.array[index] = v - 1;
        }
    }

    // True for taken, false for not taken
    fn get_prediction(&self, index: usize) -> bool {
        self.array[index] >= 2
    }

    pub fn train(
        &mut self,
        pc: u64,
        result: BranchResolveFlag,
        _target: u64,
    ) -> BranchPredictorResult {
        if result != BranchResolveFlag::Taken && result != BranchResolveFlag::NotTaken {
            return BranchPredictorResult::NotActive;
        }
        let index = (pc % S as u64) as usize;
        let prediction = self.get_prediction(index);
        if result == BranchResolveFlag::Taken {
            self.saturaing_add(index);
        } else {
            self.saturating_sub(index)
        }

        if prediction == (result == BranchResolveFlag::Taken) {
            BranchPredictorResult::Match
        } else {
            BranchPredictorResult::Mispredict
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        unimplemented!("BimodalPredictor::serialize")
    }
}
