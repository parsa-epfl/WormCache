use crate::components::bp::BranchResolveFlag;

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

    pub fn train(&mut self, pc: u64, result: BranchResolveFlag, target: u64) {
        if result != BranchResolveFlag::Taken && result != BranchResolveFlag::NotTaken {
            self.history = (self.history << 1) | 1;
            return;
        }
        let taken = result == BranchResolveFlag::Taken;
        let index = ((pc ^ self.history) % S as u64) as usize;
        if taken {
            self.saturaing_add(index);
        } else {
            self.saturating_sub(index)
        }
        self.history = (self.history << 1) | (if taken { 1 } else { 0 });
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

    pub fn serialize(&self) -> Vec<u8> {
        unimplemented!("GShare::serialize")
    }
}
