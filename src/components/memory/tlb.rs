pub struct TLBEntry {
    valid: bool,
    ts: u64,
    asid: u16,
    vpn: u64,
    ppn: u64,
}

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
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.vpn == entry.vpn && entry.asid == asid {
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

    fn invalidate_by_vpn(&mut self, vpn: u64, asid: u16) {
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.vpn == entry.vpn && entry.asid == asid {
                entry.valid = false;
            }
        }
    }

    fn invalidate_by_asid(&mut self, asid: u16) {
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.asid == asid {
                entry.valid = false;
            }
        }
    }

    fn is_warm(&self) -> bool {
        self.current_pointer == ASSO
    }
}

pub struct TLB<const SET_COUNT: usize, const ASSO: usize> {
    entries: Vec<TLBSet<ASSO>>,
    warmed_set: usize,
}

impl<const SET_COUNT: usize, const ASSO: usize> TLB<SET_COUNT, ASSO> {
    pub fn new() -> Self {
        Self {
            entries: (0..SET_COUNT).map(|_| TLBSet::new()).collect(),
            warmed_set: 0,
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

    pub fn invalidate_by_vpn(&mut self, vpn: u64, asid: u16) {
        let set_index = vpn % SET_COUNT as u64;
        let set = &mut self.entries[set_index as usize];
        set.invalidate_by_vpn(vpn, asid);
    }

    pub fn invalidate_by_asid(&mut self, asid: u16) {
        for set in self.entries.iter_mut() {
            set.invalidate_by_asid(asid);
        }
    }
}
