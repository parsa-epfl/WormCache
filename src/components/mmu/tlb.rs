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
use serde_json::json;
use serde_with::serde_as;

use crate::parameter::CACHE_SET_SIMD_SEARCH_LANE;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct TLBTag {
    pub inner: u64,
}

impl TLBTag {

    #[inline]
    fn invalid() -> Self {
        Self { inner: 0 }
    }

    #[inline]
    fn new(vpn: u64, asid: u16) -> Self {
        // I need to check whether [52:45] are all 1s or not.
        assert!(
            (vpn >> 45) == 0x7f || (vpn >> 45) == 0,
            "VPN is not in the canonical form."
        );

        // The higher 12 bits should be constant zero.
        assert!((vpn >> 52) == 0, "VPN is not in the canonical form.");

        // I can also work on the 57-bit VA, which means VPN size is 46-bit.
        // The ASID is 16 bits. 
        let real_vpn = vpn & 0x3fff_ffff_ffff;


        // fields:
        // - [0:0]: valid;
        // - [46:1]: real_vpn;
        // - [62:47]: asid;

        let inner = 0
            | (1 << 0)
            | (real_vpn << 1)
            | ((asid as u64) << 47);

        Self { inner }
    }

    fn is_valid(&self) -> bool {
        self.inner & 1 != 0
    }

    fn vpn(&self) -> u64 {
        let real_vpn = (self.inner >> 1) & 0x3fff_ffff_ffff;
        let permission_bit = real_vpn >> 45;
        let permission_bits = if permission_bit != 0 {
            0x3f
        } else {
            0   
        };
        // Now, we compose the real VPN:
        // - [45:0] -> real VPN
        // - [51:46] -> permission bits replicated.
        let vpn = (real_vpn) | (permission_bits << 46);
        vpn
    }

    fn asid(&self) -> u16 {
        ((self.inner >> 47) & 0xffff) as u16
    }

}

#[test]
fn test_tlb_tag_conversion() {
    let normal_va = 0x0000_ffff_8000_0000_u64;

    let tag = TLBTag::new(normal_va >> 12, 0x7f);
    assert_eq!(tag.vpn(), normal_va >> 12);
    assert_eq!(tag.asid(), 0x7f);

    let os_va = 0xffff_8000_0000_8214_u64;
    let tag = TLBTag::new(os_va >> 12, 0x7f);
    assert_eq!(tag.vpn(), os_va >> 12);
    assert_eq!(tag.asid(), 0x7f);
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TLBEntry {
    ts: u64,
    ppn: u64,
    is_instruction: bool,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
struct TLBSet<const ASSO: usize> {
    #[serde_as(as = "[_; ASSO]")]
    tags: [u64; ASSO],

    #[serde_as(as = "[_; ASSO]")]
    entries: [TLBEntry; ASSO],
    current_pointer: usize,
}

impl<const ASSO: usize> TLBSet<ASSO> {
    pub fn new() -> Self {
        Self {
            tags: [0; ASSO],
            entries: std::array::from_fn(|_| TLBEntry {
                ts: 0,
                ppn: 0,
                is_instruction: false,
            }),
            current_pointer: 0,
        }
    }

    #[inline]
    pub fn index_of(&self, vpn: u64, asid: u16) -> Option<usize> {
        use std::simd::*;
        use std::simd::prelude::*;

        let target = TLBTag::new(vpn, asid).inner;

        let simd_target = Simd::<u64,CACHE_SET_SIMD_SEARCH_LANE>::splat(target);
    
        for (i, chunk) in self.tags.chunks_exact(CACHE_SET_SIMD_SEARCH_LANE).enumerate() {
            let simd_chunk = Simd::from_slice(chunk);
    
            // Compare chunk with the target
            let mask = simd_chunk.simd_eq(simd_target);

            let mask = mask.to_bitmask();

            let index = mask.trailing_zeros();    
            // Check if any lane matches
            if (index as usize) < CACHE_SET_SIMD_SEARCH_LANE {
                return Some(i * CACHE_SET_SIMD_SEARCH_LANE + index as usize);
            }
        }

        None
    }

    pub fn lookup(&mut self, vpn: u64, asid: u16, ts: u64, is_instruction: bool) -> Option<u64> {
        if let Some(idx) = self.index_of(vpn, asid) {
            let entry = &mut self.entries[idx];
            assert!(
                entry.ts <= ts,
                "TLB entry is older than the current timestamp.",
            );
            entry.ts = ts;
            entry.is_instruction = is_instruction;
            return Some(entry.ppn);
        }
        None
    }

    pub fn insert(&mut self, vpn: u64, asid: u16, ppn: u64, ts: u64, is_instruction: bool) {
        let tag = TLBTag::new(vpn, asid).inner;
        if self.current_pointer < ASSO {
            self.tags[self.current_pointer] = tag;
            self.entries[self.current_pointer].ts = ts;
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
            self.tags[victim_idx] = tag;
            self.entries[victim_idx].ts = ts;
            self.entries[victim_idx].ppn = ppn;
            self.entries[victim_idx].is_instruction = is_instruction;
        }
    }

    #[inline]
    pub fn invalidate_by_idx(&mut self, idx: usize) {
        self.tags[idx] = TLBTag::invalid().inner;

        self.entries[idx] = TLBEntry {
            ts: 0,
            ppn: 0,
            is_instruction: false,
        };
    }

    #[allow(dead_code)]
    pub fn invalidate_by_vpn_asid(&mut self, vpn: u64, asid: u16) {
        if let Some(idx) = self.index_of(vpn, asid) {
            self.invalidate_by_idx(idx);
        }
    }

    #[allow(dead_code)]
    pub fn invalidate_by_asid(&mut self, asid: u16) {
        for i in 0..ASSO {
            if self.tags[i] != 0 {
                let tag = TLBTag { inner: self.tags[i] };
                if tag.asid() == asid {
                    self.invalidate_by_idx(i);
                }
            }
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

    pub fn lookup(&mut self, vpn: u64, asid: u16, ts: u64, is_instruction: bool) -> Option<u64> {
        let set_index = vpn % SET_COUNT as u64;
        let set = &mut self.entries[set_index as usize];
        set.lookup(vpn, asid, ts, is_instruction)
    }

    pub fn insert(&mut self, vpn: u64, asid: u16, ppn: u64, ts: u64, is_instruction: bool) {
        let set_index = vpn % SET_COUNT as u64;
        let set = &mut self.entries[set_index as usize];
        set.insert(vpn, asid, ppn, ts, is_instruction);
    }

    pub fn _invalidate_by_vpn_asid(&mut self, vpn: u64, asid: u16) {
        let set_index = vpn % SET_COUNT as u64;
        let set = &mut self.entries[set_index as usize];
        set.invalidate_by_vpn_asid(vpn, asid);
    }

    pub fn _invalidate_by_asid(&mut self, asid: u16) {
        for set in self.entries.iter_mut() {
            set.invalidate_by_asid(asid);
        }
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
        tlbset.insert(1, 1, 1, 1, false);
        assert_eq!(tlbset.lookup(1, 1, 2, false), Some(1));
    }

    #[test]
    fn test_tlb_new() {
        let tlb: TLB<4, 4> = TLB::new();
        assert_eq!(tlb.entries.len(), 4);
    }

    #[test]
    fn test_tlb_insert_and_lookup() {
        let mut tlb: TLB<4, 4> = TLB::new();
        tlb.insert(1, 1, 1, 1, false);
        assert_eq!(tlb.lookup(1, 1, 2, false), Some(1));
    }

    #[test]
    fn test_tlbset_replacement_policy() {
        let mut tlbset: TLBSet<4> = TLBSet::new();
        tlbset.insert(1, 1, 1, 1, false);
        tlbset.insert(2, 2, 2, 2, false);
        tlbset.insert(3, 3, 3, 3, false);
        tlbset.insert(4, 4, 4, 4, false);
        tlbset.insert(5, 5, 5, 5, false); // This should replace the first entry

        // The first entry should be replaced, so the lookup should return None
        assert_eq!(tlbset.lookup(1, 1, 2, false), None);
    }

    #[test]
    fn test_tlb_replacement_policy() {
        let mut tlb: TLB<2, 4> = TLB::new();

        // Insert 16 entries, causing multiple replacements
        for i in 0..16 {
            tlb.insert(i, i as u16, i, i, false);
        }

        // The first 4 entries should have been replaced in each set, so their lookups should return None
        for i in 0..4 {
            assert_eq!(tlb.lookup(i, i as u16, 100 + 1, false), None);
        }

        // The last 4 entries in each set should still be in the TLB, so their lookups should return their values
        for i in 12..16 {
            assert_eq!(tlb.lookup(i, i as u16, 200 + i, false), Some(i));
        }
    }
}

// Serializer to flexus checkpoint unit.

#[derive(Serialize)]
pub struct TLBEntryFlexusSerHelper {
    vpn: u64,
    ppn: u64,
}

impl<const SET_COUNT: usize, const ASSO: usize> TLB<SET_COUNT, ASSO> {
    pub fn get_flexus_checkpoint(
        &self,
        i_capacity: usize,
        d_capacity: usize,
    ) -> [serde_json::Value; 2] {
        let mut i_tlb_entries: Vec<(TLBTag, TLBEntry)> = Vec::new();
        let mut d_tlb_entries: Vec<(TLBTag, TLBEntry)> = Vec::new();

        // iterate all possible TLB entries
        for set in self.entries.iter() {
            for (tag, entry) in set.tags.iter().zip(set.entries.iter()) {
                let tag = TLBTag { inner: *tag };
                if tag.is_valid() {
                    if entry.is_instruction {
                        // we plan to put this in the instruction TLB
                        if i_tlb_entries.len() < i_capacity {
                            i_tlb_entries.push((tag, entry.clone()));
                        } else {
                            // replace the entry with the smallest timestamp.
                            let mut victim_idx = 0;
                            for i in 0..i_capacity {
                                if i_tlb_entries[i].1.ts < i_tlb_entries[victim_idx].1.ts {
                                    victim_idx = i;
                                }
                            }
                            i_tlb_entries[victim_idx] = (tag, entry.clone());
                        }
                    } else {
                        // Data TLB
                        if d_tlb_entries.len() < d_capacity {
                            d_tlb_entries.push((tag, entry.clone()));
                        } else {
                            // replace the entry with the smallest timestamp.
                            let mut victim_idx = 0;
                            for i in 0..d_capacity {
                                if d_tlb_entries[i].1.ts < d_tlb_entries[victim_idx].1.ts {
                                    victim_idx = i;
                                }
                            }
                            d_tlb_entries[victim_idx] = (tag, entry.clone());
                        }
                    }
                }
            }
        }

        // sort the i_tlb_entries and d_tlb_entries by their timestamp.
        // smaller timesttamp first.
        i_tlb_entries.sort_by(|a, b| a.1.ts.cmp(&b.1.ts));
        d_tlb_entries.sort_by(|a, b| a.1.ts.cmp(&b.1.ts));

        let i_tlb_entries = i_tlb_entries
            .into_iter()
            .map(|entry| TLBEntryFlexusSerHelper {
                vpn: entry.0.vpn(),
                ppn: entry.1.ppn,
            })
            .collect::<Vec<_>>();

        let d_tlb_entries = d_tlb_entries
            .into_iter()
            .map(|entry| TLBEntryFlexusSerHelper {
                vpn: entry.0.vpn(),
                ppn: entry.1.ppn,
            })
            .collect::<Vec<_>>();

        // alright. Now, construct the result.
        [
            json!({
                "capacity": i_capacity,
                "entries": i_tlb_entries
            }),
            json!({
                "capacity": d_capacity,
                "entries": d_tlb_entries
            }),
        ]
    }
}
