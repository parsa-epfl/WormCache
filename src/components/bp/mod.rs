mod fetch;

mod aarch64;
mod callbacks;
use super::Plugin;
use crate::qemu_api;
use once_cell::sync::Lazy;
use std::cell::UnsafeCell;
use std::io::Write;

// Use Arena to allocate the BranchMetaData.
// https://crates.io/crates/bumpalo

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
enum BranchResolveFlag {
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

static mut FETCH_UNIT: Lazy<UnsafeCell<fetch::FetchUnit>> = Lazy::new(|| {
    let fetch_unit = fetch::FetchUnit::new();
    UnsafeCell::new(fetch_unit)
});

unsafe extern "C" fn branch_resolved_cb(vcpu_index: u32, pc: u64, target: u64, flags: u32) {
    let result = BranchResolveFlag::from_u32(flags).unwrap();
    FETCH_UNIT
        .get_mut()
        .train(vcpu_index as usize, pc, result, target)
}

pub struct BranchPredictorPlugin {}

impl Plugin for BranchPredictorPlugin {
    fn init() {
        println!("BranchPredictorPlugin initialized.");

        assert!(unsafe {
            qemu_api::qemu_plugin_register_vcpu_branch_resolved_cb(Some(branch_resolved_cb))
        });

        unsafe {
            Lazy::force(&FETCH_UNIT);
        }
    }

    unsafe fn on_translation(_: *mut crate::qemu_api::qemu_plugin_tb) {
        // The callback is already inserted into the TB during init.
    }

    fn dump_snapshot() {
        for (core_id, f) in unsafe { &(*FETCH_UNIT.get()).private_units }
            .iter()
            .enumerate()
        {
            let mut file = std::fs::File::create(format!("fetch_unit_{}.json", core_id)).unwrap();
            let json = serde_json::to_string_pretty(f).unwrap();
            file.write_all(json.as_bytes()).unwrap();
        }
    }
}
