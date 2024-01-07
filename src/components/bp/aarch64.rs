// Ref: https://developer.arm.com/documentation/dui0802/b/A64-General-Instructions/A64-general-instructions-in-alphabetical-order?lang=en
use crate::qemu_api::{self, qemu_plugin_read_cpu_integer_register};

// This file is not used at all, because we finally added the callback to track QEMU branch resolution.

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum BranchType {
    B,
    BCon,
    BCCon,
    BL,
    BLR,
    BLRPAuth,
    BR,
    BRPAuth,
    CBZ,
    CBNZ,
    RET,
    RETPAuth,
    RETEnhancedPAuthImm,
    RETEnhancedPAuthReg,
    TBZ,
    TBNZ,
    None
}

// Mask, Value, BranchType
const DECODE_MAP: [(u32, u32, BranchType); 16] = [
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/B--Branch-
    (
        0b1111_1100_0000_0000_0000_0000_0000_0000,
        0b0001_0100_0000_0000_0000_0000_0000_0000,
        BranchType::B,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/B-cond--Branch-conditionally-
    (
        0b1111_1111_0000_0000_0000_0000_0001_0000,
        0b0101_0100_0000_0000_0000_0000_0000_0000,
        BranchType::BCon,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/BC-cond--Branch-Consistent-conditionally-
    (
        0b1111_1111_0000_0000_0000_0000_0001_0000,
        0b0101_0100_0000_0000_0000_0000_0001_0000,
        BranchType::BCCon,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/BL--Branch-with-Link-
    (
        0b1111_1100_0000_0000_0000_0000_0000_0000,
        0b1001_0100_0000_0000_0000_0000_0000_0000,
        BranchType::BL,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/BLR--Branch-with-Link-to-Register-
    (
        0b1111_1111_1111_1111_1111_1100_0001_1111,
        0b1101_0110_0011_1111_0000_0000_0000_0000,
        BranchType::BLR,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/BLRAA--BLRAAZ--BLRAB--BLRABZ--Branch-with-Link-to-Register--with-pointer-authentication-
    (
        0b1111_1110_1111_1111_1111_1000_0000_0000,
        0b1101_0110_0011_1111_0000_1000_0000_0000,
        BranchType::BLRPAuth,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/BR--Branch-to-Register-
    (
        0b1111_1111_1111_1111_1111_1100_0001_1111,
        0b1101_0110_0001_1111_0000_0000_0000_0000,
        BranchType::BR,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/BRAA--BRAAZ--BRAB--BRABZ--Branch-to-Register--with-pointer-authentication-
    (
        0b1111_1110_1111_1111_1111_1000_0000_0000,
        0b1101_0110_0001_1111_0000_1000_0000_0000,
        BranchType::BRPAuth,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/CBZ--Compare-and-Branch-on-Zero-
    (
        0b0111_1111_0000_0000_0000_0000_0000_0000,
        0b0011_0100_0000_0000_0000_0000_0000_0000,
        BranchType::CBZ,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/CBNZ--Compare-and-Branch-on-Nonzero-
    (
        0b0111_1111_0000_0000_0000_0000_0000_0000,
        0b0011_0101_0000_0000_0000_0000_0000_0000,
        BranchType::CBNZ,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/RET--Return-from-subroutine-
    (
        0b1111_1111_1111_1111_1111_1100_0001_1111,
        0b1101_0110_0101_1111_0000_0000_0000_0000,
        BranchType::RET,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/RETAA--RETAB--Return-from-subroutine--with-pointer-authentication-
    (
        0b1111_1111_1111_1111_1111_1011_1111_1111,
        0b1101_0110_0101_1111_0000_1011_1111_1111,
        BranchType::RETPAuth,
    ), // constantly use X[30] as the return address
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/RETAASPPC--RETABSPPC--immediate---Return-from-subroutine--with-enhanced-pointer-authentication-return--immediate--
    (
        0b1111_1111_1100_0000_0000_0000_0001_1111,
        0b0101_0101_0000_0000_0000_0000_0001_1111,
        BranchType::RETEnhancedPAuthImm,
    ), // constantly use X[30] as the return address
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/RETAASPPC--RETABSPPC--register---Return-from-subroutine--with-enhanced-pointer-authentication-return--register--
    (
        0b1111_1111_1111_1111_1111_1011_1110_0000,
        0b1101_0110_0101_1111_0000_1011_1110_0000,
        BranchType::RETEnhancedPAuthReg,
    ), // constantly use X[30] as the return address
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/TBZ--Test-bit-and-Branch-if-Zero-
    (
        0b0111_1111_0000_0000_0000_0000_0000_0000,
        0b0011_0110_0000_0000_0000_0000_0000_0000,
        BranchType::TBZ,
    ),
    // Ref: https://developer.arm.com/documentation/ddi0602/2023-12/Base-Instructions/TBNZ--Test-bit-and-Branch-if-Nonzero-
    (
        0b0111_1111_0000_0000_0000_0000_0000_0000,
        0b0011_0111_0000_0000_0000_0000_0000_0000,
        BranchType::TBNZ,
    ),
];

pub fn branch_type(instruction: u32) -> Option<BranchType> {
    for (mask, value, branch_type) in DECODE_MAP.iter() {
        if (instruction & mask) == *value {
            return Some(*branch_type);
        }
    }
    return None;
}

fn sign_extension(imm: u32, msb_index: u32) -> u64 {
    let is_negative = imm & (1 << msb_index) != 0;
    let mask: u64 = if is_negative { u64::MAX } else { 0 } << msb_index;

    return imm as u64 | mask;
}

#[test]
pub fn test_sign_extension() {
    let imm = 0x3ff4b18;
    let msb_index = 25;
    let result = sign_extension(imm, msb_index);
    assert_eq!(result << 2, 0xfffffffffffd2c60);
}

// B, and
pub fn target_b(pc: u64, instruction: u32) -> u64 {
    let imm = instruction & 0x3FFFFFF; // lower 26 bits
    let bias = sign_extension(imm, 25);
    return pc + (bias << 2);
}

// B.cond
pub fn target_bcon(pc: u64, instruction: u32) -> (u64, u64) {
    let imm = (instruction >> 5) & 0x7FFFF; // lower 19 bits
    let bias = sign_extension(imm, 18);
    let condition_code = instruction & 0xf;
    let condition_match = unsafe {
        let flags = 0;
        // let flags = qemu_api::qemu_plugin_get_cvnz();
        let c = flags & 0x1 != 0;
        let v = flags & 0x2 != 0;
        let n = flags & 0x4 != 0;
        let z = flags & 0x8 != 0;
        match condition_code {
            0 => z == true,               // eq
            1 => z == false,              // ne
            2 => c == true,               // cs, carry set
            3 => c == false,              // cc, carry clear
            4 => n == true,               // mi, minus (negative)
            5 => n == false,              // pl, plus (positive or zero)
            6 => v == true,               // vs, vset, signed overflow
            7 => v == false,              // vc, vclear, no signed overflow
            8 => c == true && z == false, // hi, unsigned higher
            9 => c == false || z == true, // ls, unsigned lower or same
            10 => n == v,                 // ge, signed greater than or equal
            11 => n != v,                 // lt, signed less than
            12 => z == false && n == v,   // gt, signed greater than
            13 => z == true || n != v,    // le, signed less than or equal
            14 => true,                   // al, always
            15 => true, // nv, never, but also returns true. (Ref: https://developer.arm.com/documentation/ddi0602/2022-06/Shared-Pseudocode/Shared-Functions?lang=en#impl-shared.ConditionHolds.1)
            _ => unreachable!("Invalid condition code: {}", condition_code),
        }
    };
    if condition_match {
        return (pc + (bias << 2), pc + 4);
    } else {
        return (pc + 4, pc + (bias << 2));
    }
}

// BC.cond
pub fn target_bccon(pc: u64, instruction: u32) -> (u64, u64) {
    return target_bcon(pc, instruction);
}

// BL
pub fn target_bl(pc: u64, instruction: u32) -> u64 {
    return target_b(pc, instruction);
}

// BLR
pub fn target_blr(pc: u64, instruction: u32) -> u64 {
    let reg = (instruction >> 5) & 0x1F;
    let target = unsafe { qemu_api::qemu_plugin_read_cpu_integer_register(reg as i32) };
    return target;
}

// BLRAA, BLRAAZ, BLRAB, BLRABZ
pub fn target_blrpauth(pc: u64, instruction: u32) -> u64 {
    let raw_target = target_blr(pc, instruction);
    let m = (instruction >> 10) & 0x1;
    let z = (instruction >> 24) & 0x1;
    let rm = instruction & 0x1F;

    return unsafe {
        let modifier = if z == 0 {
            assert!(rm == 0x1F);
            0
        } else {
            qemu_plugin_read_cpu_integer_register(rm as i32)
        };
        // qemu_api::qemu_plugin_resolve_pointer_authentication(raw_target, m as u64, modifier)
        modifier
    };
}

// BR
pub fn target_br(pc: u64, instruction: u32) -> u64 {
    return target_blr(pc, instruction);
}

// BRAA, BRAAZ, BRAB, BRABZ
pub fn target_brpauth(pc: u64, instruction: u32) -> u64 {
    return target_blrpauth(pc, instruction);
}

// CBZ
pub fn target_cbz(pc: u64, instruction: u32) -> (u64, u64) {
    let rt = instruction & 0x1F;
    let imm = (instruction >> 5) & 0x7FFFF; // lower 19 bits
    return unsafe {
        if qemu_plugin_read_cpu_integer_register(rt as i32) == 0 {
            (pc + (sign_extension(imm, 18) << 2), pc + 4)
        } else {
            (pc + 4, pc + (sign_extension(imm, 18) << 2))
        }
    };
}

// CBNZ
pub fn target_cbnz(pc: u64, instruction: u32) -> (u64, u64) {
    let rt = instruction & 0x1F;
    let imm = (instruction >> 5) & 0x7FFFF; // lower 19 bits
    return unsafe {
        if qemu_plugin_read_cpu_integer_register(rt as i32) != 0 {
            (pc + (sign_extension(imm, 18) << 2), pc + 4)
        } else {
            (pc + 4, pc + (sign_extension(imm, 18) << 2))
        }
    };
}

// RET
pub fn target_ret(pc: u64, instruction: u32) -> u64 {
    return target_br(pc, instruction);
}

// RETAA, RETAB
pub fn target_retpauth(pc: u64, instruction: u32) -> u64 {
    let m = (instruction >> 10) & 0x1;
    let target = unsafe { qemu_plugin_read_cpu_integer_register(30) };
    let sp: u64 = unsafe { qemu_plugin_read_cpu_integer_register(31) };
    // return unsafe { qemu_api::qemu_plugin_resolve_pointer_authentication(target, m as u64, sp) };
    return target;
}

// RETAASPPC, RETABSPPC, immediate
pub fn target_ret_enhanced_pauth_imm(pc: u64, instruction: u32) -> u64 {
    unimplemented!("RETAASPPC and RETABSPPC are not implemented yet.");
}

// RETAASPPC, RETABSPPC, register
pub fn target_ret_enhanced_pauth_reg(pc: u64, instruction: u32) -> u64 {
    unimplemented!("RETAASPPC and RETABSPPC are not implemented yet.");
}

// TBNZ
pub fn target_tbnz(pc: u64, instruction: u32) -> (u64, u64) {
    let rt = instruction & 0x1F;
    let rt = unsafe { qemu_plugin_read_cpu_integer_register(rt as i32) };
    let imm14 = (instruction >> 5) & 0x3FFF; // lower 14 bits
    let bit_op_msb = (instruction >> 31) & 0x1;
    let bit_op_lsbs = (instruction >> 19) & 0x1f;
    let bit_op = (bit_op_msb << 5) | bit_op_lsbs;

    if (rt & (1 << bit_op)) != 0 {
        return (pc + (sign_extension(imm14, 13) << 2), pc + 4);
    } else {
        return (pc + 4, pc + (sign_extension(imm14, 13) << 2));
    }
}

// TBZ
pub fn target_tbz(pc: u64, instruction: u32) -> (u64, u64) {
    let rt = instruction & 0x1F;
    let rt = unsafe { qemu_plugin_read_cpu_integer_register(rt as i32) };
    let imm14 = (instruction >> 5) & 0x3FFF; // lower 14 bits
    let bit_op_msb = (instruction >> 31) & 0x1;
    let bit_op_lsbs = (instruction >> 19) & 0x1f;
    let bit_op = (bit_op_msb << 5) | bit_op_lsbs;

    if (rt & (1 << bit_op)) == 0 {
        return (pc + (sign_extension(imm14, 13) << 2), pc + 4);
    } else {
        return (pc + 4, pc + (sign_extension(imm14, 13) << 2));
    }
}

pub fn check_opcode_match(opcode: &str, branch_type: BranchType) -> bool {
    match branch_type {
        BranchType::B => opcode == "b",
        BranchType::BCon => opcode.starts_with('b') && opcode.contains('.'),
        BranchType::BCCon => opcode.starts_with("bc") && opcode.contains('.'),
        BranchType::BL => opcode == "bl",
        BranchType::BLR => opcode == "blr",
        BranchType::BLRPAuth => opcode == "blraa" || opcode == "blraaz" || opcode == "blrab" || opcode == "blrabz",
        BranchType::BR => opcode == "br",
        BranchType::BRPAuth => opcode == "braa" || opcode == "braaz" || opcode == "brab" || opcode == "brabz",
        BranchType::CBZ => opcode == "cbz",
        BranchType::CBNZ => opcode == "cbnz",
        BranchType::RET => opcode == "ret",
        BranchType::RETPAuth => opcode == "retaa" || opcode == "retab",
        BranchType::RETEnhancedPAuthImm => opcode == "retaa.sppc" || opcode == "retab.sppc",
        BranchType::RETEnhancedPAuthReg => opcode == "retaa.sppc" || opcode == "retab.sppc",
        BranchType::TBZ => opcode == "tbz",
        BranchType::TBNZ => opcode == "tbnz",
        BranchType::None => unreachable!("Invalid branch type: {:?}", branch_type)
    }
}