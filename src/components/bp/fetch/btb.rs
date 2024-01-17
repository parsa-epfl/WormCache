use crate::components::bp::BranchResolveFlag;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct BTBEntry {
    tag: u64,
    target: u64,
    valid: bool,
}

#[derive(Deserialize, Serialize)]
pub struct BTB <const S: usize> {
    array: Vec<BTBEntry>,
}

impl<const S: usize> BTB<S> {
    pub fn new() -> BTB<S> {
        BTB {
            array: Vec::from_iter((0..S).map(|_| BTBEntry {
                tag: 0,
                target: 0,
                valid: false,
            })),
        }
    }

    pub fn train(&mut self, pc: u64, result: BranchResolveFlag, target: u64) {
        if result == BranchResolveFlag::NotTaken {
            return;
        }
        let index = (pc % S as u64) as usize;
        self.array[index].tag = pc;
        self.array[index].target = target;
        self.array[index].valid = true;
    }
}

