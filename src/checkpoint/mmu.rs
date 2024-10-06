use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::components::cache_hierarchy::mmu::tlb::TLBEntry;

use super::FlexusParameter;

#[derive(Serialize, Deserialize)]
struct SerializedTLBSet {
    entries: Vec<TLBEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct SerializedTLB {
    entries: Vec<SerializedTLBSet>,
}

#[derive(Serialize)]
pub struct FlexusTLBEntry {
    vpn: u64,
    ppn: u64,
}

pub struct FlexusMMU {
    itlbs: Vec<Vec<Vec<FlexusTLBEntry>>>,
    dtlbs: Vec<Vec<Vec<FlexusTLBEntry>>>,

    configuration: FlexusParameter,
}

fn serialize_a_tlb_set(set: SerializedTLBSet) -> Vec<FlexusTLBEntry> {
    set.entries
        .into_iter()
        .map(|entry| FlexusTLBEntry {
            vpn: entry.vpn,
            ppn: entry.ppn,
        })
        .collect()
}

fn serialize_a_tlb(
    tlb: SerializedTLB,
    set_count: usize,
    associativity: usize,
) -> Vec<Vec<FlexusTLBEntry>> {
    assert!(tlb.entries.len() % set_count == 0);

    let mut result = vec![];

    for _ in 0..set_count {
        result.push(vec![]);
    }

    // Step 1: Group the entries by set.
    for (set_idx, mut set) in tlb.entries.into_iter().enumerate() {
        // filter invalid entries
        set.entries.retain(|entry| entry.valid);

        let set_idx = set_idx % set_count;
        let new_set = &mut result[set_idx];
        new_set.extend(set.entries);
    }

    // Step 2: Sort by the timestamp.
    for set in result.iter_mut() {
        set.sort_by_key(|entry| entry.ts);
        set.reverse();

        set.truncate(associativity);
    }

    // Step 3: Serialize the entries.
    result
        .into_iter()
        .map(|set| serialize_a_tlb_set(SerializedTLBSet { entries: set }))
        .collect()
}

impl FlexusMMU {
    pub fn from_harvard_tlb(
        itlb: Vec<SerializedTLB>,
        dtlb: Vec<SerializedTLB>,
        configuration: FlexusParameter,
    ) -> Self {
        Self {
            itlbs: itlb
                .into_iter()
                .map(|tlb| {
                    serialize_a_tlb(
                        tlb,
                        configuration.itlb_sets,
                        configuration.itlb_associativity,
                    )
                })
                .collect(),
            dtlbs: dtlb
                .into_iter()
                .map(|tlb| {
                    serialize_a_tlb(
                        tlb,
                        configuration.dtlb_sets,
                        configuration.dtlb_associativity,
                    )
                })
                .collect(),
            configuration,
        }
    }

    pub fn from_unified_tlb(tlbs: Vec<SerializedTLB>, configuration: FlexusParameter) -> Self {
        let mut itlbs = vec![];
        let mut dtlbs = vec![];

        for tlb in tlbs {
            let mut itlb = SerializedTLB { entries: vec![] };
            let mut dtlb = SerializedTLB { entries: vec![] };

            for set in tlb.entries {
                let mut itlb_entries = vec![];
                let mut dtlb_entries = vec![];

                for entry in set.entries {
                    if entry.is_instruction {
                        itlb_entries.push(entry);
                    } else {
                        dtlb_entries.push(entry);
                    }
                }

                itlb.entries.push(SerializedTLBSet {
                    entries: itlb_entries,
                });
                dtlb.entries.push(SerializedTLBSet {
                    entries: dtlb_entries,
                });
            }

            itlbs.push(itlb);
            dtlbs.push(dtlb);
        }

        Self::from_harvard_tlb(itlbs, dtlbs, configuration)
    }

    pub fn export(&self, folder_name: &String) {
        // At present, we only support exporting the harvard TLB, and it has to be fully associative.

        assert_eq!(self.configuration.itlb_sets, 1);
        assert_eq!(self.configuration.dtlb_sets, 1);

        for (core_id, itlb) in self.itlbs.iter().enumerate() {
            let file_name = format!("{}/{:03}-mmu-itlb.json", folder_name, core_id);
            let mut file = std::fs::File::create(&file_name).unwrap();

            serde_json::to_writer(
                &mut file,
                &json!({
                    "capacity": self.configuration.itlb_associativity,
                    "entries": itlb[0],
                }),
            )
            .unwrap();

            println!("Core {}'s ITLB is exported to {}", core_id, file_name);

            let file_name = format!("{}/{:03}-mmu-dtlb.json", folder_name, core_id);
            let mut file = std::fs::File::create(&file_name).unwrap();

            serde_json::to_writer(
                &mut file,
                &json!({
                    "capacity": self.configuration.dtlb_associativity,
                    "entries": self.dtlbs[core_id][0],
                }),
            )
            .unwrap();

            println!("Core {}'s DTLB is exported to {}", core_id, file_name);
        }
    }
}

pub fn process_mmus(
    checkpoint_folder: &String,
    flexus_configuration: &FlexusParameter,
    output_folder: &String,
) {
    // find the MMU checkpoint.

    // find the file that is named as "mmu-*.json.zstd".

    let mmu_checkpoint = std::fs::read_dir(checkpoint_folder)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let file_name = entry.file_name().into_string().unwrap();
            if file_name.ends_with(".json.zstd") && file_name.contains("mmu") {
                Some(file_name)
            } else {
                None
            }
        })
        .collect::<Vec<String>>();

    assert_eq!(mmu_checkpoint.len(), 1);
    println!(
        "MMU checkpoint is detected. Filename: {}",
        mmu_checkpoint[0]
    );

    let file = std::fs::File::open(format!("{}/{}", checkpoint_folder, mmu_checkpoint[0])).unwrap();

    let decoder = zstd::Decoder::new(file).unwrap();

    // The file should be an array containing a list of MMUs for each core. I need to extract the TLBs by myself.

    let mmus: serde_json::Value = serde_json::from_reader(decoder).unwrap();

    let mmus: Vec<SerializedTLB> = match mmus {
        serde_json::Value::Array(vec) => vec
            .iter()
            .map(|mmu| serde_json::from_value(mmu["tlb"].clone()).unwrap())
            .collect::<Vec<_>>(),
        _ => panic!("The MMU checkpoint is not an array."),
    };

    let mmu = FlexusMMU::from_unified_tlb(mmus, flexus_configuration.clone());

    mmu.export(output_folder);
}
