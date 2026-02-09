pub fn get_tag_setidx<const N_PC: usize, const RPT_SETS: usize>(pc: u64) -> (u64, u64) {
    let index_len = RPT_SETS.trailing_zeros();
    assert!(N_PC > index_len as usize);

    let pc = pc & ((1 << N_PC) - 1);
    let tag = pc >> index_len;
    let set_idx = pc & ((1 << index_len) - 1);
    (tag, set_idx)
}