struct LoopPredictorEntry {
    target: u64,
    tag: u64,
    valid: bool,
    terminal: u64,
    current: u64
}

struct LoopPredictor<const S: usize> {
    array: Vec<LoopPredictorEntry>,
}

impl<const S: usize> LoopPredictor<S> {
    pub fn new() -> LoopPredictor<S> {
        LoopPredictor {
            array: Vec::from_iter(std::iter::repeat(LoopPredictorEntry {
                target: 0,
                tag: 0,
                valid: false,
                terminal: 0,
                current: 0
            }).take(S)),
        }
    }

    pub fn train(&mut self, pc: u64, result: BranchResolveFlag, target: u64) {
        // Loop predictor is not very easy. It has special replacement policy. 
        unimplemented!("LoopPredictor::train")
    }

    pub fn serialize(&self) -> Vec<u8> {
        unimplemented!("LoopPredictor::serialize")
    }
}