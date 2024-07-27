// Desc: Memory Management Unit
// This file is highly related to the ISA.

mod tlb;

use crate::arch;
use crate::arch::aarch64::ptw;
use crate::qemu_api;

use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use std::{ffi::c_void, io::Write};
use tlb::TLB;

pub trait AbstractMMU {
    fn new() -> Self;
    fn translate_and_refill(&mut self, va: u64, ts: u64, is_instruction: bool) -> MMUTranslationResult;

    // Currently, this interface is for debugging. It reuses QEMU's PTW result.
    fn refill_4k_tlb(&mut self, vpn: u64, ppn: u64, ts: u64, is_instruction: bool);
    fn lookup(&mut self, vpn: u64, ts: u64, is_instruction: bool) -> Option<u64>;

    fn serialize(&self) -> serde_json::Value;
    fn deserialize(&mut self, value: serde_json::Value);

    fn dump_flexus_checkpoint(&self, filename: &str, suffix: &str);
}

pub struct NoMMU {}

impl AbstractMMU for NoMMU {
    fn new() -> Self {
        Self {}
    }
    fn translate_and_refill(&mut self, va: u64, _: u64, _: bool) -> MMUTranslationResult {
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

    fn dump_flexus_checkpoint(&self, _: &str, _: &str) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(align(64))]
pub struct MemoryManagementUnit<
    ARCH: arch::ISA,
    const T_ASSO: usize = 16,
    const T_SETS: usize = 1024,
> {
    tlb: TLB<T_SETS, T_ASSO>,
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

impl<const T_A: usize, const T_S: usize> AbstractMMU
    for MemoryManagementUnit<arch::AArch64, T_A, T_S>
{
    fn new() -> Self {
        Self {
            tlb: TLB::new(),
            htbl_2mb: HashMap::default(),
            htlb_1gb: HashMap::default(),
            last_ttbr: u64::MAX, // This is special for kernel instruction space.
            arch: std::marker::PhantomData,
        }
    }

    fn translate_and_refill(&mut self, va: u64, ts: u64, is_instruction: bool) -> MMUTranslationResult {
        let is_kernel = (va >> 63) != 0;

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

        // First, we try 4KB page.
        let vpn = va >> 12;

        if let Some(ppn) = self.tlb.lookup(vpn, asid, ts, is_instruction) {
            let pa = ppn << 12 | (va & 0xfff);
            return MMUTranslationResult::Hit(pa);
        }

        // Then, we try 2MB page.
        let vpn_2mb = vpn >> 9;
        let key_2mb = (asid as u64) << 48 | vpn_2mb;
        if let Some(ppn) = self.htbl_2mb.get(&key_2mb) {
            let pa = ppn << 21 | (va & 0x1fffff);
            return MMUTranslationResult::Hit(pa);
        }

        // Then, we try 1GB page.
        let vpn_1gb = vpn >> 18;
        let key_1gb = (asid as u64) << 48 | vpn_1gb;
        if let Some(ppn) = self.htlb_1gb.get(&key_1gb) {
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
            arch::aarch64::PageSize::_4KB => self.tlb.insert(vpn, asid, ptw_result.paddr >> 12, ts, is_instruction),
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

        self.tlb.insert(vpn, asid, ppn, ts, is_instruction)
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

        self.tlb.lookup(vpn, asid, ts, is_instruction)
    }

    fn serialize(&self) -> serde_json::Value {
        return serde_json::to_value(self).unwrap();
    }

    fn deserialize(&mut self, value: serde_json::Value) {
        *self = serde_json::from_value(value).unwrap();
    }
    
    fn dump_flexus_checkpoint(&self, folder_name: &str, suffix: &str) {
        // there are two data structures to dump: iTLB and dTLB. 
        const FLEXUS_ITLB_CAPACITY: usize = 64;
        const FLEXUS_DTLB_CAPACITY: usize = 64;

        let to_dump = self.tlb.get_flexus_checkpoint(
            FLEXUS_ITLB_CAPACITY, 
            FLEXUS_DTLB_CAPACITY
        );

        let mut file = std::fs::File::create(format!("{}/itlb_{}.json", folder_name, suffix)).unwrap();
        file.write_all(serde_json::to_string(&to_dump[0]).unwrap().as_bytes()).unwrap();

        let mut file = std::fs::File::create(format!("{}/dtlb_{}.json", folder_name, suffix)).unwrap();
        file.write_all(serde_json::to_string(&to_dump[1]).unwrap().as_bytes()).unwrap();
    }

    
}
