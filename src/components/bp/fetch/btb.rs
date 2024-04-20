use crate::components::bp::BranchResolveFlag;

use serde::{ser::SerializeStruct, Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct BTBEntry {
    tag_and_valid: u64, // the upper 63 bits are the tag, and the lowest bit is the valid bit
    target: u64,
    ts: u64,
}

pub struct BTB<const SET: usize, const ASSO: usize> {
    array: Vec<[BTBEntry; ASSO]>,
    local_ts: u64,
}

impl<const SET: usize, const ASSO: usize> BTB<SET, ASSO> {
    pub fn new() -> Self {
        BTB {
            array: Vec::from_iter((0..SET).map(|_| {
                std::array::from_fn(|_| BTBEntry {
                    tag_and_valid: 0,
                    target: 0,
                    ts: 0,
                })
            })),
            local_ts: 0,
        }
    }

    pub fn train(&mut self, pc: u64, result: BranchResolveFlag, target: u64) {
        if result == BranchResolveFlag::NotTaken {
            return;
        }
        self.local_ts += 1;

        let index = (pc % SET as u64) as usize;

        // assert_eq!((pc & 0x3), 0);
        let internal_tag = pc | 1;

        // first, find an invalid entry
        for i in 0..ASSO {
            if !(self.array[index][i].tag_and_valid & 1 == 1) {
                self.array[index][i].tag_and_valid = internal_tag;
                self.array[index][i].target = target;
                self.array[index][i].ts = self.local_ts;
                return;
            }
        }

        // Well, we have to evict one, find the one with the minimum timestamp.
        // We will use iterator to find the minimum timestamp.
        let min_index = self.array[index]
            .iter_mut()
            .min_by(|a, b| a.ts.cmp(&b.ts))
            .unwrap();

        min_index.tag_and_valid = internal_tag;
        min_index.target = target;
        min_index.ts = self.local_ts;
    }
}

impl<const SET: usize, const ASSO: usize> Serialize for BTB<SET, ASSO> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_struct("BTB", 1)?;
        s.serialize_field(
            "array",
            &self
                .array
                .iter()
                .map(|el| el.as_slice())
                .collect::<Vec<_>>(),
        )?;
        s.end()
    }
}
