
trait CoherenceProtocol {
    fn is_hit(&self);
}

// Each coherence protocol has two parts:
// - The state machine (next state) of the cache line.
// - The state machine of the directory entry.