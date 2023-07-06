use std::sync::{Barrier, Condvar, Mutex, atomic::AtomicUsize};

#[derive(Debug)]
pub struct QuantumManager {
    pending_elements: Mutex<usize>,
    cv: Condvar,
    total_elements: usize,
    barrier: Barrier,
    turns: AtomicUsize
}

impl QuantumManager {
    pub fn new(instrumented_vcpu: usize) -> Self {
        return QuantumManager {
            pending_elements: Mutex::new(0),
            cv: Condvar::new(),
            total_elements: instrumented_vcpu,
            barrier: Barrier::new(instrumented_vcpu + 1),
            turns: AtomicUsize::new(0)
        };
    }

    pub fn vcpu_wait(&self) {
        // Wait from vcpu side
        let mut element = self.pending_elements.lock().unwrap();
        *element += 1;
        // immediately give the lock to other.
        drop(element);
        self.cv.notify_one();
        self.barrier.wait();
    }

    pub fn quantum_thread_exec(&self) {
        loop {
            // wait for the pending elements to become total_element
            let mut element = self.cv.wait_while(self.pending_elements.lock().unwrap(), |el| {
                return *el == self.total_elements;
            }).unwrap();
            // OK, we have all, then we clean the pending elements.
            *element = 0;
            self.turns.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            // Well, I think I need to release the lock of element here.
            drop(element);

            // there might be a handler before doing advancement.
            // E.g., increase time, but it is a different problem.
            // Let's all vCPU move advance!
            self.barrier.wait();
        }
    }

    pub fn get_turns(&self) -> usize {
        return self.turns.load(std::sync::atomic::Ordering::SeqCst);
    }

}
