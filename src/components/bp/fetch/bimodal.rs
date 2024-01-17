use crate::components::bp::BranchResolveFlag;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct BimodalPredictor<const S: usize> {
    array: Vec<u8>,
    // 0, 1 -> Miss
    // 2, 3 -> Hit
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

    pub fn train(&mut self, pc: u64, result: BranchResolveFlag, target: u64) {
        if result != BranchResolveFlag::Taken && result != BranchResolveFlag::NotTaken {
            return;
        }
        let index = (pc % S as u64) as usize;
        if result == BranchResolveFlag::Taken {
            self.saturaing_add(index);
        } else {
            self.saturating_sub(index)
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        unimplemented!("BimodalPredictor::serialize")
    }
}
