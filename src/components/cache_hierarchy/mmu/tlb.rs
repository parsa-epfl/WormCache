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

use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum AddressSpaceID {
    Global,
    NonGlobal(u16),
}

impl AddressSpaceID {
    #[inline]
    pub fn check(&self, other: &AddressSpaceID) -> bool {
        match self {
            // The hit condition is calculated from the following rule:
            // - If the entry is global, it is a hit.
            // - If the entry is not global, it is a hit if the ASID matches.
            AddressSpaceID::Global => true,
            AddressSpaceID::NonGlobal(_) => self == other,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TLBEntry {
    pub valid: bool,
    pub ts: u64,
    pub asid: AddressSpaceID,
    pub vpn: u64,
    pub ppn: u64,
    pub is_instruction: bool,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
struct TLBSet<const ASSO: usize> {
    #[serde_as(as = "[_; ASSO]")]
    entries: [TLBEntry; ASSO],
    current_pointer: usize,
}

impl<const ASSO: usize> TLBSet<ASSO> {
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| TLBEntry {
                valid: false,
                ts: 0,
                asid: AddressSpaceID::Global,
                vpn: 0,
                ppn: 0,
                is_instruction: false,
            }),
            current_pointer: 0,
        }
    }

    pub fn lookup(
        &mut self,
        vpn: u64,
        asid: AddressSpaceID,
        ts: u64,
        is_instruction: bool,
    ) -> Option<u64> {
        // TODO: This function is badly implemented. Currently its algorithm complexity is O(n).
        // This will be a problem for 64 entry TLB sets, but whatever. A good design will be implemented later.
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.vpn == vpn && entry.asid.check(&asid) {
                assert!(
                    entry.ts <= ts,
                    "TLB entry is older than the current timestamp.",
                );
                // assert!(entry.is_instruction == is_instruction);
                entry.is_instruction = is_instruction;
                entry.ts = ts;
                return Some(entry.ppn);
            }
        }
        None
    }

    pub fn insert(
        &mut self,
        vpn: u64,
        asid: AddressSpaceID,
        ppn: u64,
        ts: u64,
        is_instruction: bool,
    ) {
        if self.current_pointer < ASSO {
            self.entries[self.current_pointer].valid = true;
            self.entries[self.current_pointer].ts = ts;
            self.entries[self.current_pointer].asid = asid;
            self.entries[self.current_pointer].vpn = vpn;
            self.entries[self.current_pointer].ppn = ppn;
            self.entries[self.current_pointer].is_instruction = is_instruction;
            self.current_pointer += 1;
        } else {
            // find a victim.
            let mut victim_idx = 0;
            for i in 0..ASSO {
                if self.entries[i].ts < self.entries[victim_idx].ts {
                    victim_idx = i;
                }
            }
            self.entries[victim_idx].valid = true;
            self.entries[victim_idx].ts = ts;
            self.entries[victim_idx].asid = asid;
            self.entries[victim_idx].vpn = vpn;
            self.entries[victim_idx].ppn = ppn;
            self.entries[victim_idx].is_instruction = is_instruction;
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TLB<const SET_COUNT: usize, const ASSO: usize> {
    entries: Vec<TLBSet<ASSO>>,
}

impl<const SET_COUNT: usize, const ASSO: usize> TLB<SET_COUNT, ASSO> {
    pub fn new() -> Self {
        Self {
            entries: (0..SET_COUNT).map(|_| TLBSet::new()).collect(),
        }
    }

    pub fn lookup(
        &mut self,
        vpn: u64,
        asid: AddressSpaceID,
        ts: u64,
        is_instruction: bool,
    ) -> Option<u64> {
        let set_index = vpn % SET_COUNT as u64;
        let set = &mut self.entries[set_index as usize];
        set.lookup(vpn, asid, ts, is_instruction)
    }

    pub fn insert(
        &mut self,
        vpn: u64,
        asid: AddressSpaceID,
        ppn: u64,
        ts: u64,
        is_instruction: bool,
    ) {
        let set_index = vpn % SET_COUNT as u64;
        let set = &mut self.entries[set_index as usize];
        set.insert(vpn, asid, ppn, ts, is_instruction);
    }

    // pub fn _invalidate_by_vpn(&mut self, vpn: u64, asid: u16) {
    //     let set_index = vpn % SET_COUNT as u64;
    //     let set = &mut self.entries[set_index as usize];
    //     set.invalidate_by_vpn(vpn, asid);
    // }

    // pub fn _invalidate_by_asid(&mut self, asid: u16) {
    //     for set in self.entries.iter_mut() {
    //         set.invalidate_by_asid(asid);
    //     }
    // }
}

impl<const SET_COUNT: usize, const ASSO: usize> Default for TLB<SET_COUNT, ASSO> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tlbset_new() {
        let tlbset: TLBSet<4> = TLBSet::new();
        assert_eq!(tlbset.current_pointer, 0);
        assert_eq!(tlbset.entries.len(), 4);
    }

    #[test]
    fn test_tlbset_insert_and_lookup() {
        let mut tlbset: TLBSet<4> = TLBSet::new();
        tlbset.insert(1, AddressSpaceID::NonGlobal(1), 1, 1, false);
        assert_eq!(
            tlbset.lookup(1, AddressSpaceID::NonGlobal(1), 2, false),
            Some(1)
        );
    }

    #[test]
    fn test_tlb_new() {
        let tlb: TLB<4, 4> = TLB::new();
        assert_eq!(tlb.entries.len(), 4);
    }

    #[test]
    fn test_tlb_insert_and_lookup() {
        let mut tlb: TLB<4, 4> = TLB::new();
        tlb.insert(1, AddressSpaceID::NonGlobal(1), 1, 1, false);
        assert_eq!(
            tlb.lookup(1, AddressSpaceID::NonGlobal(1), 2, false),
            Some(1)
        );
    }

    #[test]
    fn test_tlbset_replacement_policy() {
        let mut tlbset: TLBSet<4> = TLBSet::new();
        tlbset.insert(1, AddressSpaceID::NonGlobal(1), 1, 1, false);
        tlbset.insert(2, AddressSpaceID::NonGlobal(2), 2, 2, false);
        tlbset.insert(3, AddressSpaceID::NonGlobal(3), 3, 3, false);
        tlbset.insert(4, AddressSpaceID::NonGlobal(4), 4, 4, false);
        tlbset.insert(5, AddressSpaceID::NonGlobal(5), 5, 5, false); // This should replace the first entry

        // The first entry should be replaced, so the lookup should return None
        assert_eq!(
            tlbset.lookup(1, AddressSpaceID::NonGlobal(1), 2, false),
            None
        );
    }

    #[test]
    fn test_tlb_replacement_policy() {
        let mut tlb: TLB<2, 4> = TLB::new();

        // Insert 16 entries, causing multiple replacements
        for i in 0..16 {
            tlb.insert(i, AddressSpaceID::NonGlobal(i as u16), i, i, false);
        }

        // The first 4 entries should have been replaced in each set, so their lookups should return None
        for i in 0..4 {
            assert_eq!(
                tlb.lookup(i, AddressSpaceID::NonGlobal(i as u16), 100 + 1, false),
                None
            );
        }

        // The last 4 entries in each set should still be in the TLB, so their lookups should return their values
        for i in 12..16 {
            assert_eq!(
                tlb.lookup(i, AddressSpaceID::NonGlobal(i as u16), 200 + i, false),
                Some(i)
            );
        }
    }
}
