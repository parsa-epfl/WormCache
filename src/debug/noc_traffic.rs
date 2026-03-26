use std::cell::UnsafeCell;
use std::io::Write;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::parameter::{CORE_COUNT, ENABLE_STATISTICS};

#[derive(Clone, Copy)]
pub enum AccessReason {
    Directory                                    = 0,
    LLC                                          = 1,
    DRAM                                         = 2,
    PrivateCacheDemandBlock                      = 3,
    PrivateCacheInvalidateDueToGetX              = 4,
    PrivateCacheDowngrade                        = 5,
    PrivateCacheInvalidateDueToDirectoryEviction = 6,
}

impl AccessReason {
    pub const COUNT: usize = 7;
}

#[repr(align(64))]
struct PerAccessorCounts {
    counts: [[u64; AccessReason::COUNT]; CORE_COUNT],
}

pub struct NocTraffic {
    per_accessor: [UnsafeCell<PerAccessorCounts>; CORE_COUNT],
}

unsafe impl Sync for NocTraffic {}

impl NocTraffic {
    #[inline]
    fn record(&self, accessor_id: u32, destination_id: u32, reason: AccessReason) {
        if ENABLE_STATISTICS {
            unsafe {
                (*self.per_accessor[accessor_id as usize].get()).counts[destination_id as usize]
                    [reason as usize] += 1;
            }
        }
    }

    pub fn get_header() -> String {
        "destination_id,accessor_id,directory,llc,dram,private_cache_demand_block,\
         private_cache_invalidate_due_to_getx,private_cache_downgrade,\
         private_cache_invalidate_due_to_directory_eviction"
            .to_string()
    }

    fn save_to_csv_inner(&self, file_name: &str) {
        let mut file = std::fs::File::create(file_name).unwrap();
        file.write_fmt(format_args!("{}\n", Self::get_header()))
            .unwrap();
        for destination_id in 0..CORE_COUNT {
            for accessor_id in 0..CORE_COUNT {
                unsafe {
                    let counts =
                        &(*self.per_accessor[accessor_id].get()).counts[destination_id];
                    file.write_fmt(format_args!(
                        "{},{},{},{},{},{},{},{},{}\n",
                        destination_id,
                        accessor_id,
                        counts[AccessReason::Directory as usize],
                        counts[AccessReason::LLC as usize],
                        counts[AccessReason::DRAM as usize],
                        counts[AccessReason::PrivateCacheDemandBlock as usize],
                        counts[AccessReason::PrivateCacheInvalidateDueToGetX as usize],
                        counts[AccessReason::PrivateCacheDowngrade as usize],
                        counts[AccessReason::PrivateCacheInvalidateDueToDirectoryEviction as usize],
                    ))
                    .unwrap();
                }
            }
        }
        file.flush().unwrap();
    }

    #[inline]
    pub fn global_record(accessor_id: u32, destination_id: u32, reason: AccessReason) {
        global().record(accessor_id, destination_id, reason);
    }

    pub fn save_to_csv(file_name: &str) {
        global().save_to_csv_inner(file_name);
    }
}

static GLOBAL_NOC_TRAFFIC: AtomicPtr<NocTraffic> = AtomicPtr::new(std::ptr::null_mut());

#[inline]
fn global() -> &'static NocTraffic {
    unsafe { &*GLOBAL_NOC_TRAFFIC.load(Ordering::Relaxed) }
}

/// Allocates the global NocTraffic directly on the heap.
/// Must be called once before any recording begins.
pub fn init() {
    let ptr = unsafe {
        Box::into_raw(Box::<NocTraffic>::new_zeroed().assume_init())
    };
    GLOBAL_NOC_TRAFFIC.store(ptr, Ordering::Relaxed);
}
