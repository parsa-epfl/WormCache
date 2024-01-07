mod tage;

mod aarch64;
mod callbacks;
use super::Plugin;
use crate::qemu_api;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::ffi;
use std::sync::Mutex;
use std::io::Write;

struct BranchMetaData {
    pc: u64,
    instruction: u32,
    bb_bias: u32,
}

// Use Arena to allocate the BranchMetaData.
// https://crates.io/crates/bumpalo
static mut BRANCH_METADATA: Lazy<Mutex<HashMap<usize, Box<BranchMetaData>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug)]
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
            _ => None,
        }
    }
}

static mut LOG_FILE: Lazy<Mutex<std::fs::File>> = Lazy::new(|| {
    let file = std::fs::File::create("branch_predictor.log").unwrap();
    Mutex::new(file)
});

unsafe extern "C" fn branch_resolved_cb(_: u32, pc: u64, target: u64, flags: u32) {
    let mut f = LOG_FILE.lock().unwrap();
    writeln!(f, "branch_resolved_cb: pc: {:x}, target: {:x}, flags: {:?}", pc, target, BranchResolveFlag::from_u32(flags).unwrap()).unwrap();
}

pub struct BranchPredictorPlugin {}

impl Plugin for BranchPredictorPlugin {
    fn init() {
        println!("BranchPredictorPlugin initialized.");
        unsafe {
            BRANCH_METADATA.lock().unwrap().clear();
        }

        assert!(unsafe {
            qemu_api::qemu_plugin_register_vcpu_branch_resolved_cb(Some(branch_resolved_cb))
        });
    }

    unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
    }

    fn dump_snapshot() {}
}
