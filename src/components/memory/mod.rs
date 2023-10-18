mod checkpoint;
mod mtr;
mod per_core_record;
mod ts_cache;
mod ts_model;
mod ts_set;

use std::fs;
use std::io::Write;

pub use per_core_record::TimestampSingleCoreMemoryHierarchy;
pub use ts_cache::TimestampCache;
pub use ts_cache::TimestampCacheMetaData;
pub use ts_model::TimestampMemoryHierarchy;
pub use ts_set::CacheReturnResult;
pub use ts_set::TimestampCacheLineStatus;
pub use ts_set::TimestampCacheSet;

use crate::CORE_COUNT;

use once_cell::sync::Lazy;

static PLUGIN: Lazy<
    TimestampMemoryHierarchy<
        { crate::PRI_CACHE_ASSO },
        { crate::PRI_CACHE_SET },
        { crate::SHARED_CACHE_ASSO },
        { crate::SHARED_CACHE_SET },
    >,
> = Lazy::new(|| TimestampMemoryHierarchy::new(crate::CORE_COUNT));

fn get_memory_ts() -> u128 {
    return std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u128;
}

// 1. init function.
#[inline]
pub unsafe fn init() {}

// 2. translation function.
#[inline]
pub fn on_instruction_cacheline_touched(vcpu_idx: u32, context: &crate::PluginFetchBlockContext) {
    PLUGIN.hierarchies(vcpu_idx as u8).access_memory(
        get_memory_ts() as usize,
        context.pa as usize,
        true,
        false,
    )
}

// 3. on memory access
#[inline]
pub fn on_data_cacheline_touched(vcpu_idx: u32, _: usize, paddr: usize, is_store: bool) {
    PLUGIN.hierarchies(vcpu_idx as u8).access_memory(
        get_memory_ts() as usize,
        paddr as usize,
        false,
        is_store,
    )
}

// 3. the function to export checkpoint. This function is guaranteed to be serial.
#[inline]
pub fn dump_snapshot() {
    let private_param = checkpoint::PrivateCacheParameters {
        l1i_sets: 32,
        l1i_associativity: 16,
        l1d_sets: 32,
        l1d_associativity: 16,
        l2_sets: crate::PRI_CACHE_SET,
        l2_associativity: crate::PRI_CACHE_ASSO,
        directory_associativity: crate::PRI_CACHE_ASSO * CORE_COUNT,
    };

    let mtr = PLUGIN.render_mtr::<{ crate::PRI_CACHE_SET }>();
    let mtr = mtr.prune_by_associativity(private_param.directory_associativity);
    let caches = PLUGIN.render_cache_hierarchy(&mtr, &private_param);
    let exported_json = serde_json::to_string_pretty(&caches).unwrap();
    let mut output = fs::File::create("./dumped.json").unwrap();
    output.write_all(exported_json.as_bytes()).unwrap();
}
