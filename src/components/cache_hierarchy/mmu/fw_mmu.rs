use std::ffi::c_void;

use serde::{Deserialize, Serialize};

use crate::{
    arch::{self, aarch64::ptw}, parameter, qemu_api
};

use rustc_hash::FxHashMap as HashMap;

use super::{
    AbstractMMU, MMUTranslationResult,
    tlb::{self, AddressSpaceID, FullyAssociativeTLB, TLB},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(align(64))]
pub struct FunctionalWarmingMMU<
    ARCH: arch::ISA,
    const I_T_ASSO: usize = 64,
    const D_T_ASSO: usize = 64,
    const S_T_ASSO: usize = 16,
    const S_T_SETS: usize = 64,
    const NO_HUGE_PAGE: bool = false,
> {
    l0_itlb: (u64, AddressSpaceID, u64),
    stlb: TLB<S_T_SETS, S_T_ASSO>,
    itlb: FullyAssociativeTLB,
    dtlb: FullyAssociativeTLB,
    htbl_2m: HashMap<u64, (AddressSpaceID, u64)>,
    htbl_1g: HashMap<u64, (AddressSpaceID, u64)>,
    last_ttbr: u64,
    arch: std::marker::PhantomData<ARCH>,

    insertion_since_last_igc: usize,
    insertion_since_last_dgc: usize,
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
    const D_T_A: usize,
    const S_T_A: usize,
    const S_T_S: usize,
    const NO_HUGE_PAGE: bool,
> FunctionalWarmingMMU<arch::AArch64, I_T_A, D_T_A, S_T_A, S_T_S, NO_HUGE_PAGE>
{
    fn refill_4k_tlb(
        &mut self,
        vpn: u64,
        asid: AddressSpaceID,
        ppn: u64,
        ts: u64,
        is_instruction: bool,
    ) {
        self.stlb.insert(vpn, asid, ppn, ts, is_instruction);

        if is_instruction {
            self.itlb.insert(vpn, asid, ts, ppn);
            self.insertion_since_last_igc += 1;
            if self.insertion_since_last_igc == 512 {
                self.itlb.collect_garbage();
                self.insertion_since_last_igc = 0;
            }

            // Set the L0 ITLB.
            self.l0_itlb = (vpn, asid, ppn);
        } else {
            self.dtlb.insert(vpn, asid, ts, ppn);
            self.insertion_since_last_dgc += 1;
            if self.insertion_since_last_dgc == 512 {
                self.dtlb.collect_garbage();
                self.insertion_since_last_dgc = 0;
            }
        }
    }

    #[allow(dead_code)]
    fn page_walk(&self, va: u64, tcr: u64) -> u64 {
        let ptw_result = {
            let t1_size = 64 - ((tcr >> 16) & 0b111111);
            let t0_size = 64 - (tcr & 0b111111);

            assert!(t0_size == 48); // the lower 48-bit VA are used for translation.
            assert!(t1_size == 48); // the OS should take over all spaces.

            let which_ttbr_for_base = if va < (1 << 48) { 0 } else { 1 };
            let ttbr = unsafe { qemu_api::qemu_plugin_read_ttbr_el1(which_ttbr_for_base) };
            let vpn = va >> 12;
            ptw(ttbr, tcr, vpn << 12, paddr_reader)
        };

        let ppn = ptw_result.paddr >> 12;

        ppn << 12 | (va & 0xfff)
    }
}

impl<
    const I_T_A: usize,
    const D_T_A: usize,
    const S_T_A: usize,
    const S_T_S: usize,
    const NO_HUGE_PAGE: bool,
> AbstractMMU for FunctionalWarmingMMU<arch::AArch64, I_T_A, D_T_A, S_T_A, S_T_S, NO_HUGE_PAGE>
{
    fn new() -> Self {
        Self {
            l0_itlb: (0, AddressSpaceID::NonGlobal(0), 0),
            stlb: TLB::new(),
            itlb: FullyAssociativeTLB::new(I_T_A),
            dtlb: FullyAssociativeTLB::new(D_T_A),
            htbl_2m: HashMap::default(),
            htbl_1g: HashMap::default(),
            last_ttbr: u64::MAX,
            arch: std::marker::PhantomData,
            insertion_since_last_igc: 0,
            insertion_since_last_dgc: 0,
        }
    }

    fn translate_and_refill(
        &mut self,
        _core_id: u32,
        va: u64,
        ts: u64,
        is_instruction: bool,
    ) -> MMUTranslationResult {
        let tcr = unsafe { qemu_api::qemu_plugin_read_tcr_el1() };
        // How do we decide whether this is a kernel space or a user space?
        // I have to read the two granules.
        let which_ttbr_for_asid = if tcr >> 22 & 0b1 == 1 { 1 } else { 0 };

        let vpn = va >> 12;
        let raw_asid =
            unsafe { (qemu_api::qemu_plugin_read_ttbr_el1(which_ttbr_for_asid) >> 48) as u16 };

        let asid = tlb::AddressSpaceID::NonGlobal(raw_asid); // We will start with a non-global ASID. It can still match the global ASID.

        if is_instruction {
            // check the L0 ITLB.
            if self.l0_itlb.0 == vpn && self.l0_itlb.1 == asid {
                if parameter::COMPARE_TRANSLATION_RESULT_WITH_WALKER {
                    let walker_pa = self.page_walk(va, tcr);
                    let pa = self.l0_itlb.2 << 12 | (va & 0xfff);
                    assert_eq!(pa, walker_pa);
                }

                return MMUTranslationResult::Hit(self.l0_itlb.2);
            }
        }

        // First, we check the L2 TLB.
        if let Some(ppn) = self.stlb.peek(vpn, asid) {
            if is_instruction {
                self.itlb.deferred_insert(vpn, asid, ts, ppn);
            } else {
                self.dtlb.deferred_insert(vpn, asid, ts, ppn);
            }

            if parameter::COMPARE_TRANSLATION_RESULT_WITH_WALKER {
                let walker_pa = self.page_walk(va, tcr);
                let pa = ppn << 12 | (va & 0xfff);
                assert_eq!(pa, walker_pa);
            }

            return MMUTranslationResult::Hit(ppn);
        }

        // Alrignt. Then we have to check the L1 TLB, which has higher associativity.
        if is_instruction {
            self.itlb.handle_deferred_insertion();
            if let Some(ppn) = self.itlb.lookup(vpn, raw_asid as u16, ts) {
                if parameter::COMPARE_TRANSLATION_RESULT_WITH_WALKER {
                    let walker_pa = self.page_walk(va, tcr);
                    let pa = ppn << 12 | (va & 0xfff);
                    assert_eq!(pa, walker_pa);
                }

                return MMUTranslationResult::Hit(ppn);
            }
        } else {
            self.dtlb.handle_deferred_insertion();
            if let Some(ppn) = self.dtlb.lookup(vpn, raw_asid as u16, ts) {
                if parameter::COMPARE_TRANSLATION_RESULT_WITH_WALKER {
                    let walker_pa = self.page_walk(va, tcr);
                    let pa = ppn << 12 | (va & 0xfff);
                    assert_eq!(pa, walker_pa);
                }
                
                return MMUTranslationResult::Hit(ppn);
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

        // based on the ptw_result, we refill each TLB correspondingly.
        self.refill_4k_tlb(vpn, asid, ptw_result.paddr >> 12, ts, is_instruction);

        if ptw_result.cacheable {
            MMUTranslationResult::Miss(ptw_result.paddr, ptw_result.traces)
        } else {
            MMUTranslationResult::MissNotCacheable(ptw_result.paddr)
        }
    }

    fn lookup(&mut self, _vpn: u64, _ts: u64, _is_instruction: bool) -> Option<u64> {
        todo!()
    }

    fn flush(&mut self, mode: super::MMUFlushMode) {
        self.itlb.flush(mode);
        self.dtlb.flush(mode);
        self.stlb.flush(mode);
    }

    fn serialize(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }

    fn deserialize(&mut self, value: serde_json::Value) {
        *self = serde_json::from_value(value).unwrap();
    }
}
