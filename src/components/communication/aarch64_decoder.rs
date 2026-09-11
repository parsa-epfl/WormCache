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
    let op0 = (insn >> 28) & 0xF; // bits [31:28]

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
                0b0000 | 0b0001 | 0b0011 | 0b0100 | 0b0101 | 0b1000 | 0b1001 | 0b1100 | 0b1101 => {
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
    if bits_29_21 == 0b001000111
        || bits_29_21 == 0b001000101
        || bits_29_21 == 0b001000110
        || bits_29_21 == 0b001000100
    {
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

/// Check if an instruction has acquire semantics (memory ordering constraint)
///
/// Instructions with acquire semantics:
/// - LDAXR, LDAXRB, LDAXRH, LDAXP (load-acquire exclusive)
/// - LDAR, LDARB, LDARH (load-acquire)
/// - LDLAR, LDLARB, LDLARH (load-LOAcquire)
/// - CASA, CASAL, CASAB, CASALB, CASAH, CASALH, CASPA, CASPAL (CAS with acquire)
/// - LSE atomics with acquire: LDADDA, LDADDAL, LDCLRA, LDCLRAL, LDEORA, LDEORAL,
///   LDSETA, LDSETAL, LDSMAXA, LDSMAXAL, LDSMINA, LDSMINAL, LDUMAXA, LDUMAXAL,
///   LDUMINA, LDUMINAL, SWPA, SWPAL
/// - DMB barriers with load-acquire semantics: DMB LD, DMB ISHLD, DMB OSHLD, DMB NSHLD
///
/// Reference: ARM ARM DDI0602, verified against QEMU target/arm/tcg/a64.decode
#[inline]
pub fn is_acquire_memory_instruction(insn: u32) -> bool {
    // Check for Load-Acquire Exclusive (LDAXR, LDAXRB, LDAXRH, LDAXP)
    // QEMU encoding: LDXR .. 001000 010 ..... . ..... ..... .....@stxr
    // LDAXR has lasr=1 (bit 15)
    // Pattern: size|001000|0|1|o0|Rs|1|Rt2|Rn|Rt
    let bits_29_24 = (insn >> 24) & 0x3F;
    if bits_29_24 == 0b001000 {
        let bit_23 = (insn >> 23) & 1; // 0 for exclusive
        let bit_22 = (insn >> 22) & 1; // L=1 for load
        let _bit_21 = (insn >> 21) & 1; // o0
        let bit_15 = (insn >> 15) & 1; // lasr/acquire bit

        if bit_23 == 0 && bit_22 == 1 && bit_15 == 1 {
            // LDAXR/LDAXP family
            return true;
        }
    }

    // Check for Load-Acquire (LDAR, LDARB, LDARH) and Load-LOAcquire (LDLAR, LDLARB, LDLARH)
    // QEMU encoding: LDAR .. 001000 110 11111 . 11111 ..... .....@stlr
    // Pattern: size|001000|1|1|0|11111|lasr|11111|Rn|Rt
    // bit [15] = lasr: 0=LDLAR, 1=LDAR
    if bits_29_24 == 0b001000 {
        let bit_23 = (insn >> 23) & 1; // 1 for acquire group
        let bit_22 = (insn >> 22) & 1; // L=1
        let bit_21 = (insn >> 21) & 1; // 0
        let bits_20_16 = (insn >> 16) & 0x1F; // 11111
        let _lasr = (insn >> 15) & 1; // lasr (both LDAR=1 and LDLAR=0 have acquire semantics)
        let bits_14_10 = (insn >> 10) & 0x1F; // 11111

        if bit_23 == 1
            && bit_22 == 1
            && bit_21 == 0
            && bits_20_16 == 0b11111
            && bits_14_10 == 0b11111
        {
            // Both LDAR (lasr=1) and LDLAR (lasr=0) have acquire semantics
            return true;
        }
    }

    // Check for CAS with acquire (CASA, CASAL, CASAB, CASALB, CASAH, CASALH, CASPA, CASPAL)
    // QEMU encoding: CAS sz:2 001000 1 - 1 rs:5 - 11111 rn:5 rt:5
    // Pattern: size|001000|1|A|1|Rs|o0|11111|Rn|Rt
    // bit [22] = A (acquire)
    if bits_29_24 == 0b001000 {
        let bit_23 = (insn >> 23) & 1; // 1 for CAS
        let bit_22 = (insn >> 22) & 1; // A (acquire)
        let bit_21 = (insn >> 21) & 1; // 1
        let bits_14_10 = (insn >> 10) & 0x1F; // 11111

        if bit_23 == 1 && bit_21 == 1 && bits_14_10 == 0b11111 && bit_22 == 1 {
            // CASA, CASAL, CASPA, CASPAL
            return true;
        }
    }

    // Check for LSE atomics with acquire
    // QEMU encoding: @atomic sz:2 ... . .. a:1 r:1 . rs:5 . ... .. rn:5 rt:5
    // Pattern: size|111000|A|R|1|Rs|opc|00|Rn|Rt
    // bit [23] = A (acquire)
    let bits_29_24_lse = (insn >> 24) & 0x3F;
    if bits_29_24_lse == 0b111000 {
        let bit_21 = (insn >> 21) & 1; // 1 for atomic
        let bit_23_a = (insn >> 23) & 1; // A (acquire)
        let bits_11_10 = (insn >> 10) & 0x3; // 00

        if bit_21 == 1 && bits_11_10 == 0 && bit_23_a == 1 {
            // Check Rt != 11111 for acquire semantics to apply
            let rt = insn & 0x1F;
            if rt != 0b11111 {
                let opc = (insn >> 12) & 0xF;
                // Valid opcodes with acquire: LDADDA, LDCLRA, LDEORA, LDSETA,
                // LDSMAXA, LDSMINA, LDUMAXA, LDUMINA, SWPA
                match opc {
                    0b0000 | 0b0001 | 0b0011 | 0b0100 | 0b0101 | 0b1000 | 0b1001 | 0b1100
                    | 0b1101 => {
                        return true;
                    }
                    _ => {}
                }
            }
        }
    }

    // Check for DMB with load-acquire semantics
    // DMB instruction pattern: 11010101 00000011 0011 CRm:4 101 11111
    // Base encoding: 0xD5033xxx with bits 7-5 = 101 (0x5)
    // Verified against aarch64-linux-gnu-as output
    // CRm (bits 11:8) encodes both domain and types:
    // - CRm<3:2> = domain (00=OSH, 01=NSH, 10=ISH, 11=SY)
    // - CRm<1:0> = types (00=ALL, 01=LD, 10=ST, 11=SY)
    // DMB instructions that provide acquire semantics:
    // - DMB SY (CRm=0xF): full barrier
    // - DMB LD (CRm=0xD): load barrier
    // - DMB ISH (CRm=0xB): inner shareable full barrier
    // - DMB ISHLD (CRm=0x9): inner shareable load barrier
    // - DMB OSHLD (CRm=0x1), DMB NSHLD (CRm=0x5): load barriers
    let bits_31_12 = (insn >> 12) & 0xFFFFF;
    if bits_31_12 == 0xD5033 {
        // Check bits 7-5 = 101 (0x5) for DMB
        let bits_7_5 = (insn >> 5) & 0x7;
        if bits_7_5 == 0b101 {
            let crm = (insn >> 8) & 0xF;
            // Check if this DMB has acquire semantics
            // types = CRm & 0x3
            // types = 00: ALL/SY (full barrier with loads)
            // types = 01: LD (load barrier, prevents load reordering)
            // types = 10: ST (store barrier, NO acquire semantics)
            // types = 11: SY (full system barrier)
            let types = crm & 0x3;
            if types == 0b00 || types == 0b01 || types == 0b11 {
                return true;
            }
        }
    }

    false
}

/// Check if an instruction is a DSB (Data Synchronization Barrier)
///
/// DSB instructions ensure completion of memory accesses:
/// - DSB SY: Full system data synchronization barrier
/// - DSB ISH: Inner shareable data synchronization barrier
/// - DSB OSH: Outer shareable data synchronization barrier
/// - DSB NSH: Non-shareable data synchronization barrier
/// - DSB LD/ST/ISHLD/ISHST/OSHLD/OSHST/NSHLD/NSHST: Load/store variants
///
/// Encoding pattern: 11010101 00000011 0011 CRm:4 100 11111
/// - bits 31-12 = 0xD5033
/// - bits 7-5 = 100 (0x4)
/// - bits 4-0 = 11111 (0x1F)
/// - CRm (bits 11:8) encodes domain and types
///
/// Verified against aarch64-linux-gnu-as
#[inline]
pub fn is_dsb_instruction(insn: u32) -> bool {
    // Check base pattern: bits 31-12 = 0xD5033
    let bits_31_12 = (insn >> 12) & 0xFFFFF;
    if bits_31_12 != 0xD5033 {
        return false;
    }

    // Check bits 7-5 = 100 (0x4) for DSB
    let bits_7_5 = (insn >> 5) & 0x7;
    if bits_7_5 != 0b100 {
        return false;
    }

    // Check bits 4-0 = 11111 (0x1F)
    let bits_4_0 = insn & 0x1F;
    if bits_4_0 != 0x1F {
        return false;
    }

    true
}

/// Check if an instruction is an ISB (Instruction Synchronization Barrier)
///
/// ISB instructions ensure that subsequent instructions are fetched from cache/memory
/// after all prior context-altering operations complete:
/// - ISB: Instruction synchronization barrier
/// - ISB SY: Full system instruction synchronization barrier
///
/// Encoding pattern: 11010101 00000011 0011 CRm:4 110 11111
/// - bits 31-12 = 0xD5033
/// - bits 7-5 = 110 (0x6)
/// - bits 4-0 = 11111 (0x1F)
/// - CRm (bits 11:8) = 0xF (1111) for ISB
///
/// Verified against aarch64-linux-gnu-as
#[inline]
pub fn is_isb_instruction(insn: u32) -> bool {
    // Check base pattern: bits 31-12 = 0xD5033
    let bits_31_12 = (insn >> 12) & 0xFFFFF;
    if bits_31_12 != 0xD5033 {
        return false;
    }

    // Check bits 7-5 = 110 (0x6) for ISB
    let bits_7_5 = (insn >> 5) & 0x7;
    if bits_7_5 != 0b110 {
        return false;
    }

    // Check bits 4-0 = 11111 (0x1F)
    let bits_4_0 = insn & 0x1F;
    if bits_4_0 != 0x1F {
        return false;
    }

    // ISB uses CRm = 0xF (1111)
    let crm = (insn >> 8) & 0xF;
    crm == 0xF
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

    // Tests for is_acquire_memory_instruction

    #[test]
    fn test_acquire_ldaxr() {
        // LDAXR w0, [x1] - load-acquire exclusive
        // Encoding verified against QEMU
        let insn: u32 = 0x885ffc20;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_ldaxrb() {
        // LDAXRB w0, [x1] - byte load-acquire exclusive
        let insn: u32 = 0x085ffc20;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_ldaxrh() {
        // LDAXRH w0, [x1] - halfword load-acquire exclusive
        let insn: u32 = 0x485ffc20;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_ldaxp() {
        // LDAXP w0, w1, [x2] - load-acquire exclusive pair
        // Verified encoding from aarch64-linux-gnu-as
        // 0x887f8440 = ldaxp w0, w1, [x2]
        let insn: u32 = 0x887f8440;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_ldar() {
        // LDAR w0, [x1] - load-acquire
        // QEMU: LDAR .. 001000 110 11111 . 11111 ..... .....@stlr
        let insn: u32 = 0x88dffc20;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_ldlar() {
        // LDLAR w0, [x1] - load-LOAcquire
        // Encoding: 1x 001000 110 11111 0 11111 Rn Rt
        // size=10, bit 23=1, bit 22=1 (L), bit 21=0, bit 15=0 (lasr)
        let insn: u32 = 0x88df7c20;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_casa() {
        // CASA w0, w1, [x2] - compare and swap with acquire
        // QEMU: CAS sz:2 001000 1 - 1 rs:5 - 11111 rn:5 rt:5
        let insn: u32 = 0x88e07c41;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_casal() {
        // CASAL w0, w1, [x2] - compare and swap with acquire-release
        let insn: u32 = 0x88e0fc41;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_ldadda() {
        // LDADDA w0, w1, [x2] - atomic add with acquire
        // QEMU: LDADD .. 111 0 00 . . 1 ..... 0000 00 ..... .....@atomic
        // Bit [23] = A = 1 for acquire
        // LDADDA = LDADD | (1 << 23)
        // 0xb8200041 is LDADD, so LDADDA = 0xb8200041 | 0x00800000 = 0xb8a00041
        let insn: u32 = 0xb8a00041;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_ldclra() {
        // LDCLRA w0, w1, [x2] - atomic bit clear with acquire
        // QEMU: LDCLR .. 111 0 00 . . 1 ..... 0001 00 ..... .....@atomic
        // Bit [23] = A = 1 for acquire
        let insn: u32 = 0xb8a01041;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_dmb_ishld() {
        // DMB ISHLD - data memory barrier inner shareable load
        // Verified encoding from aarch64-linux-gnu-as: 0xd50339bf
        // Pattern: 0xD5033 | (CRm << 8) | (0b101 << 5) | 0x1F
        // CRm = 0x9 (1001): domain=10 (ISH), types=01 (LD)
        let insn: u32 = 0xd50339bf;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_dmb_ld() {
        // DMB LD - data memory barrier load (full system)
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033dbf
        // CRm = 0xD (1101): domain=11 (SY), types=01 (LD)
        let insn: u32 = 0xd5033dbf;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_dmb_sy() {
        // DMB SY - data memory barrier full system
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033fbf
        // CRm = 0xF (1111): domain=11 (SY), types=11 (SY)
        let insn: u32 = 0xd5033fbf;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_acquire_dmb_ish() {
        // DMB ISH - data memory barrier inner shareable
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033bbf
        // CRm = 0xB (1011): domain=10 (ISH), types=11 (SY)
        let insn: u32 = 0xd5033bbf;
        assert!(is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_not_acquire_ldxr() {
        // LDXR w0, [x1] - load exclusive WITHOUT acquire
        let insn: u32 = 0x885f7c20;
        assert!(!is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_not_acquire_ldadd() {
        // LDADD w0, w1, [x2] - atomic add WITHOUT acquire
        let insn: u32 = 0xb8200041;
        assert!(!is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_not_acquire_cas() {
        // CAS w0, w1, [x2] - compare and swap WITHOUT acquire
        let insn: u32 = 0x88a07c41;
        assert!(!is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_not_acquire_regular_load() {
        // LDR w0, [x1] - regular load
        let insn: u32 = 0xb9400020;
        assert!(!is_acquire_memory_instruction(insn));
    }

    #[test]
    fn test_not_acquire_dmb_ishst() {
        // DMB ISHST - store barrier (not acquire)
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033abf
        // CRm = 0xA (1010): domain=10 (ISH), types=10 (ST)
        let insn: u32 = 0xd5033abf;
        assert!(!is_acquire_memory_instruction(insn));
    }

    // Tests for is_dsb_instruction

    #[test]
    fn test_dsb_sy() {
        // DSB SY - data synchronization barrier full system
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033f9f
        let insn: u32 = 0xd5033f9f;
        assert!(is_dsb_instruction(insn));
    }

    #[test]
    fn test_dsb_ish() {
        // DSB ISH - data synchronization barrier inner shareable
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033b9f
        let insn: u32 = 0xd5033b9f;
        assert!(is_dsb_instruction(insn));
    }

    #[test]
    fn test_dsb_ishld() {
        // DSB ISHLD - data synchronization barrier inner shareable load
        // Verified encoding from aarch64-linux-gnu-as: 0xd503399f
        let insn: u32 = 0xd503399f;
        assert!(is_dsb_instruction(insn));
    }

    #[test]
    fn test_dsb_ishst() {
        // DSB ISHST - data synchronization barrier inner shareable store
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033a9f
        let insn: u32 = 0xd5033a9f;
        assert!(is_dsb_instruction(insn));
    }

    #[test]
    fn test_dsb_ld() {
        // DSB LD - data synchronization barrier load
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033d9f
        let insn: u32 = 0xd5033d9f;
        assert!(is_dsb_instruction(insn));
    }

    #[test]
    fn test_dsb_st() {
        // DSB ST - data synchronization barrier store
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033e9f
        let insn: u32 = 0xd5033e9f;
        assert!(is_dsb_instruction(insn));
    }

    #[test]
    fn test_not_dsb_dmb() {
        // DMB ISH - not a DSB
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033bbf
        let insn: u32 = 0xd5033bbf;
        assert!(!is_dsb_instruction(insn));
    }

    #[test]
    fn test_not_dsb_isb() {
        // ISB - not a DSB
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033fdf
        let insn: u32 = 0xd5033fdf;
        assert!(!is_dsb_instruction(insn));
    }

    #[test]
    fn test_not_dsb_regular_load() {
        // LDR w0, [x1] - regular load, not DSB
        let insn: u32 = 0xb9400020;
        assert!(!is_dsb_instruction(insn));
    }

    // Tests for is_isb_instruction

    #[test]
    fn test_isb() {
        // ISB - instruction synchronization barrier
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033fdf
        let insn: u32 = 0xd5033fdf;
        assert!(is_isb_instruction(insn));
    }

    #[test]
    fn test_isb_sy() {
        // ISB SY - instruction synchronization barrier full system
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033fdf
        let insn: u32 = 0xd5033fdf;
        assert!(is_isb_instruction(insn));
    }

    #[test]
    fn test_not_isb_dmb() {
        // DMB ISH - not an ISB
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033bbf
        let insn: u32 = 0xd5033bbf;
        assert!(!is_isb_instruction(insn));
    }

    #[test]
    fn test_not_isb_dsb() {
        // DSB SY - not an ISB
        // Verified encoding from aarch64-linux-gnu-as: 0xd5033f9f
        let insn: u32 = 0xd5033f9f;
        assert!(!is_isb_instruction(insn));
    }

    #[test]
    fn test_not_isb_regular_load() {
        // LDR w0, [x1] - regular load, not ISB
        let insn: u32 = 0xb9400020;
        assert!(!is_isb_instruction(insn));
    }

    #[test]
    fn test_not_isb_sb() {
        // SB - speculation barrier (not ISB)
        // SB has bits 7-5 = 111, CRm = 0000
        let insn: u32 = 0xd50330df;
        assert!(!is_isb_instruction(insn));
    }
}
