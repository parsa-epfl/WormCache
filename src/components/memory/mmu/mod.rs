// Desc: Memory Management Unit
// This file is highly related to the ISA.

mod tlb;

use crate::arch;
use crate::arch::aarch64::ptw;
use crate::qemu_api;

use std::ffi::c_void;
use tlb::TLB;

pub trait AbstractMMU {
    fn new() -> Self;
    fn translate_and_refill(&mut self, vpn: u64, ts: u64) -> MMUTranslationResult;
}

pub struct NoMMU {}

impl AbstractMMU for NoMMU {
    fn new() -> Self {
        Self {}
    }
    fn translate_and_refill(&mut self, vpn: u64, ts: u64) -> MMUTranslationResult {
        MMUTranslationResult::Hit(vpn)
    }
}

#[derive(Debug)]
pub struct MemoryManagementUnit<
    ARCH: arch::ISA,
    const T_ASSO: usize = 16,
    const T_SETS: usize = 1024,
> {
    tlb: TLB<T_ASSO, T_SETS>,
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
    return buf;
}

impl<const T_A: usize, const T_S: usize> AbstractMMU for MemoryManagementUnit<arch::AArch64, T_A, T_S> {
    fn new() -> Self {
        Self {
            tlb: TLB::new(),
            last_ttbr: u64::MAX, // This is special for kernel instruction space.
            arch: std::marker::PhantomData,
        }
    }

    fn translate_and_refill(&mut self, vpn: u64, ts: u64) -> MMUTranslationResult {
        let is_kernel = (vpn >> 51) == 1;

        if is_kernel {
            self.last_ttbr = u64::MAX;
        } else if self.last_ttbr == u64::MAX {
            self.last_ttbr = unsafe { qemu_api::qemu_plugin_read_ttbr_el1(0) };
        }

        let asid = if is_kernel {
            0xffff as u16
        } else {
            (self.last_ttbr >> 48) as u16
        };

        if let Some(ppn) = self.tlb.lookup(vpn, asid, ts) {
            return MMUTranslationResult::Hit(ppn);
        }

        let ptw_result = unsafe {
            let tcr = qemu_api::qemu_plugin_read_tcr_el1();
            let ttbr = qemu_api::qemu_plugin_read_ttbr_el1(if is_kernel { 0 } else { 1 });
            ptw(ttbr, tcr, vpn << 12, paddr_reader)
        };

        self.tlb.insert(vpn, asid, ptw_result.paddr >> 12, ts);

        if ptw_result.cacheable {
            return MMUTranslationResult::Miss(ptw_result.paddr >> 12, ptw_result.traces);
        } else {
            return MMUTranslationResult::MissNotCacheable(ptw_result.paddr >> 12);
        }
    }
}
