use crate::{
    checkpoint::helpers::{BTBHelper, FetchUnitHelper, TAGEHelper},
    components::bp::fetch::tage::*,
};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::FlexusParameter;

#[derive(Serialize, Deserialize)]
struct FlexusBTBEntry {
    #[serde(rename = "PC")]
    pc: u64,
    target: u64,
    #[serde(rename = "type")]
    type_: u64,
    ts: u64, // for debugging
}

fn serialize_a_btb(
    btb_proxy: BTBHelper,
    flexus_configuration: &FlexusParameter,
) -> Vec<Vec<FlexusBTBEntry>> {
    assert!(btb_proxy.array.len() % flexus_configuration.btb_sets == 0);

    if flexus_configuration.no_resizing {
        assert!(btb_proxy.array.len() == flexus_configuration.btb_sets);
    }

    let mut serialized_btb = Vec::new();

    // Step 0: Initialize the serialized BTB.
    for _ in 0..flexus_configuration.btb_sets {
        serialized_btb.push(Vec::new());
    }

    // Step 1: Merge sets.
    for (old_set_idx, mut old_set) in btb_proxy.array.into_iter().enumerate() {
        let new_set_idx = old_set_idx % flexus_configuration.btb_sets;
        old_set.retain(|entry| entry.ts != 0); // Filter out invalid entries.
        serialized_btb[new_set_idx].append(&mut old_set);
    }

    // As a sanity check, all entries's PC should be unique.
    let mut pc_set = FxHashSet::default();
    for set in serialized_btb.iter() {
        for entry in set.iter() {
            assert!(!pc_set.contains(&entry.tag));
            pc_set.insert(entry.tag);
        }
    }

    // Step 2: Apply LRU associativity.
    for set in serialized_btb.iter_mut() {
        set.sort_by_key(|entry| entry.ts);
        set.reverse();

        if flexus_configuration.no_resizing {
            assert!(
                set.len() <= flexus_configuration.btb_associativity,
                "BTB set is too large",
            );
        }

        set.truncate(flexus_configuration.btb_associativity);
    }

    // Step 3: Serialize the BTB.
    let mut serialized_btb_json = Vec::new();
    for set in serialized_btb.iter() {
        let mut serialized_set = Vec::new();
        for entry in set.iter().rev() {
            serialized_set.push(FlexusBTBEntry {
                pc: entry.tag,
                target: entry.target,
                type_: entry.branch_type as u64,
                ts: entry.ts,
            });
        }
        serialized_btb_json.push(serialized_set);
    }

    serialized_btb_json
}

#[derive(Serialize, Deserialize)]
struct FlexusTAGEPredictorState {
    #[serde(rename = "TICK")]
    pub tick: i32,
    #[serde(rename = "SEED")]
    pub seed: i32,
    #[serde(rename = "PHIST")]
    pub phist: i32,
    #[serde(rename = "GHIST")]
    pub ghist: Vec<bool>,

    #[serde(rename = "LOGB")]
    pub logb: usize,
    #[serde(rename = "NHIST")]
    pub nhist: usize,
    #[serde(rename = "LOGG")]
    pub logg: usize,
    #[serde(rename = "TBITS")]
    pub tbits: usize,
    #[serde(rename = "MAXHIST")]
    pub maxhist: usize,
    #[serde(rename = "MINHIST")]
    pub minhist: usize,
    #[serde(rename = "CBITS")]
    pub cbits: usize,

    pub btable: Vec<TAGEBiModalEntry>,
    pub gtable: Vec<Vec<TAGEGlobalTableEntry>>,

    pub ch_i: Vec<FoldedHistory>,
    pub ch_t: Vec<Vec<FoldedHistory>>,

    pub m: Vec<usize>,
}

fn serialize_a_tage(tage: TAGEHelper) -> FlexusTAGEPredictorState {
    FlexusTAGEPredictorState {
        tick: tage.tick,
        seed: tage.seed,
        phist: tage.phist,
        ghist: tage.ghist.iter().copied().collect(),

        logb: LOGB,
        nhist: NHIST,
        logg: LOGG,
        tbits: TBITS,
        maxhist: MAXHIST,
        minhist: MINHIST,
        cbits: CBITS,

        btable: tage.btable.to_vec(),
        gtable: tage.gtable.iter().map(|x| x.to_vec()).collect(),

        ch_i: tage.ch_i.to_vec(),
        ch_t: tage.ch_t.iter().map(|x| x.to_vec()).collect(),

        m: HISTORIES.to_vec(),
    }
}

pub fn process_frontend(
    checkpoint_folder: &String,
    flexus_configuration: &FlexusParameter,
    output_folder: &String,
) {
    let (is_rkyv, frontend_checkpoints) =
        crate::checkpoint::detect_checkpoint_files(checkpoint_folder, "fetch");
    assert!(!frontend_checkpoints.is_empty(), "No fetch checkpoint file found");

    if frontend_checkpoints.len() > 1 {
        println!(
            "Frontend checkpoint is detected (parallel, {} workers, {}).",
            frontend_checkpoints.len(),
            if is_rkyv { "rkyv" } else { "JSON" }
        );
        let mut private_units = Vec::new();
        for file_name in &frontend_checkpoints {
            let path = format!("{}/{}", checkpoint_folder, file_name);
            let bytes = crate::util::read_compressed(&path);
            let worker: FetchUnitHelper = if is_rkyv {
                rkyv::from_bytes::<FetchUnitHelper, rkyv::rancor::Error>(&bytes).unwrap()
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            private_units.extend(worker.private_units);
        }
        let combined = FetchUnitHelper { private_units };
        // Export each core's fetch unit
        for (core_id, unit) in combined.private_units.into_iter().enumerate() {
            let file_name = format!("{}/{:03}-bpred.json", output_folder, core_id);
            let file = std::fs::File::create(&file_name).unwrap();
            serde_json::to_writer(
                file,
                &json!({
                    "btb": serialize_a_btb(unit.btb, flexus_configuration),
                    "tage": serialize_a_tage(unit.tage),
                }),
            )
            .unwrap();
            println!("Core {}'s fetch unit is exported to {}", core_id, file_name);
        }
    } else {
        let frontend_checkpoint = &frontend_checkpoints[0];
        println!(
            "Frontend checkpoint is detected ({}). Filename: {}",
            if is_rkyv { "rkyv" } else { "JSON" },
            frontend_checkpoint
        );
        let path = format!("{}/{}", checkpoint_folder, frontend_checkpoint);
        let bytes = crate::util::read_compressed(&path);
        let helper: FetchUnitHelper = if is_rkyv {
            rkyv::from_bytes::<FetchUnitHelper, rkyv::rancor::Error>(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        // Export each core's fetch unit
        for (core_id, unit) in helper.private_units.into_iter().enumerate() {
            let file_name = format!("{}/{:03}-bpred.json", output_folder, core_id);
            let file = std::fs::File::create(&file_name).unwrap();
            serde_json::to_writer(
                file,
                &json!({
                    "btb": serialize_a_btb(unit.btb, flexus_configuration),
                    "tage": serialize_a_tage(unit.tage),
                }),
            )
            .unwrap();
            println!("Core {}'s fetch unit is exported to {}", core_id, file_name);
        }
    }
}
