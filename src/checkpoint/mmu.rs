use serde::Serialize;
use serde_json::json;

use crate::{
    checkpoint::{
        FlexusSTLBInclusion,
        helpers::{TLBHelper, TLBSetHelper},
    },
    components::cache_hierarchy::mmu::tlb::{AddressSpaceID, FullyAssociativeTLB, TLBEntry},
};
use rustc_hash::FxHashMap;

use super::FlexusParameter;

#[derive(Serialize)]
pub struct FlexusTLBEntry {
    asid: u64,
    ng: bool,
    vpn: u64,
    ppn: u64,
    ts: u64,
}

pub struct FlexusMMU {
    itlbs: Vec<Vec<Vec<FlexusTLBEntry>>>,
    dtlbs: Vec<Vec<Vec<FlexusTLBEntry>>>,
    stlbs: Vec<Vec<Vec<FlexusTLBEntry>>>,

    configuration: FlexusParameter,
}

fn serialize_a_tlb_set(set: TLBSetHelper) -> Vec<FlexusTLBEntry> {
    set.entries
        .into_iter()
        .map(|entry| FlexusTLBEntry {
            vpn: entry.vpn,
            ppn: entry.ppn,
            ts: entry.ts,
            ng: matches!(entry.asid, AddressSpaceID::NonGlobal(_)),
            asid: match entry.asid {
                AddressSpaceID::Global => 0,
                AddressSpaceID::NonGlobal(asid) => asid as u64,
            },
        })
        .rev()
        .collect()
}

fn serialize_a_tlb(
    tlb: TLBHelper,
    set_count: usize,
    associativity: usize,
    no_resizing: bool,
    stlb_inclusion: bool,
    evicted_entries: &mut FxHashMap<(u64, AddressSpaceID), TLBEntry>,
) -> Vec<Vec<FlexusTLBEntry>> {
    assert!(tlb.entries.len() % set_count == 0);

    if no_resizing {
        assert!(tlb.entries.len() == set_count);
    }

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
        set.reverse(); // MRU are stored in the front after sorting.

        // We make an inclusion insertion here, considering that the L1 TLBs are small.
        if stlb_inclusion {
            for entry in set.iter() {
                evicted_entries.insert((entry.vpn, entry.asid), entry.clone());
            }
        }

        if set.len() > associativity {
            if no_resizing {
                panic!("The TLB is not resizable, but the entries exceed the associativity.");
            }

            let evicted = set.drain(associativity..);

            if !stlb_inclusion {
                // insert the evicted entries into the evicted_entries map.
                for entry in evicted {
                    evicted_entries.insert((entry.vpn, entry.asid), entry.clone());
                }
            }
        }
    }

    // Step 3: Serialize the entries.
    result
        .into_iter()
        .map(|set| {
            serialize_a_tlb_set(TLBSetHelper {
                entries: set,
                current_pointer: 0,
            })
        })
        .collect()
}

fn render_stlb(
    base: TLBHelper,
    evicted_entries: FxHashMap<(u64, AddressSpaceID), TLBEntry>,
    flexus_configuration: &FlexusParameter,
) -> Vec<Vec<FlexusTLBEntry>> {
    assert!(base.entries.len() % flexus_configuration.stlb_sets == 0);

    if flexus_configuration.no_resizing {
        assert!(base.entries.len() == flexus_configuration.stlb_sets);
    }

    let mut result = vec![];

    for _ in 0..flexus_configuration.stlb_sets {
        result.push(vec![]);
    }

    for (base_set_idx, base_set) in base.entries.into_iter().enumerate() {
        let set_idx = base_set_idx % flexus_configuration.stlb_sets;
        let new_set = &mut result[set_idx];
        new_set.extend(base_set.entries.into_iter().filter_map(|entry| {
            if entry.valid {
                Some(FlexusTLBEntry {
                    vpn: entry.vpn,
                    ppn: entry.ppn,
                    ts: entry.ts,
                    ng: matches!(entry.asid, AddressSpaceID::NonGlobal(_)),
                    asid: match entry.asid {
                        AddressSpaceID::Global => 0,
                        AddressSpaceID::NonGlobal(asid) => asid as u64,
                    },
                })
            } else {
                None
            }
        }));
    }

    for entry in evicted_entries.values() {
        let set_idx = entry.vpn % flexus_configuration.stlb_sets as u64;
        assert!(entry.valid);
        result[set_idx as usize].push(FlexusTLBEntry {
            vpn: entry.vpn,
            ppn: entry.ppn,
            ts: entry.ts,
            ng: matches!(entry.asid, AddressSpaceID::NonGlobal(_)),
            asid: match entry.asid {
                AddressSpaceID::Global => 0,
                AddressSpaceID::NonGlobal(asid) => asid as u64,
            },
        });
    }

    for set in result.iter_mut() {
        set.sort_by_key(|entry| entry.ts);
        set.reverse(); // MRU are stored in the front after sorting.

        if set.len() > flexus_configuration.stlb_associativity {
            if flexus_configuration.no_resizing {
                panic!("The TLB is not resizable, but the entries exceed the associativity.");
            }

            set.drain(flexus_configuration.stlb_associativity..);
        }
    }

    result
}

impl FlexusMMU {
    pub fn from_harvard_tlb(
        itlb: Vec<TLBHelper>,
        dtlb: Vec<TLBHelper>,
        stlb: Vec<TLBHelper>,
        configuration: FlexusParameter,
    ) -> Self {
        assert_eq!(itlb.len(), dtlb.len());
        assert_eq!(itlb.len(), stlb.len());

        let mut itlbs = vec![];
        let mut dtlbs = vec![];
        let mut stlbs = vec![];

        for (itlb, (dtlb, stlb)) in itlb.into_iter().zip(dtlb.into_iter().zip(stlb.into_iter())) {
            let mut evicted_entries = FxHashMap::default();
            let itlb = serialize_a_tlb(
                itlb,
                configuration.itlb_sets,
                configuration.itlb_associativity,
                configuration.no_resizing,
                matches!(configuration.stlb_inclusion, FlexusSTLBInclusion::Inclusive),
                &mut evicted_entries,
            );
            let dtlb = serialize_a_tlb(
                dtlb,
                configuration.dtlb_sets,
                configuration.dtlb_associativity,
                configuration.no_resizing,
                matches!(configuration.stlb_inclusion, FlexusSTLBInclusion::Inclusive),
                &mut evicted_entries,
            );
            let stlb = render_stlb(stlb, evicted_entries, &configuration);

            itlbs.push(itlb);
            dtlbs.push(dtlb);
            stlbs.push(stlb);
        }

        Self {
            itlbs,
            dtlbs,
            stlbs,
            configuration,
        }
    }

    pub fn export(&self, folder_name: &String) {
        // At present, we only support exporting the harvard TLB, and it has to be fully associative.

        for (core_id, itlb) in self.itlbs.iter().enumerate() {
            let file_name = format!("{}/{:03}-mmu-itlb.json", folder_name, core_id);
            let mut file = std::fs::File::create(&file_name).unwrap();

            serde_json::to_writer(
                &mut file,
                &json!({
                    "associativity": self.configuration.itlb_associativity,
                    "entries": itlb,
                }),
            )
            .unwrap();

            println!("Core {}'s ITLB is exported to {}", core_id, file_name);

            let file_name = format!("{}/{:03}-mmu-dtlb.json", folder_name, core_id);
            let mut file = std::fs::File::create(&file_name).unwrap();

            serde_json::to_writer(
                &mut file,
                &json!({
                    "associativity": self.configuration.dtlb_associativity,
                    "entries": self.dtlbs[core_id],
                }),
            )
            .unwrap();

            println!("Core {}'s DTLB is exported to {}", core_id, file_name);

            // Expose the STLB.
            let file_name = format!("{}/{:03}-mmu-stlb.json", folder_name, core_id);
            let mut file = std::fs::File::create(&file_name).unwrap();

            serde_json::to_writer(
                &mut file,
                &json!({
                    "associativity": self.configuration.stlb_associativity,
                    "entries": self.stlbs[core_id],
                }),
            )
            .unwrap();

            println!("Core {}'s STLB is exported to {}", core_id, file_name);
        }
    }
}

fn load_tlb_json(value: serde_json::Value, is_instruction: bool) -> TLBHelper {
    if let Ok(result) = serde_json::from_value(value.clone()) {
        result
    } else {
        // Well, this is a newer version of the checkpoint. It is a FullyAssociativeTLB.
        let mut fully_assoaicative_tlb: FullyAssociativeTLB =
            serde_json::from_value(value).unwrap();

        // process the deferred insertions in the TLB.
        fully_assoaicative_tlb.run_lru();

        // do the type conversion.
        let entries: Vec<_> = fully_assoaicative_tlb
            .elements
            .into_iter()
            .map(|(hash, entry)| {
                let (vpn, asid) = FullyAssociativeTLB::unpack_hash(hash);
                TLBEntry {
                    vpn,
                    asid,
                    ppn: entry.ppn,
                    ts: entry.ts,
                    valid: true,
                    is_instruction,
                }
            })
            .collect();

        TLBHelper {
            entries: vec![TLBSetHelper {
                entries,
                current_pointer: 0,
            }],
        }
    }
}

/// Convert a FullyAssociativeTLBHelper to SerializedTLB for processing.
fn fully_associative_tlb_helper_to_serialized(
    helper: crate::checkpoint::helpers::FullyAssociativeTLBHelper,
    is_instruction: bool,
) -> TLBHelper {
    let entries: Vec<_> = helper
        .elements
        .into_iter()
        .map(|(hash, entry)| {
            let (vpn, asid) = FullyAssociativeTLB::unpack_hash(hash);
            TLBEntry {
                vpn,
                asid,
                ppn: entry.ppn,
                ts: entry.ts,
                valid: true,
                is_instruction,
            }
        })
        .collect();

    TLBHelper {
        entries: vec![TLBSetHelper {
            entries,
            current_pointer: 0,
        }],
    }
}

/// Detect whether to use rkyv or JSON format based on file existence.
fn detect_mmu_format(checkpoint_folder: &str) -> (bool, String) {
    // First check for rkyv files
    let rkyv_files: Vec<String> = std::fs::read_dir(checkpoint_folder)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let file_name = entry.file_name().into_string().unwrap();
            if file_name.ends_with(".rkyv.zstd") && file_name.contains("mmu") {
                Some(file_name)
            } else {
                None
            }
        })
        .collect();

    if !rkyv_files.is_empty() {
        assert_eq!(
            rkyv_files.len(),
            1,
            "Expected exactly one mmu.rkyv.zstd file"
        );
        return (true, rkyv_files.into_iter().next().unwrap());
    }

    // Fall back to JSON files
    let json_files: Vec<String> = std::fs::read_dir(checkpoint_folder)
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
        .collect();

    assert_eq!(
        json_files.len(),
        1,
        "Expected exactly one mmu.json.zstd file"
    );
    (false, json_files.into_iter().next().unwrap())
}

pub fn process_mmus(
    checkpoint_folder: &String,
    flexus_configuration: &FlexusParameter,
    output_folder: &String,
) {
    use crate::checkpoint::helpers::{MMUHelper, MMUsHelper};

    let (is_rkyv, mmu_checkpoint) = detect_mmu_format(checkpoint_folder);

    println!(
        "MMU checkpoint is detected ({}). Filename: {}",
        if is_rkyv { "rkyv" } else { "JSON" },
        mmu_checkpoint
    );

    let file = std::fs::File::open(format!("{}/{}", checkpoint_folder, mmu_checkpoint)).unwrap();
    let mut decoder = zstd::Decoder::new(file).unwrap();

    let (i_tlbs, d_tlbs, s_tlbs): (Vec<TLBHelper>, Vec<TLBHelper>, Vec<TLBHelper>) = if is_rkyv {
        // Load rkyv format
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();

        let mmus_helper: MMUsHelper =
            rkyv::from_bytes::<MMUsHelper, rkyv::rancor::Error>(&bytes).unwrap();

        let mut i_tlbs = Vec::new();
        let mut d_tlbs = Vec::new();
        let mut s_tlbs = Vec::new();

        for mmu_helper in mmus_helper.mmus {
            match mmu_helper {
                MMUHelper::OrdinaryMMU(helper) => {
                    i_tlbs.push(helper.itlb);
                    d_tlbs.push(helper.dtlb);
                    s_tlbs.push(helper.stlb);
                }
                MMUHelper::FullyAssociativeL1MMU(helper) => {
                    i_tlbs.push(fully_associative_tlb_helper_to_serialized(
                        helper.itlb,
                        true,
                    ));
                    d_tlbs.push(fully_associative_tlb_helper_to_serialized(
                        helper.dtlb,
                        false,
                    ));
                    s_tlbs.push(helper.stlb);
                }
                MMUHelper::NoMMU(_) => {
                    // NoMMU has no TLB entries
                    i_tlbs.push(TLBHelper { entries: vec![] });
                    d_tlbs.push(TLBHelper { entries: vec![] });
                    s_tlbs.push(TLBHelper { entries: vec![] });
                }
            }
        }

        (i_tlbs, d_tlbs, s_tlbs)
    } else {
        // Load JSON format (legacy)
        let mmus: serde_json::Value = serde_json::from_reader(decoder).unwrap();

        let i_tlbs: Vec<TLBHelper> = match mmus.clone() {
            serde_json::Value::Array(vec) => vec
                .iter()
                .map(|mmu| load_tlb_json(mmu["itlb"].clone(), true))
                .collect::<Vec<_>>(),
            _ => panic!("The MMU checkpoint is not an array."),
        };

        let d_tlbs: Vec<TLBHelper> = match mmus.clone() {
            serde_json::Value::Array(vec) => vec
                .iter()
                .map(|mmu| load_tlb_json(mmu["dtlb"].clone(), false))
                .collect::<Vec<_>>(),
            _ => panic!("The MMU checkpoint is not an array."),
        };

        let s_tlbs: Vec<TLBHelper> = match mmus {
            serde_json::Value::Array(vec) => vec
                .iter()
                .map(|mmu| serde_json::from_value(mmu["stlb"].clone()).unwrap())
                .collect::<Vec<_>>(),
            _ => panic!("The MMU checkpoint is not an array."),
        };

        (i_tlbs, d_tlbs, s_tlbs)
    };

    let mmu = FlexusMMU::from_harvard_tlb(i_tlbs, d_tlbs, s_tlbs, flexus_configuration.clone());

    mmu.export(output_folder);
}
