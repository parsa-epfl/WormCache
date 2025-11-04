use super::FlexusParameter;
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

fn serialize_a_pht(
    pht_proxy: PHTProxy,
    flexus_configuration: &FlexusParameter,
) -> PHTProxy {
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
        old_set.entries.retain(|entry| entry.valid); // Filter out invalid entries.
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
        let mut serialized_set: FlexusPHTSet = FlexusPHTSet { entries: Vec::new() };
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

pub fn process_sms(
    checkpoint_folder: &String,
    flexus: &FlexusParameter,
    output_folder: &String,
) {
    let pht_checkpoints = std::fs::read_dir(checkpoint_folder)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let file_name = entry.file_name().into_string().unwrap();
            if file_name.contains("pht") && file_name.ends_with(".json.zstd") {
                Some(file_name)
            } else {
                None
            }
        })
        .collect::<Vec<String>>();
    
    assert!(!pht_checkpoints.is_empty());
    assert!(pht_checkpoints.len() == 1);
    println!(
        "PHT checkpoint detected. Filename: {}",
        pht_checkpoints[0]
    );
    let file = std::fs::File::open(format!(
        "{}/{}",
        checkpoint_folder, pht_checkpoints[0]
    ))
    .unwrap();

    let decoder = zstd::Decoder::new(file).unwrap();

    let pht: Vec<PHTProxy> = serde_json::from_reader(decoder).unwrap();

    for (core_id, unit) in pht.into_iter().enumerate() {
        let file_name = format!("{}/{:03}-pht.json", output_folder, core_id);
        let file = std::fs::File::create(&file_name).unwrap();
        serde_json::to_writer(
            file, 
            &json!({
                "pht": serialize_a_pht(unit, flexus)
            }),
        )
        .unwrap();
        println!("PHT for core {} exported to {}", core_id, file_name);
    }

}