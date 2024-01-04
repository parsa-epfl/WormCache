
use std::ffi;
use super::aarch64::BranchType;

pub const fn get_callback(branch_type: BranchType) -> unsafe extern "C" fn(u32, *mut ffi::c_void) {
    match branch_type {
        BranchType::B => b_exec,
        BranchType::BCon => bcon_exec,
        BranchType::BCCon => bccon_exec,
        BranchType::BL => bl_exec,
        BranchType::BLR => blr_exec,
        BranchType::BLRPAuth => blrpauth_exec,
        BranchType::BR => br_exec,
        BranchType::BRPAuth => brpauth_exec,
        BranchType::CBZ => cbz_exec,
        BranchType::CBNZ => cbnz_exec,
        BranchType::RET => ret_exec,
        BranchType::RETPAuth => retpauth_exec,
        BranchType::RETEnhancedPAuthImm => ret_enhaced_pauth_imm_exec,
        BranchType::RETEnhancedPAuthReg => ret_enhanced_pauth_reg_exec,
        BranchType::TBNZ => tbnz_exec,
        BranchType::TBZ => tbz_exec,
    }
}

pub unsafe extern "C" fn b_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn bcon_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn bccon_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn bl_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn blr_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn blrpauth_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn br_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn brpauth_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn cbz_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn cbnz_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn ret_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn retpauth_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn ret_enhaced_pauth_imm_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn ret_enhanced_pauth_reg_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn tbnz_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}

pub unsafe extern "C" fn tbz_exec(
    _: u32,
    meta_data: *mut ffi::c_void, 
) {
}