pub mod fetch;

mod aarch64;
mod callbacks;
use std::io::Write;

use super::Plugin;
use crate::{parameter, qemu_api};

use serde::Deserialize;

use zstd::{Decoder, Encoder};

// Use Arena to allocate the BranchMetaData.
// https://crates.io/crates/bumpalo

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum BranchResolveFlag {
    Taken = 0,
    NotTaken = 1,
    Call = 2,
    Return = 3,
    Indirect = 4,
}

impl BranchResolveFlag {
    fn from_u32(value: u32) -> Option<BranchResolveFlag> {
        match value {
            0 => Some(BranchResolveFlag::Taken),
            1 => Some(BranchResolveFlag::NotTaken),
            2 => Some(BranchResolveFlag::Call),
            3 => Some(BranchResolveFlag::Return),
            4 => Some(BranchResolveFlag::Indirect),
            _ => unreachable!(),
        }
    }
}

// const ALLOCATED_CORE_COUNT: usize = if parameter::CACHE_HIERARCHY_FOR_HALF_OF_CORES {
//     parameter::CORE_COUNT / 2
// } else {
//     parameter::CORE_COUNT
// };

// static mut FETCH_UNIT: Lazy<UnsafeCell<fetch::FetchUnit<{ ALLOCATED_CORE_COUNT }>>> =
//     Lazy::new(|| {
//         let fetch_unit = fetch::FetchUnit::new();
//         UnsafeCell::new(fetch_unit)
//     });

static mut FETCH_UNIT: *mut fetch::FetchUnit<{ parameter::CORE_COUNT }> = std::ptr::null_mut();

unsafe extern "C" fn branch_resolved_cb(vcpu_index: u32, pc: u64, target: u64, flags: u32) {
    if parameter::MEASURE_HALF_OF_CORES && vcpu_index >= parameter::CORE_COUNT as u32 / 2 {
        return;
    }

    let result = BranchResolveFlag::from_u32(flags).unwrap();
    (*FETCH_UNIT).train(vcpu_index as usize, pc, result, target)
}

pub struct BranchPredictorPlugin {}

impl Plugin for BranchPredictorPlugin {
    fn init() {
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

    fn dump_snapshot(_name: &str) {
        //     for (core_id, f) in unsafe { &(*FETCH_UNIT).private_units }.iter().enumerate() {
        //         let mut file =
        //             std::fs::File::create(format!("{}/fetch_unit_{}.json", name, core_id)).unwrap();
        //         let json = serde_json::to_string_pretty(f).unwrap();
        //         file.write_all(json.as_bytes()).unwrap();
        //     }
    }

    fn serialize(name: &str) {
        // open a file
        let mut file = std::fs::File::create(format!("{}/fetch.json.zstd", name)).unwrap();

        let mut file = Encoder::new(&mut file, 0).unwrap();

        // write the content
        let json = serde_json::to_string_pretty(unsafe { &(*FETCH_UNIT) }).unwrap();
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
