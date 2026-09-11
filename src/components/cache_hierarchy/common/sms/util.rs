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

use crate::components::cache_hierarchy::CacheBlockRequest;
use crate::parameter::PC_WIDTH;

pub fn get_base_pc_offset<const N_BLK: usize>(request: &CacheBlockRequest) -> (u64, u64, u64) {
    let pc = request.pc;
    let base = request.block_id >> (N_BLK.trailing_zeros());
    let offset = request.block_id & ((1 << N_BLK.trailing_zeros()) - 1);
    (base, pc, offset)
}

pub fn get_address<const N_BLK: usize>(base: u64, offset: u64) -> u64 {
    (base << (N_BLK.trailing_zeros())) | offset
}

pub fn build_key<const N_BLK: usize, const PHT_SETS: usize, const ROT: bool>(
    pc: u64,
    offset: u64,
) -> u64 {
    let off_width = N_BLK.trailing_zeros();
    let index_len = PHT_SETS.trailing_zeros();
    assert!(PC_WIDTH + off_width as usize > index_len as usize);

    if ROT {
        // If rotation, then only PC based indexing
        let pc = pc & ((1 << (PC_WIDTH + off_width as usize)) - 1);
        pc
    } else {
        // else (PC + offset) based indexing
        let pc = pc & ((1 << PC_WIDTH) - 1);
        let offset = offset & ((1 << off_width) - 1);
        let key = (pc << off_width) | offset;
        key
    }
}

// TODO: can be made better though traits but for now, this is fine
pub fn rotate_left_vec<T>(pattern: &mut Vec<T>, rot_val: usize) {
    let len = pattern.len();
    if len == 0 || rot_val % len == 0 {
        return;
    }
    pattern.rotate_left(rot_val % len);
}

pub fn rotate_left_arr<T, const N: usize>(pattern: &mut [T; N], rot_val: usize) {
    let len = pattern.len();
    if len == 0 || rot_val % len == 0 {
        return;
    }
    pattern.rotate_left(rot_val % len);
}

pub fn rotate_right<T, const N: usize>(pattern: &mut [T; N], rot_val: usize) {
    let len = pattern.len();
    if len == 0 || rot_val % len == 0 {
        return;
    }
    pattern.rotate_right(rot_val % len);
}

pub enum PatternType {
    Access,
    Read,
    Write,
}
