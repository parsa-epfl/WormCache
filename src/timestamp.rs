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

// generate timestamp.

use std::io::{Read, Write};

use crate::util::get_monotonic_ts;

static mut QEMU_INITIALIZING_TIMESTAMP: u64 = 0;
static mut CHECKPOINT_TIMESTAMP: u64 = 0;

pub fn initialize() {
    unsafe {
        QEMU_INITIALIZING_TIMESTAMP = get_monotonic_ts();
    }
}

#[inline(always)]
pub fn get_ts() -> u64 {
    let current_ts = get_monotonic_ts();
    return current_ts - unsafe { QEMU_INITIALIZING_TIMESTAMP } + unsafe { CHECKPOINT_TIMESTAMP };
}

// Save the starting timestamp.
pub fn serialize(name: &str) {
    let current_ts = get_ts();
    // dump the current_ts to a file.
    let mut file = std::fs::File::create(format!("{}/timestamp", name)).unwrap();
    file.write_all(current_ts.to_string().as_bytes()).unwrap();
    file.flush().unwrap();
}

// Load the starting timestamp.
pub fn deserialize(name: &str) {
    // load the timestamp from the file.
    // If the file does not exist, ignore and set the timestamp to 0.
    if !std::path::Path::new(&format!("{}/timestamp", name)).exists() {
        println!("Checkpoint timestamp file does not exist.");
        unsafe {
            CHECKPOINT_TIMESTAMP = 0;
        }
        return;
    }

    let mut file = std::fs::File::open(format!("{}/timestamp", name)).unwrap();
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).unwrap();
    let current_ts = buffer.trim().parse::<u64>().unwrap();
    unsafe {
        CHECKPOINT_TIMESTAMP = current_ts;
    }
    // print the timestamp.
    println!("Checkpoint timestamp: {}", unsafe { CHECKPOINT_TIMESTAMP });
}
