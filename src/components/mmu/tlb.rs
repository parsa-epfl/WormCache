
#[derive(Debug)]
pub struct TLBEntry {
    valid: bool,
    ts: u64,
    asid: u16,
    vpn: u64,
    ppn: u64,
}

#[derive(Debug)]
struct TLBSet<const ASSO: usize> {
    entries: [TLBEntry; ASSO],
    current_pointer: usize,
}

impl<const ASSO: usize> TLBSet<ASSO> {
    pub fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| TLBEntry {
                valid: false,
                ts: 0,
                asid: 0,
                vpn: 0,
                ppn: 0,
            }),
            current_pointer: 0,
        }
    }

    pub fn lookup(&mut self, vpn: u64, asid: u16, ts: u64) -> Option<u64> {
        // TODO: This function is badly implemented. Currently its algorithm complexity is O(n).
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.vpn == vpn && entry.asid == asid {
                entry.ts = ts;
                return Some(entry.ppn);
            }
        }
        None
    }

    pub fn insert(&mut self, vpn: u64, asid: u16, ppn: u64, ts: u64) {
        if self.current_pointer < ASSO {
            self.entries[self.current_pointer].valid = true;
            self.entries[self.current_pointer].ts = ts;
            self.entries[self.current_pointer].asid = asid;
            self.entries[self.current_pointer].vpn = vpn;
            self.entries[self.current_pointer].ppn = ppn;
            self.current_pointer += 1;
            return;
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
        }
    }

    // fn invalidate_by_vpn(&mut self, _vpn: u64, _asid: u16) {
    //     unimplemented!();
    //     // for entry in self.entries.iter_mut() {
    //     //     if entry.valid && entry.vpn == vpn && entry.asid == asid {
    //     //         entry.valid = false;
    //     //     }
    //     // }
    // }

    // fn invalidate_by_asid(&mut self, _asid: u16) {
    //     unimplemented!();
    //     // for entry in self.entries.iter_mut() {
    //     //     if entry.valid && entry.asid == asid {
    //     //         entry.valid = false;
    //     //     }
    //     // }
    // }

    // fn is_warm(&self) -> bool {
    //     self.current_pointer == ASSO
    // }
}

#[derive(Debug)]
pub struct TLB<const SET_COUNT: usize, const ASSO: usize> {
    entries: Vec<TLBSet<ASSO>>,
}

impl<const SET_COUNT: usize, const ASSO: usize> TLB<SET_COUNT, ASSO> {
    pub fn new() -> Self {
        Self {
            entries: (0..SET_COUNT).map(|_| TLBSet::new()).collect(),
        }
    }

    pub fn lookup(&mut self, vpn: u64, asid: u16, ts: u64) -> Option<u64> {
        let set_index = vpn % SET_COUNT as u64;
        let set = &mut self.entries[set_index as usize];
        set.lookup(vpn, asid, ts)
    }

    pub fn insert(&mut self, vpn: u64, asid: u16, ppn: u64, ts: u64) {
        let set_index = vpn % SET_COUNT as u64;
        let set = &mut self.entries[set_index as usize];
        set.insert(vpn, asid, ppn, ts);
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
        tlbset.insert(1, 1, 1, 1);
        assert_eq!(tlbset.lookup(1, 1, 2), Some(1));
    }

    #[test]
    fn test_tlb_new() {
        let tlb: TLB<4, 4> = TLB::new();
        assert_eq!(tlb.entries.len(), 4);
    }

    #[test]
    fn test_tlb_insert_and_lookup() {
        let mut tlb: TLB<4, 4> = TLB::new();
        tlb.insert(1, 1, 1, 1);
        assert_eq!(tlb.lookup(1, 1, 2), Some(1));
    }

    #[test]
    fn test_tlbset_replacement_policy() {
        let mut tlbset: TLBSet<4> = TLBSet::new();
        tlbset.insert(1, 1, 1, 1);
        tlbset.insert(2, 2, 2, 2);
        tlbset.insert(3, 3, 3, 3);
        tlbset.insert(4, 4, 4, 4);
        tlbset.insert(5, 5, 5, 5); // This should replace the first entry

        // The first entry should be replaced, so the lookup should return None
        assert_eq!(tlbset.lookup(1, 1, 2), None);
    }

    #[test]
    fn test_tlb_replacement_policy() {
        let mut tlb: TLB<2, 4> = TLB::new();

        // Insert 16 entries, causing multiple replacements
        for i in 0..16 {
            tlb.insert(i, i as u16, i, i);
        }

        // The first 4 entries should have been replaced in each set, so their lookups should return None
        for i in 0..4 {
            assert_eq!(tlb.lookup(i, i as u16, 2), None);
        }

        // The last 4 entries in each set should still be in the TLB, so their lookups should return their values
        for i in 12..16 {
            assert_eq!(tlb.lookup(i, i as u16, 2), Some(i));
        }
    }
}