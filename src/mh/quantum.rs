use std::sync::{atomic::AtomicUsize, Barrier, Condvar, Mutex};

#[derive(Debug)]
pub struct QuantumManager {
    pending_elements: Mutex<usize>,
    p_e_cv: Condvar,
    total_elements: usize,

    barrier_counter: Mutex<usize>,
    barrier_cv: Condvar,

    turns: AtomicUsize,
}

impl QuantumManager {
    pub fn new(instrumented_vcpu: usize) -> Self {
        return QuantumManager {
            pending_elements: Mutex::new(0),
            p_e_cv: Condvar::new(),

            total_elements: instrumented_vcpu,

            barrier_counter: Mutex::new(0),
            barrier_cv: Condvar::new(),

            turns: AtomicUsize::new(0),
        };
    }

    /// Return whether you can continue.
    pub fn vcpu_wait(&self) -> bool {
        // If current CPU is stopped, we immediate return false.
        unsafe {
            if !crate::qemu_api::qemu_plugin_is_current_cpu_can_run() {
                return false;
            }
        }

        // Wait from vcpu side
        let mut element = self.pending_elements.lock().unwrap();
        *element += 1;
        // immediately give the lock to other.
        drop(element);
        self.p_e_cv.notify_one();

        // Now, waiting for the confirmation of the next one.
        let mut barrier_element_count = self.barrier_counter.lock().unwrap();
        // put myself there.
        *barrier_element_count += 1;

        // DMN, this is the last arriving thread, so I return directly.
        if *barrier_element_count == self.total_elements + 1 {
            *barrier_element_count = 0;
            self.barrier_cv.notify_all();
            return true;
        }

        // If we arrive here, it means not all threads are there.
        loop {
            let (locked, timeout) = self
                .barrier_cv
                .wait_timeout(barrier_element_count, std::time::Duration::from_secs(1))
                .unwrap();

            if timeout.timed_out() {
                unsafe {
                    if !crate::qemu_api::qemu_plugin_is_current_cpu_can_run() {
                        return false;
                    } else {
                        // so nothing happen, we have to continue to wait.
                        barrier_element_count = locked;
                    }
                }
            } else {
                // Okay, so it is a notification wake up, we then can return.
                return true;
            }
        }
    }

    pub fn quantum_thread_exec(&self) {
        loop {
            // wait for the pending elements to become total_element
            let mut element = self
                .p_e_cv
                .wait_while(self.pending_elements.lock().unwrap(), |el| {
                    return *el == self.total_elements;
                })
                .unwrap();
            // OK, we have all, then we clean the pending elements.
            *element = 0;
            self.turns.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            // Well, I think I need to release the lock of element here.
            drop(element);

            // there might be a handler before doing advancement.
            // E.g., increase time, but it is a different problem.
            // Let's all vCPU move advance!
            let mut barrier_element_count = self.barrier_counter.lock().unwrap();
            *barrier_element_count += 1;

            if *barrier_element_count == self.total_elements + 1 {
                // Nice, I will notify other.
                *barrier_element_count = 0;
                self.barrier_cv.notify_all();
            } else {
                // I just wait for others to wake me up.
                drop(self.barrier_cv.wait(barrier_element_count).unwrap());
            }

            unsafe {
                // the time is updated here. 50K quantum means 20us increment.
                crate::qemu_api::qemu_plugin_advance_vm_time((crate::QUAMTUM / 5 * 2).try_into().unwrap());
            }
        }
    }

    pub fn get_turns(&self) -> usize {
        return self.turns.load(std::sync::atomic::Ordering::SeqCst);
    }
}
