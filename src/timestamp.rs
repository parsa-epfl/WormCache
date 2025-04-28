// generate timestamp.

use crate::util::get_monotonic_ts;

static mut QEMU_INITIALIZING_TIMESTAMP: u64 = 0;
static mut CHECKPOINT_TIMESTAMP: u64 = 0;


pub fn initialize() {
    unsafe {
        QEMU_INITIALIZING_TIMESTAMP = get_monotonic_ts();
    }
}

// Save the starting timestamp.
fn serialize() {}

// Load the starting timestamp.
fn deserialize() {}

#[inline(always)]
pub fn get_ts() -> u64 {
    let current_ts = get_monotonic_ts();
    return current_ts - unsafe { QEMU_INITIALIZING_TIMESTAMP } + unsafe { CHECKPOINT_TIMESTAMP };
}
