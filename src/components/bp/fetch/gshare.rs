#![allow(dead_code)]
// BSD 3-Clause License
//
// Copyright (c) 2024, Parallel Systems Architecture Laboratory (PARSA), EPFL.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this
//    list of conditions and the following disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.
//
// 3. Neither the name of the PARSA, EPFL
//    nor the names of its contributors may be used to endorse or promote
//    products derived from this software without specific prior written
//    permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use crate::components::bp::{BranchResolutionResult, BranchType};

use serde::{Deserialize, Serialize};

use super::BranchPredictorResult;

// The maximum size of the global history register is 64 bits.
#[derive(Debug, Deserialize, Serialize)]
pub struct GShare<const S: usize> {
    pub history: u64,
    pub table: Vec<u8>,
}

impl<const S: usize> GShare<S> {
    pub fn new() -> GShare<S> {
        GShare {
            history: 0,
            table: vec![0; S],
        }
    }

    pub fn train(
        &mut self,
        pc: u64,
        result: BranchResolutionResult,
        _target: u64,
    ) -> BranchPredictorResult {
        let taken = result.is_taken;
        let index = ((pc ^ self.history) % S as u64) as usize;
        let prediction = self.get_prediction(index);
        if taken {
            self.saturaing_add(index);
        } else {
            self.saturating_sub(index)
        }

        if result.branch_type == BranchType::Conditional {
            self.update_history(taken);
        }

        if prediction == taken {
            BranchPredictorResult::Match
        } else {
            BranchPredictorResult::Mispredict
        }
    }

    pub fn update_history(&mut self, taken: bool) {
        self.history = (self.history << 1) | u64::from(taken);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_training_updates_counter_and_history() {
        let mut gshare = GShare::<8>::new();
        let result = BranchResolutionResult {
            branch_type: BranchType::Conditional,
            is_taken: true,
        };

        assert!(matches!(
            gshare.train(0, result, 0),
            BranchPredictorResult::Mispredict
        ));
        assert_eq!(gshare.table[0], 1);
        assert_eq!(gshare.history, 1);
    }

    #[test]
    fn explicit_history_update_does_not_train_counter() {
        let mut gshare = GShare::<8>::new();

        gshare.update_history(true);
        gshare.update_history(false);

        assert_eq!(gshare.history, 2);
        assert!(gshare.table.iter().all(|counter| *counter == 0));
    }
}
