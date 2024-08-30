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
