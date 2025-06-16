use crate::components::cache_hierarchy::CacheBlockRequest;

pub fn get_base_pc_offset(request: &CacheBlockRequest, n_blk: usize) -> (u64, u64, u64) {
    let pc = request.pc;
    let base = request.block_id >> (n_blk.trailing_zeros());
    let offset = request.block_id & ((1 << n_blk.trailing_zeros()) - 1);
    (base, pc, offset)
}

pub fn get_address(base: u64, offset: u64, n_blk: usize) -> u64 {
    let index_len = n_blk.trailing_zeros();
    assert!(index_len > 0);
    (base << index_len) | offset
}

pub fn build_key(pc: u64, offset: u64, n_sets: usize) -> u64 {
    const PC_WIDTH: u64 = 16;
    const OFF_WIDTH: u64 = 0;                       // Use PC based indexing for now
    let index_len = n_sets.trailing_zeros();
    assert!(PC_WIDTH + OFF_WIDTH > index_len.into());

    let pc = pc & ((1 << PC_WIDTH) - 1);
    let offset = offset & ((1 << OFF_WIDTH) - 1);
    let mut key = (pc << OFF_WIDTH) | offset;
    let mut tag = key >> index_len;
    while tag > 0 {
        key ^= tag & ((1 << index_len) - 1);
        tag >>= index_len;
    }
    key
}

pub fn rotate_left<T, const N: usize>(pattern: &mut [T; N], rot_val: usize) {
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