use crate::components::cache_hierarchy::CacheBlockRequest;
use crate::parameter::{PC_WIDTH};

pub fn get_base_pc_offset<const N_BLK: usize>(request: &CacheBlockRequest) -> (u64, u64, u64) {
    let pc = request.pc;
    let base = request.block_id >> (N_BLK.trailing_zeros());
    let offset = request.block_id & ((1 << N_BLK.trailing_zeros()) - 1);
    (base, pc, offset)
}

pub fn get_address<const N_BLK: usize>(base: u64, offset: u64) -> u64 {
    (base << (N_BLK.trailing_zeros())) | offset
}

pub fn build_key<const N_BLK: usize, const PHT_SETS: usize, const ROT: bool>(pc: u64, offset: u64) -> u64 {
    let off_width = N_BLK.trailing_zeros();
    let index_len = PHT_SETS.trailing_zeros();
    assert!(PC_WIDTH + off_width as usize > index_len as usize);

    let pc = pc & ((1 << PC_WIDTH) - 1);
    if ROT {    // If rotation, then only PC based indexing
        pc
    } else {    // else (PC + offset) based indexing
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