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

use super::FlexusParameter;
pub use crate::checkpoint::helpers::PHTPerCoreHelper;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize)]
struct FlexusPHTEntry {
    tag: u64,
    access_pattern: Vec<u8>,
    write_pattern: Vec<u8>,
    read_pattern: Vec<u8>,
    ts: u64,
    valid: bool,
}

#[derive(Serialize, Deserialize)]
struct FlexusPHTSet {
    entries: Vec<FlexusPHTEntry>,
}

#[derive(Serialize, Deserialize)]
struct PHTProxy {
    sets: Vec<FlexusPHTSet>,
}

fn serialize_a_pht(pht_proxy: PHTProxy, flexus_configuration: &FlexusParameter) -> PHTProxy {
    assert!(pht_proxy.sets.len() % flexus_configuration.pht_sets == 0);

    if flexus_configuration.no_resizing {
        assert!(pht_proxy.sets.len() == flexus_configuration.pht_sets);
    }

    let mut serialized_pht = Vec::new();

    for _ in 0..flexus_configuration.pht_sets {
        serialized_pht.push(Vec::new());
    }

    for (old_set_idx, mut old_set) in pht_proxy.sets.into_iter().enumerate() {
        let new_set_idx = old_set_idx % flexus_configuration.pht_sets;
        old_set.entries.retain(|entry| entry.valid);
        serialized_pht[new_set_idx].append(&mut old_set.entries);
    }

    for set in serialized_pht.iter_mut() {
        set.sort_by_key(|entry| entry.ts);
        set.reverse();

        if flexus_configuration.no_resizing {
            assert!(
                set.len() <= flexus_configuration.pht_associativity,
                "PHT set is too large",
            );
        }

        set.truncate(flexus_configuration.pht_associativity);
    }

    let mut serialized_pht_json: PHTProxy = PHTProxy { sets: Vec::new() };
    for set in serialized_pht.iter() {
        let mut serialized_set: FlexusPHTSet = FlexusPHTSet {
            entries: Vec::new(),
        };
        for entry in set.iter().rev() {
            serialized_set.entries.push(FlexusPHTEntry {
                tag: entry.tag,
                access_pattern: entry.access_pattern.clone(),
                write_pattern: entry.write_pattern.clone(),
                read_pattern: entry.read_pattern.clone(),
                ts: entry.ts,
                valid: entry.valid,
            });
        }
        serialized_pht_json.sets.push(serialized_set);
    }

    serialized_pht_json
}

pub fn process_sms(checkpoint_folder: &String, flexus: &FlexusParameter, output_folder: &String) {
    let (is_rkyv, pht_checkpoints) =
        crate::checkpoint::detect_checkpoint_files(checkpoint_folder, "pht");
    assert!(!pht_checkpoints.is_empty(), "No PHT checkpoint files found");

    if pht_checkpoints.len() > 1 {
        println!(
            "PHT checkpoint detected (parallel, {} workers, {}).",
            pht_checkpoints.len(),
            if is_rkyv { "rkyv" } else { "JSON" }
        );
    } else {
        println!(
            "PHT checkpoint detected ({}). Filename: {}",
            if is_rkyv { "rkyv" } else { "JSON" },
            pht_checkpoints[0]
        );
    }

    let mut pht: Vec<PHTPerCoreHelper> = Vec::new();
    for file_name in &pht_checkpoints {
        let path = format!("{}/{}", checkpoint_folder, file_name);
        let bytes = crate::util::read_compressed(&path);
        let worker: Vec<PHTPerCoreHelper> = if is_rkyv {
            rkyv::from_bytes::<Vec<PHTPerCoreHelper>, rkyv::rancor::Error>(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        pht.extend(worker);
    }

    for (core_id, unit) in pht.into_iter().enumerate() {
        let file_name = format!("{}/{:03}-pht.json", output_folder, core_id);
        let file = std::fs::File::create(&file_name).unwrap();

        let pht_proxy = PHTProxy {
            sets: unit
                .sets
                .into_iter()
                .map(|set| FlexusPHTSet {
                    entries: set
                        .entries
                        .into_iter()
                        .map(|entry| FlexusPHTEntry {
                            tag: entry.tag,
                            access_pattern: entry.access_pattern,
                            write_pattern: entry.write_pattern,
                            read_pattern: entry.read_pattern,
                            ts: entry.ts,
                            valid: entry.valid,
                        })
                        .collect(),
                })
                .collect(),
        };

        serde_json::to_writer(
            file,
            &json!({
                "pht": serialize_a_pht(pht_proxy, flexus)
            }),
        )
        .unwrap();
        println!("PHT for core {} exported to {}", core_id, file_name);
    }
}
