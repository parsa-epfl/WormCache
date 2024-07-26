pub mod fetch;

mod aarch64;
mod callbacks;
use std::io::Write;

use super::Plugin;
use crate::{parameter, qemu_api};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use zstd::{Decoder, Encoder};

// Use Arena to allocate the BranchMetaData.
// https://crates.io/crates/bumpalo

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum BranchType {
    NonBranch = 0,
    Conditional = 1,
    Unconditional = 2,
    DirectCall = 3,
    IndirectBranch = 4,
    IndirectCall = 5,
    Return = 6,
}

impl BranchType {
    pub fn is_call(&self) -> bool {
        match self {
            BranchType::DirectCall => true,
            BranchType::IndirectCall => true,
            _ => false,
        }
    }

    pub fn is_return(&self) -> bool {
        match self {
            BranchType::Return => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BranchResolutionResult {
    pub branch_type: BranchType,
    pub is_taken: bool,
}

impl BranchResolutionResult {
    fn from_u32(value: u32) -> BranchResolutionResult {
        let is_taken = value & 1 == 1;
        let result_value = value >> 1;

        return BranchResolutionResult {
            is_taken,
            branch_type: match result_value {
                0 => BranchType::NonBranch,
                1 => BranchType::Conditional,
                2 => {
                    assert!(is_taken);
                    BranchType::Unconditional
                }
                3 => {
                    assert!(is_taken);
                    BranchType::DirectCall
                }
                4 => {
                    assert!(is_taken);
                    BranchType::IndirectBranch
                }
                5 => {
                    assert!(is_taken);
                    BranchType::IndirectCall
                }
                6 => {
                    assert!(is_taken);
                    BranchType::Return
                }
                _ => unreachable!(),
            },
        };
    }
}

static mut FETCH_UNIT: *mut fetch::FetchUnit<{ parameter::CORE_COUNT }> = std::ptr::null_mut();

unsafe extern "C" fn branch_resolved_cb(vcpu_index: u32, pc: u64, target: u64, flags: u32) {
    if parameter::MEASURE_HALF_OF_CORES && vcpu_index >= parameter::CORE_COUNT as u32 / 2 {
        return;
    }

    let result = BranchResolutionResult::from_u32(flags);
    (*FETCH_UNIT).train(vcpu_index as usize, pc, result, target)
}

pub struct BranchPredictorPlugin {}

impl Plugin for BranchPredictorPlugin {
    fn init(_plugin_id: u64, _options: &FxHashMap<String, String>) {
        println!("BranchPredictorPlugin initialized.");

        assert!(unsafe {
            qemu_api::qemu_plugin_register_vcpu_branch_resolved_cb(Some(branch_resolved_cb))
        });

        unsafe {
            FETCH_UNIT = Box::into_raw(Box::new(fetch::FetchUnit::new()));
        }
    }

    unsafe fn on_translation(_: *mut crate::qemu_api::qemu_plugin_tb) {
        // The callback is already inserted into the TB during init.
    }

    fn dump_snapshot(name: &str) {
        for (core_id, f) in unsafe { &(*FETCH_UNIT).private_units }.iter().enumerate() {
            let file =
                std::fs::File::create(format!("{}/fetch_unit_{}.json", name, core_id)).unwrap();
            // let json = serde_json::to_string(f).unwrap();
            // file.write_all(json.as_bytes()).unwrap();
            serde_json::to_writer(file, &f.get_flexus_checkpoint()).unwrap();
        }
    }

    fn serialize(name: &str) {
        // open a file
        let mut file = std::fs::File::create(format!("{}/fetch.json.zstd", name)).unwrap();

        let mut file = Encoder::new(&mut file, 0).unwrap();

        // write the content
        let json = serde_json::to_string(unsafe { &(*FETCH_UNIT) }).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        file.finish().unwrap();
    }

    fn deserialize(name: &str) {
        // open a file
        let file = std::fs::File::open(format!("{}/fetch.json.zstd", name));

        if file.is_err() {
            println!("Cannot load the fetch unit state. Error: {:?}", file.err());
            return;
        }

        let file = file.unwrap();

        let file = Decoder::new(file).unwrap();

        // read the content
        let reader = std::io::BufReader::new(file);

        // Deserialize the content
        let mut reader = serde_json::Deserializer::from_reader(reader);

        Deserialize::deserialize_in_place(&mut reader, unsafe { &mut (*FETCH_UNIT) }).unwrap();
    }
}
