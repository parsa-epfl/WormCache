use std::ops::DerefMut;

pub trait CCell<T> {
    // the cache line cell.
    fn new(incoming: T) -> Self;
    fn inner(&self) -> impl DerefMut<Target = T>;

    fn support_parallel_access() -> bool;
}

// The purpose of CCell to unity the implementation for single-threaded and multithreaded application.

use std::cell::RefCell;

impl<T> CCell<T> for RefCell<T> {
    fn new(set: T) -> Self {
        RefCell::new(set)
    }

    fn inner(&self) -> impl DerefMut<Target = T> {
        self.borrow_mut()
    }

    fn support_parallel_access() -> bool {
        false
    }
}

use spin::mutex::SpinMutex;

impl<T> CCell<T> for SpinMutex<T> {
    fn new(set: T) -> Self {
        SpinMutex::new(set)
    }

    fn inner(&self) -> impl DerefMut<Target = T> {
        self.lock()
    }

    fn support_parallel_access() -> bool {
        true
    }
}

use std::cell::UnsafeCell;

impl<T> CCell<T> for UnsafeCell<T> {
    fn new(set: T) -> Self {
        UnsafeCell::new(set)
    }

    fn inner(&self) -> impl DerefMut<Target = T> {
        unsafe { &mut *self.get() }
    }

    fn support_parallel_access() -> bool {
        false
    }
}
