use std::collections::LinkedList;

use crate::components::bp::BranchResolveFlag;
use serde::{Deserialize, Serialize};

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

    pub fn train(&mut self, pc: u64, result: BranchResolveFlag, _target: u64) {
        if result != BranchResolveFlag::Call && result != BranchResolveFlag::Return {
            return;
        }
        if result == BranchResolveFlag::Call {
            self.stack.push_back(pc + 4);
        } else {
            self.stack.pop_back();
        }
    }

    fn push_and_evict(&mut self, pc: u64) {
        if self.stack.len() == S {
            self.stack.pop_front();
        }
        self.stack.push_back(pc);
    }
}
