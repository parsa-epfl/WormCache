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

// Desc: Memory Management Unit
// This file is highly related to the ISA.

pub mod tlb;

use crate::arch::aarch64::ptw;
use crate::components::debug::statistics::EventType;
use crate::qemu_api;
use crate::{arch, components::debug::statistics::Statistics};

use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use tlb::{AddressSpaceID, TLB};

pub trait AbstractMMU {
    fn new() -> Self;
    fn translate_and_refill(
        &mut self,
        core_id: u32,
        va: u64,
        ts: u64,
        is_instruction: bool,
    ) -> MMUTranslationResult;

    // Currently, this interface is for debugging. It reuses QEMU's PTW result.
    fn lookup(&mut self, vpn: u64, ts: u64, is_instruction: bool) -> Option<u64>;

    fn serialize(&self) -> serde_json::Value;
    fn deserialize(&mut self, value: serde_json::Value);
}

pub struct NoMMU {}

impl AbstractMMU for NoMMU {
    fn new() -> Self {
        Self {}
    }
    fn translate_and_refill(
        &mut self,
        _core_id: u32,
        va: u64,
        _: u64,
        _: bool,
    ) -> MMUTranslationResult {
        MMUTranslationResult::Hit(va)
    }
    fn lookup(&mut self, _: u64, _: u64, _: bool) -> Option<u64> {
        None
    }

    fn serialize(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn deserialize(&mut self, _: serde_json::Value) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(align(64))]
pub struct MemoryManagementUnit<
    ARCH: arch::ISA,
    const I_T_ASSO: usize = 64,
    const I_T_SETS: usize = 1,
    const D_T_ASSO: usize = 64,
    const D_T_SETS: usize = 1,
    const S_TLB_ENABLED: bool = true,
    const S_T_ASSO: usize = 16,
    const S_T_SETS: usize = 1024,
    const NO_HUGE_PAGE: bool = false,
> {
    itlb: TLB<I_T_SETS, I_T_ASSO>,
    dtlb: TLB<D_T_SETS, D_T_ASSO>,
    stlb: TLB<S_T_SETS, S_T_ASSO>,
    htbl_2mb: HashMap<u64, (AddressSpaceID, u64)>, // Currently, we just use a simple hashmap to store the 2MB page table.
    htlb_1gb: HashMap<u64, (AddressSpaceID, u64)>, // Same to the 2MB page table.
    last_ttbr: u64,
    arch: std::marker::PhantomData<ARCH>,
    // other MMU caches can be also added here as well.
}

pub enum MMUTranslationResult {
    Hit(u64),              // PPN
    Miss(u64, [u64; 4]),   // PPN, walk traces
    MissNotCacheable(u64), // PPN
}

fn paddr_reader(addr: u64) -> u64 {
    // make addr aligned with 8.
    let addr = addr & !0b111;
    let mut buf: u64 = 0;
    unsafe {
        qemu_api::qemu_plugin_read_physical_memory(addr, 8, &mut buf as *mut u64 as *mut c_void);
    }
    buf
}

impl<
        const I_T_A: usize,
        const I_T_S: usize,
        const D_T_A: usize,
        const D_T_S: usize,
        const S_ENABLED: bool,
        const S_T_A: usize,
        const S_T_S: usize,
        const NO_HUGE_PAGE: bool,
    >
    MemoryManagementUnit<
        arch::AArch64,
        I_T_A,
        I_T_S,
        D_T_A,
        D_T_S,
        S_ENABLED,
        S_T_A,
        S_T_S,
        NO_HUGE_PAGE,
    >
{
    fn refill_4k_tlb(
        &mut self,
        vpn: u64,
        asid: AddressSpaceID,
        ppn: u64,
        ts: u64,
        is_instruction: bool,
    ) {
        if is_instruction {
            self.itlb.insert(vpn, asid.clone(), ppn, ts, is_instruction);
        } else {
            self.dtlb.insert(vpn, asid.clone(), ppn, ts, is_instruction);
        }

        if S_ENABLED {
            self.stlb.insert(vpn, asid, ppn, ts, is_instruction);
        }
    }
}

impl<
        const I_T_A: usize,
        const I_T_S: usize,
        const D_T_A: usize,
        const D_T_S: usize,
        const S_ENABLED: bool,
        const S_T_A: usize,
        const S_T_S: usize,
        const NO_HUGE_PAGE: bool,
    > AbstractMMU
    for MemoryManagementUnit<
        arch::AArch64,
        I_T_A,
        I_T_S,
        D_T_A,
        D_T_S,
        S_ENABLED,
        S_T_A,
        S_T_S,
        NO_HUGE_PAGE,
    >
{
    fn new() -> Self {
        Self {
            itlb: TLB::new(),
            dtlb: TLB::new(),
            stlb: TLB::new(),
            htbl_2mb: HashMap::default(),
            htlb_1gb: HashMap::default(),
            last_ttbr: u64::MAX, // This is special for kernel instruction space.
            arch: std::marker::PhantomData,
        }
    }

    fn translate_and_refill(
        &mut self,
        core_id: u32,
        va: u64,
        ts: u64,
        is_instruction: bool,
    ) -> MMUTranslationResult {
        let tcr = unsafe { qemu_api::qemu_plugin_read_tcr_el1() };
        // How do we decide whether this is a kernel space or a user space?
        // I have to read the two granules.
        let which_ttbr_for_asid = if tcr >> 22 & 0b1 == 1 { 1 } else { 0 };

        // First, we try 4KB page.
        let vpn = va >> 12;
        let asid = tlb::AddressSpaceID::NonGlobal(unsafe {
            (qemu_api::qemu_plugin_read_ttbr_el1(which_ttbr_for_asid) >> 48) as u16
        }); // We will start with a non-global ASID. It can still match the global ASID.

        let is_kernel = (vpn >> 51) == 1;

        Statistics::global_record(core_id, EventType::TLBAccess, is_kernel);

        if is_instruction {
            Statistics::global_record(core_id, EventType::TLBAccessDueToInstruction, is_kernel);
        } else {
            Statistics::global_record(core_id, EventType::TLBAccessDueToData, is_kernel);
        }

        // First, check the L1 TLB.
        if is_instruction {
            if let Some(ppn) = self.itlb.lookup(vpn, asid, ts, is_instruction) {
                let pa = ppn << 12 | (va & 0xfff);
                return MMUTranslationResult::Hit(pa);
            }
        } else {
            if let Some(ppn) = self.dtlb.lookup(vpn, asid, ts, is_instruction) {
                let pa = ppn << 12 | (va & 0xfff);
                return MMUTranslationResult::Hit(pa);
            }
        }

        if S_ENABLED {
            // Then, we try L2 TLB.
            if let Some(ppn) = self.stlb.lookup(vpn, asid, ts, is_instruction) {
                let pa = ppn << 12 | (va & 0xfff);
                return MMUTranslationResult::Hit(pa);
            }
        }

        if !NO_HUGE_PAGE {
            // Then, we try 2MB page.
            let vpn_2mb = vpn >> 9;
            if let Some(ppn) = self.htbl_2mb.get(&vpn_2mb) {
                if ppn.0.check(&asid) {
                    let pa = ppn.1 << 21 | (va & 0x1fffff);

                    Statistics::global_record(core_id, EventType::HugeTLBHit, is_kernel);

                    if is_instruction {
                        Statistics::global_record(
                            core_id,
                            EventType::HugeTLBHitDueToInstruction,
                            is_kernel,
                        );
                    } else {
                        Statistics::global_record(
                            core_id,
                            EventType::HugeTLBHitDueToData,
                            is_kernel,
                        );
                    }

                    return MMUTranslationResult::Hit(pa);
                }
            }

            // Then, we try 1GB page.
            let vpn_1gb = vpn >> 18;
            if let Some(ppn) = self.htlb_1gb.get(&vpn_1gb) {
                if ppn.0.check(&asid) {
                    Statistics::global_record(core_id, EventType::HugeTLBHit, is_kernel);

                    if is_instruction {
                        Statistics::global_record(
                            core_id,
                            EventType::HugeTLBHitDueToInstruction,
                            is_kernel,
                        );
                    } else {
                        Statistics::global_record(
                            core_id,
                            EventType::HugeTLBHitDueToData,
                            is_kernel,
                        );
                    }

                    let pa = ppn.1 << 30 | (va & 0x3fffffff);
                    return MMUTranslationResult::Hit(pa);
                }
            }

            let ptw_result = {
                let t1_size = 64 - ((tcr >> 16) & 0b111111);
                let t0_size = 64 - (tcr & 0b111111);

                assert!(t0_size == 48); // the lower 48-bit VA are used for translation.
                assert!(t1_size == 48); // the OS should take over all spaces.

                let which_ttbr_for_base = if va < (1 << 48) { 0 } else { 1 };
                let ttbr = unsafe { qemu_api::qemu_plugin_read_ttbr_el1(which_ttbr_for_base) };
                ptw(ttbr, tcr, vpn << 12, paddr_reader)
            };

            let asid = if ptw_result.is_global {
                AddressSpaceID::Global
            } else {
                asid
            };

            if is_kernel {
                // kernel space has to be global address.
                assert!(matches!(asid, AddressSpaceID::Global));
            }

            // based on the ptw_result, we refill each TLB correspondingly.
            match ptw_result.page_size {
                arch::aarch64::PageSize::_4KB => {
                    self.refill_4k_tlb(vpn, asid, ptw_result.paddr >> 12, ts, is_instruction);
                }
                arch::aarch64::PageSize::_2MB => {
                    self.htbl_2mb
                        .insert(vpn_2mb, (asid, ptw_result.paddr >> 21));
                }
                arch::aarch64::PageSize::_1GB => {
                    self.htlb_1gb
                        .insert(vpn_1gb, (asid, ptw_result.paddr >> 30));
                }
            }

            if ptw_result.cacheable {
                MMUTranslationResult::Miss(ptw_result.paddr, ptw_result.traces)
            } else {
                MMUTranslationResult::MissNotCacheable(ptw_result.paddr)
            }
        } else {
            let ptw_result = {
                let t1_size = 64 - ((tcr >> 16) & 0b111111);
                let t0_size = 64 - (tcr & 0b111111);

                assert!(t0_size == 48); // the lower 48-bit VA are used for translation.
                assert!(t1_size == 48); // the OS should take over all spaces.

                let which_ttbr_for_base = if va < (1 << 48) { 0 } else { 1 };
                let ttbr = unsafe { qemu_api::qemu_plugin_read_ttbr_el1(which_ttbr_for_base) };
                ptw(ttbr, tcr, vpn << 12, paddr_reader)
            };

            let asid = if ptw_result.is_global {
                AddressSpaceID::Global
            } else {
                asid.clone()
            };

            if is_kernel {
                // kernel space has to be global address.
                assert!(matches!(asid, AddressSpaceID::Global));
            }

            // based on the ptw_result, we refill each TLB correspondingly.
            self.refill_4k_tlb(vpn, asid, ptw_result.paddr >> 12, ts, is_instruction);

            if ptw_result.cacheable {
                MMUTranslationResult::Miss(ptw_result.paddr, ptw_result.traces)
            } else {
                MMUTranslationResult::MissNotCacheable(ptw_result.paddr)
            }
        }
    }

    fn lookup(&mut self, vpn: u64, ts: u64, is_instruction: bool) -> Option<u64> {
        let tcr = unsafe { qemu_api::qemu_plugin_read_tcr_el1() };
        // How do we decide whether this is a kernel space or a user space?
        // I have to read the two granules.
        let which_ttbr_for_asid = if tcr >> 22 & 0b1 == 1 { 1 } else { 0 };
        let asid = tlb::AddressSpaceID::NonGlobal(unsafe {
            (qemu_api::qemu_plugin_read_ttbr_el1(which_ttbr_for_asid) >> 48) as u16
        }); // We will start with a non-global ASID. It can still match the global ASID.

        self.stlb.lookup(vpn, asid, ts, is_instruction)
    }

    fn serialize(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }

    fn deserialize(&mut self, value: serde_json::Value) {
        *self = serde_json::from_value(value).unwrap();
    }
}
