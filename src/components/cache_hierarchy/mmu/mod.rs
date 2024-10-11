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
use crate::qemu_api;
use crate::arch;

use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use tlb::TLB;

use super::debug::statistics::{EventType, Statistics};

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
    fn refill_4k_tlb(&mut self, vpn: u64, ppn: u64, ts: u64, is_instruction: bool);
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
    fn refill_4k_tlb(&mut self, _: u64, _: u64, _: u64, _: bool) {}
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
    htbl_2mb: HashMap<u64, u64>, // Currently, we just use a simple hashmap to store the 2MB page table.
    htlb_1gb: HashMap<u64, u64>, // Same to the 2MB page table.
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
        let is_kernel = (va >> 63) != 0;

        if is_kernel {
            self.last_ttbr = u64::MAX;
        } else if self.last_ttbr == u64::MAX {
            self.last_ttbr = unsafe { qemu_api::qemu_plugin_read_ttbr_el1(0) };
        }

        let asid = if is_kernel {
            0xffff_u16
        } else {
            // TODO: The asid may come from a different register. This requires further investigation.
            (self.last_ttbr >> 48) as u16
        };

        // First, we try 4KB page.
        let vpn = va >> 12;

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
            let key_2mb = (asid as u64) << 48 | vpn_2mb;
            if let Some(ppn) = self.htbl_2mb.get(&key_2mb) {
                let pa = ppn << 21 | (va & 0x1fffff);

                Statistics::global_record(core_id, EventType::HugeTLBHit, is_kernel);

                if is_instruction {
                    Statistics::global_record(
                        core_id,
                        EventType::HugeTLBHitDueToInstruction,
                        is_kernel,
                    );
                } else {
                    Statistics::global_record(core_id, EventType::HugeTLBHitDueToData, is_kernel);
                }

                return MMUTranslationResult::Hit(pa);
            }

            // Then, we try 1GB page.
            let vpn_1gb = vpn >> 18;
            let key_1gb = (asid as u64) << 48 | vpn_1gb;
            if let Some(ppn) = self.htlb_1gb.get(&key_1gb) {
                Statistics::global_record(core_id, EventType::HugeTLBHit, is_kernel);

                if is_instruction {
                    Statistics::global_record(
                        core_id,
                        EventType::HugeTLBHitDueToInstruction,
                        is_kernel,
                    );
                } else {
                    Statistics::global_record(core_id, EventType::HugeTLBHitDueToData, is_kernel);
                }

                let pa = ppn << 30 | (va & 0x3fffffff);
                return MMUTranslationResult::Hit(pa);
            }

            let ptw_result = unsafe {
                let tcr = qemu_api::qemu_plugin_read_tcr_el1();
                let ttbr = qemu_api::qemu_plugin_read_ttbr_el1(if is_kernel { 1 } else { 0 });
                ptw(ttbr, tcr, vpn << 12, paddr_reader)
            };

            // based on the ptw_result, we refill each TLB correspondingly.
            match ptw_result.page_size {
                arch::aarch64::PageSize::_4KB => {
                    self.refill_4k_tlb(vpn, ptw_result.paddr >> 12, ts, is_instruction);
                }
                arch::aarch64::PageSize::_2MB => {
                    self.htbl_2mb.insert(key_2mb, ptw_result.paddr >> 21);
                }
                arch::aarch64::PageSize::_1GB => {
                    self.htlb_1gb.insert(key_1gb, ptw_result.paddr >> 30);
                }
            }

            if ptw_result.cacheable {
                MMUTranslationResult::Miss(ptw_result.paddr, ptw_result.traces)
            } else {
                MMUTranslationResult::MissNotCacheable(ptw_result.paddr)
            }
        } else {
            let ptw_result = unsafe {
                let tcr = qemu_api::qemu_plugin_read_tcr_el1();
                let ttbr = qemu_api::qemu_plugin_read_ttbr_el1(if is_kernel { 1 } else { 0 });
                ptw(ttbr, tcr, vpn << 12, paddr_reader)
            };

            // based on the ptw_result, we refill each TLB correspondingly.
            self.refill_4k_tlb(vpn, ptw_result.paddr >> 12, ts, is_instruction);

            if ptw_result.cacheable {
                MMUTranslationResult::Miss(ptw_result.paddr, ptw_result.traces)
            } else {
                MMUTranslationResult::MissNotCacheable(ptw_result.paddr)
            }
        }
    }

    fn refill_4k_tlb(&mut self, vpn: u64, ppn: u64, ts: u64, is_instruction: bool) {
        let is_kernel = (vpn >> 51) == 1;

        if is_kernel {
            self.last_ttbr = u64::MAX;
        } else if self.last_ttbr == u64::MAX {
            self.last_ttbr = unsafe { qemu_api::qemu_plugin_read_ttbr_el1(0) };
        }

        let asid = if is_kernel {
            0xffff_u16
        } else {
            (self.last_ttbr >> 48) as u16
        };

        if is_instruction {
            self.itlb
                .insert(vpn, asid, ppn, ts, is_instruction);
        } else {
            self.dtlb
                .insert(vpn, asid, ppn, ts, is_instruction);
        }

        if S_ENABLED {
            self.stlb
                .insert(vpn, asid, ppn, ts, is_instruction);
        }
    }

    fn lookup(&mut self, vpn: u64, ts: u64, is_instruction: bool) -> Option<u64> {
        let is_kernel = (vpn >> 51) == 1;

        if is_kernel {
            self.last_ttbr = u64::MAX;
        } else if self.last_ttbr == u64::MAX {
            self.last_ttbr = unsafe { qemu_api::qemu_plugin_read_ttbr_el1(0) };
        }

        let asid = if is_kernel {
            0xffff_u16
        } else {
            (self.last_ttbr >> 48) as u16
        };

        self.stlb.lookup(vpn, asid, ts, is_instruction)
    }

    fn serialize(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }

    fn deserialize(&mut self, value: serde_json::Value) {
        *self = serde_json::from_value(value).unwrap();
    }
}
