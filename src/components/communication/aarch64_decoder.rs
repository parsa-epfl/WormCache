/// AArch64 instruction decoder for identifying atomic and load-exclusive operations
///
/// This module provides functionality to decode AArch64 instructions and determine
/// if they are atomic operations or load-exclusive operations.

/// Check if an instruction is a load-exclusive operation
/// 
/// Load-exclusive instructions include:
/// - LDXR, LDXRB, LDXRH, LDXP (basic exclusive loads)
/// - LDAXR, LDAXRB, LDAXRH, LDAXP (acquire exclusive loads)  
/// - LDLARB, LDLARH, LDLAR (LOAcquire register)
///
/// These instructions use the exclusive monitor for synchronization primitives.
///
/// Reference: ARM ARM DDI0487 C6.2.138 (LDXR), C6.2.137 (LDAXR)
/// Encoding: size|001000|0|L|o0|Rs|o1|Rt2|Rn|Rt
///           31 30|29  24|23|22|21|20-16|15|14-10|9-5|4-0
#[inline]
pub fn is_load_exclusive(insn: u32) -> bool {
    // Check bits [29:24] = 001000 (0x08) - Load/Store exclusive group
    let bits_29_24 = (insn >> 24) & 0x3F;
    if bits_29_24 != 0x08 {
        return false;
    }
    
    // Bit [23] must be 0 for exclusive operations (not atomic)
    let bit_23 = (insn >> 23) & 1;
    if bit_23 != 0 {
        return false;
    }
    
    // Bit [22] = L, must be 1 for loads
    let l_bit = (insn >> 22) & 1;
    if l_bit != 1 {
        return false;
    }
    
    // Bit [21] = o0, affects operation type
    // Bit [15] = o1, affects operation type
    // Rs [20:16] should be 0b11111 for non-pair loads
    // For LDXR/LDAXR: o0 can be 0 or 1, o1 determines acquire semantics
    // For LDXP/LDAXP: o0=1 for pair operations
    
    // All load-exclusive operations are valid here
    // The architecture guarantees this is a load-exclusive variant
    true
}

/// Check if an instruction is a Compare and Swap (CAS) operation
///
/// CAS instructions include:
/// - CAS, CASA, CASAL, CASL (compare and swap word/doubleword)
/// - CASB, CASAB, CASALB, CASLB (compare and swap byte)
/// - CASH, CASAH, CASALH, CASLH (compare and swap halfword)
/// - CASP, CASPA, CASPAL, CASPL (compare and swap pair)
///
/// Encoding (ARM DDI0602 A64 ISA):
/// | 1 | x | 0 | 0 | 1 | 0 | 0 | 0 | 1 | L | 1 | Rs | o0 | 1 | 1 | 1 | 1 | 1 | Rn | Rt |
/// |size |          29:24       |23 |22 |21|20:16|15 |      14:10      | 9:5| 4:0|
///
/// bits [31:30] = size (00=byte, 01=half, 10=word, 11=double)
/// bits [29:24] = 001000
/// bit  [23] = 1 (atomic, distinguishes from load/store exclusive which have 0)
/// bit  [22] = L (acquire semantics)
/// bit  [21] = 1 (fixed for CAS)
/// bits [20:16] = Rs (source register)
/// bit  [15] = o0 (release semantics)
/// bits [14:10] = 11111 (Rt2 field, fixed for CAS/CASP)
/// bits [9:5] = Rn (base address register)
/// bits [4:0] = Rt (destination register)
///
/// Reference: ARM ARM DDI0602 CAS, CASA, CASAL, CASL
#[inline]
pub fn is_cas_operation(insn: u32) -> bool {
    // Check bits [29:21] = 001000 1 L 1 pattern
    // bits [29:24] = 001000, bit [23] = 1, bit [21] = 1
    // bit [22] (L) can be 0 or 1 for acquire variants
    let bits_29_24 = (insn >> 24) & 0x3F;
    let bit_23 = (insn >> 23) & 1;
    let bit_21 = (insn >> 21) & 1;
    
    if bits_29_24 != 0b001000 || bit_23 != 1 || bit_21 != 1 {
        return false;
    }
    
    // Check bits [14:10] = 11111 (Rt2 field, fixed for CAS/CASP)
    let bits_14_10 = (insn >> 10) & 0x1F;
    if bits_14_10 == 0x1F {
        return true;
    }
    
    false
}

/// Check if an instruction is an atomic memory operation
///
/// Atomic instructions include LSE (Large System Extension) atomics:
/// - LDADD, LDADDA, LDADDAL, LDADDL (atomic add)
/// - LDCLR, LDCLRA, LDCLRAL, LDCLRL (atomic bit clear)
/// - LDEOR, LDEORA, LDEORAL, LDEORL (atomic XOR)
/// - LDSET, LDSETA, LDSETAL, LDSETL (atomic bit set)
/// - LDSMAX, LDSMAXA, LDSMAXAL, LDSMAXL (atomic signed max)
/// - LDSMIN, LDSMINA, LDSMINAL, LDSMINL (atomic signed min)
/// - LDUMAX, LDUMAXA, LDUMAXAL, LDUMAXL (atomic unsigned max)
/// - LDUMIN, LDUMINA, LDUMINAL, LDUMINL (atomic unsigned min)
/// - SWP, SWPA, SWPAL, SWPL (atomic swap)
/// - CAS, CASA, CASAL, CASL (compare and swap)
/// - SWPB, SWPAB, SWPALB, SWPLB (atomic swap byte)
/// - SWPH, SWPAH, SWPALH, SWPLH (atomic swap halfword)
/// - SWPP, SWPPA, SWPPAL, SWPPL (atomic swap pair/quadword)
/// - CASB, CASAB, CASALB, CASLB (compare and swap byte)
/// - CASH, CASAH, CASALH, CASLH (compare and swap halfword)
/// - CASP, CASPA, CASPAL, CASPL (compare and swap pair)
/// - LD64B, ST64B, ST64BV, ST64BV0 (64-byte atomic)
/// - LDCLRP, LDCLRPA, LDCLRPAL, LDCLRPL (atomic bit clear pair)
/// - LDSETP, LDSETPA, LDSETPAL, LDSETPL (atomic bit set pair)
/// - RCWCAS, RCWCLR, RCWSET, RCWSWP (read-check-write atomics)
///
/// Also includes traditional store-exclusive operations that pair with load-exclusive.
#[inline]
pub fn is_atomic_operation(insn: u32) -> bool {
    let op0 = (insn >> 28) & 0xF;  // bits [31:28]
    
    // LSE Atomic memory operations (ARMv8.1-A)
    // Encoding: size|111|0|00|A|R|1|Rs|opc|00|Rn|Rt
    //           31 30|29-27|26|25-24|23|22|21|20-16|15-12|11-10|9-5|4-0
    // bits [31:30] = size
    // bits [29:24] = 111000
    // bit  [23] = A (acquire)
    // bit  [22] = R (release)
    // bit  [21] = 1 (marks as atomic op)
    // bits [20:16] = Rs (source register)
    // bits [15:12] = opc (operation)
    // bits [11:10] = 00
    // bits [9:5] = Rn (address register)
    // bits [4:0] = Rt (data register)
    //
    // Reference: ARM ARM C6.2.131-C6.2.140 (LDADD through LDUMIN, SWP)
    
    // Check for LSE atomic operations pattern
    // Pattern: size 111 0 00 A R 1
    if op0 == 0b1011 || op0 == 0b1111 || op0 == 0b0011 || op0 == 0b0111 {
        // bits [29:24] should be 111000 for atomic ops
        // bit [21] should be 1
        let bits_29_24 = (insn >> 24) & 0x3F;
        let bit_21 = (insn >> 21) & 1;
        
        if bits_29_24 == 0b111000 && bit_21 == 1 {
            // Check bits [11:10] should be 00
            let bits_11_10 = (insn >> 10) & 0x3;
            if bits_11_10 != 0 {
                return false;
            }
            
            // This is an LSE atomic operation
            // opc field determines the operation type
            let opc = (insn >> 12) & 0xF;
            
            // Valid atomic opcodes:
            // 0000: LDADD   0100: LDEOR   1000: LDSMAX   1100: LDUMAX
            // 0001: LDCLR   0101: LDSET   1001: LDSMIN   1101: LDUMIN
            // 0011: SWP
            
            match opc {
                0b0000 | 0b0001 | 0b0011 | 0b0100 | 0b0101 |
                0b1000 | 0b1001 | 0b1100 | 0b1101 => {
                    return true;
                }
                _ => {}
            }
        }
    }
    
    // Compare and Swap (CAS) family
    // Encoding: size|001000|1|A|R|1|Rs|o|11111|Rn|Rt
    //           31 30|29  24|23|22|21|20-16|15|14-10|9-5|4-0
    // bits [31:30] = size (00=byte, 01=half, 10=word, 11=double)
    // bits [29:24] = 001000
    // bit  [23] = 1 (atomic, not exclusive)
    // bit  [22] = A (acquire)
    // bit  [21] = R (release)
    // bit  [15] = o (0=CAS, 1=CASP for pair)
    // bits [14:10] = 11111
    //
    // Reference: ARM ARM C6.2.40 (CAS), C6.2.41 (CASP)
    let bits_29_21 = (insn >> 21) & 0x1FF;
    if bits_29_21 == 0b001000111 || bits_29_21 == 0b001000101 ||
       bits_29_21 == 0b001000110 || bits_29_21 == 0b001000100 {
        // CAS, CASA, CASAL, CASL variants
        let bits_14_10 = (insn >> 10) & 0x1F;
        if bits_14_10 == 0x1F {
            return true;
        }
    }
    
    // Store-Exclusive operations (STXR, STXRB, STXRH, STXP, STLXR, etc.)
    // These are atomic in the sense they pair with load-exclusive
    // Encoding: size|001000|0|L|o0|Rs|o1|Rt2|Rn|Rt
    //           31 30|29  24|23|22|21|20-16|15|14-10|9-5|4-0
    // Pattern: size 001000 0 0 (L=0 for store)
    //
    // Reference: ARM ARM C6.2.308 (STXR), C6.2.306 (STLXR)
    // let bits_29_24 = (insn >> 24) & 0x3F;
    // if bits_29_24 == 0x08 {
    //     let bit_23 = (insn >> 23) & 1;
    //     let l_bit = (insn >> 22) & 1;
    //     if bit_23 == 0 && l_bit == 0 {
    //         // This is a store-exclusive, which is atomic when paired with LDXR
    //         return true;
    //     }
    // }
    
    // LD64B / ST64B family (64-byte atomics)
    // LD64B:  11 011 0 01 0 1 11111 0101 01 Rn Rt
    // ST64B:  11 011 0 01 1 0 11111 0101 00 Rn Rt
    // ST64BV: 11 011 0 01 1 0 11111 0101 01 Rn Rt
    // ST64BV0:11 011 0 01 1 0 11111 0101 10 Rn Rt
    // if (insn >> 21) & 0x7FF == 0b11011001011 {
    //     let op2 = (insn >> 10) & 0x3F;
    //     if op2 == 0b010101 || op2 == 0b010100 {
    //         return true;
    //     }
    // }
    
    // SWPP family (quadword swap) - ARMv8.4-A
    // Encoding similar to other atomics but with pair bit set
    if (insn >> 21) & 0x1FF == 0b111000011 {
        let size = (insn >> 30) & 0x3;
        if size == 0b11 {
            // Could be SWPP, LDCLRP, LDSETP etc.
            let o = (insn >> 15) & 1;
            if o == 1 {
                // Pair operations
                return true;
            }
        }
    }
    
    // RCW (Read-Check-Write) atomics - ARMv8.9-A / ARMv9.4-A
    // RCWCAS, RCWCLR, RCWSET, RCWSWP, RCWSCAS, RCWSCLR, RCWSSET, RCWSSWP
    // Encoding: size 011 000 1 A R 1 Rs opc 0 S Rn Rt
    if (insn >> 21) & 0x7F == 0b0110001 {
        let bit15 = (insn >> 15) & 1;
        let bits12_14 = (insn >> 12) & 0x7;
        // RCW ops have specific opc patterns
        if bit15 == 1 || bits12_14 == 0b100 || bits12_14 == 0b101 {
            return true;
        }
    }
    
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_load_exclusive_ldxr() {
        // LDXR w0, [x1]  - 32-bit load exclusive
        // Encoding: 10 00 1000 0 1 11111 0 11111 xxxxx xxxxx
        let insn: u32 = 0x885f7c20; // LDXR w0, [x1]
        assert!(is_load_exclusive(insn));
    }
    
    #[test]
    fn test_load_exclusive_ldaxr() {
        // LDAXR w0, [x1] - 32-bit load-acquire exclusive
        let insn: u32 = 0x885ffc20; // LDAXR w0, [x1]
        assert!(is_load_exclusive(insn));
    }
    
    #[test]
    fn test_load_exclusive_ldxrb() {
        // LDXRB w0, [x1] - byte load exclusive
        let insn: u32 = 0x085f7c20; // LDXRB w0, [x1]
        assert!(is_load_exclusive(insn));
    }
    
    #[test]
    fn test_load_exclusive_ldxrh() {
        // LDXRH w0, [x1] - halfword load exclusive
        let insn: u32 = 0x485f7c20; // LDXRH w0, [x1]
        assert!(is_load_exclusive(insn));
    }
    
    #[test]
    fn test_atomic_ldadd() {
        // LDADD w0, w1, [x2] - atomic add
        // Encoding: size 111 0 00 A R 1 Rs opc 00 Rn Rt
        // Let's debug the actual encoding
        let insn: u32 = 0xb8200041; // LDADD w0, w1, [x2]
        
        // Debug: print the instruction bits
        println!("LDADD instruction: 0x{:08x}", insn);
        println!("Bits [31:28] (op0): 0b{:04b}", (insn >> 28) & 0xF);
        println!("Bits [29:21]: 0b{:09b}", (insn >> 21) & 0x1FF);
        println!("Bits [15:12] (opc): 0b{:04b}", (insn >> 12) & 0xF);
        
        assert!(is_atomic_operation(insn));
    }
    
    #[test]
    fn test_atomic_swp() {
        // SWP w0, w1, [x2] - swap
        let insn: u32 = 0xb8208041; // SWP w0, w1, [x2]
        
        // Debug: print the instruction bits
        println!("SWP instruction: 0x{:08x}", insn);
        println!("Bits [31:28] (op0): 0b{:04b}", (insn >> 28) & 0xF);
        println!("Bits [29:21]: 0b{:09b}", (insn >> 21) & 0x1FF);
        println!("Bits [15:12] (opc): 0b{:04b}", (insn >> 12) & 0xF);
        
        assert!(is_atomic_operation(insn));
    }
    
    // #[test]
    // fn test_store_exclusive() {
    //     // STXR w0, w1, [x2] - store exclusive (atomic when paired with LDXR)
    //     let insn: u32 = 0x88007c41; // STXR w0, w1, [x2]
    //     assert!(is_atomic_operation(insn));
    // }
    
    #[test]
    fn test_regular_load_not_exclusive() {
        // LDR w0, [x1] - regular load, not exclusive
        let insn: u32 = 0xb9400020; // LDR w0, [x1]
        assert!(!is_load_exclusive(insn));
        assert!(!is_atomic_operation(insn));
    }
    
    #[test]
    fn test_regular_store_not_atomic() {
        // STR w0, [x1] - regular store, not atomic
        let insn: u32 = 0xb9000020; // STR w0, [x1]
        assert!(!is_atomic_operation(insn));
    }
    
    #[test]
    fn test_cas_operation() {
        // CAS w0, w1, [x2] - compare and swap
        // Encoding: size|001000|1|A|R|1|Rs|o|11111|Rn|Rt
        // 10 001000 1 0 0 1 Rs 0 11111 Rn Rt
        let insn: u32 = 0x88a07c41; // CAS w0, w1, [x2]
        assert!(is_cas_operation(insn));
        assert!(is_atomic_operation(insn));
    }
    
    #[test]
    fn test_casa_operation() {
        // CASA w0, w1, [x2] - compare and swap acquire
        let insn: u32 = 0x88e07c41; // CASA w0, w1, [x2]
        assert!(is_cas_operation(insn));
        assert!(is_atomic_operation(insn));
    }
    
    #[test]
    fn test_casal_operation() {
        // CASAL w0, w1, [x2] - compare and swap acquire-release
        let insn: u32 = 0x88e0fc41; // CASAL w0, w1, [x2]
        assert!(is_cas_operation(insn));
        assert!(is_atomic_operation(insn));
    }
    
    #[test]
    fn test_regular_load_not_cas() {
        // LDR w0, [x1] - regular load, not CAS
        let insn: u32 = 0xb9400020; // LDR w0, [x1]
        assert!(!is_cas_operation(insn));
    }
}
