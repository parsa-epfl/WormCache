/// This file contains the parameters for the whole plugin.
/// All of them are compilation constants that the the compiler can propagate them during compilation.
use plugin_helper::PluginHelper;

/**
 * CORE_COUNT
 *
 * Number of vCPUs of QEMU.
 */
pub const CORE_COUNT: usize = 4;

/**
 * CACHE_LINE_SIZE
 *
 * The size of a cache line, in number of bytes.
 */

pub const CACHE_LINE_SIZE: usize = 64;

/**
 * TLB_ASSO
 *
 * The associativity of the private & last-level TLB.
 */
pub const TLB_ASSO: usize = 16;

/**
 * TLB_SET
 *
 * The number of sets of the private & last-level TLB.
 */

pub const TLB_SET: usize = 1024;

/**
 * USE_UNIFIED_CACHE
 *
 * Whether to use the unified private cache.
 * If true, the private instruction cache and the private data cache are unified.
 * If false, the private instruction cache and the private data cache are separated, i.e., the Harvard architecture.
 */
pub const USE_UNIFIED_CACHE: bool = false;

/**
 * PRI_CACHE_ASSO
 *
 * The associativity of the private cache.
 * This parameter is only used when the unified private cache is enabled.
 */
pub const UNIFIED_PRI_CACHE_ASSO: usize = 16;
/**
 * PRI_CACHE_SET
 *
 * The number of sets of the private cache.
 * This parameter is only used when the unified private cache is enabled.
 */
pub const UNIFIED_PRI_CACHE_SET: usize = 2048;

/**
 * HARVARD_PRI_I_CACHE_ASSO
 *
 * The associativity of the private instruction cache.
 * This parameter is only used when the unified private cache is disabled.
 */
pub const HARVARD_PRI_I_CACHE_ASSO: usize = 16;

/**
 * HARVARD_PRI_I_CACHE_SET
 *
 * The number of sets of the private instruction cache.
 * This parameter is only used when the unified private cache is disabled.
 */
pub const HARVARD_PRI_I_CACHE_SET: usize = 2048;

/**
 * HARVARD_PRI_D_CACHE_ASSO
 *
 * The associativity of the private data cache.
 * This parameter is only used when the unified private cache is disabled.
 */
pub const HARVARD_PRI_D_CACHE_ASSO: usize = 16;

/**
 * HARVARD_PRI_D_CACHE_SET
 *
 * The number of sets of the private data cache.
 * This parameter is only used when the unified private cache is disabled.
 */
pub const HARVARD_PRI_D_CACHE_SET: usize = 2048;

/**
 * SHARED_CACHE_ASSO
 *
 * The associativity of the shared cache for traffic recording.
 */
pub const SHARED_CACHE_ASSO: usize = 16; // with 16 and 64, each cache set is 1KB.

/**
 * SHARED_CACHE_SET
 *
 * The number of sets of the shared cache for traffic recording.
 */
pub const SHARED_CACHE_SET: usize = 1024 * 1024; // 1GB shared cache.

/**
 * SHARED_CACHE_EXCLUSIVE
 *
 * Whether the shared cache is exclusive.
 * True if it is exclusive, false if it is non-inclusive.
 * Currently, we don't support inclusive shared cache.
 */
pub const SHARED_CACHE_EXCLUSIVE: bool = true;

/**
 * BP_GSHARE_SET
 *
 * The number of sets of the gshare branch predictor.
 */
pub const BP_GSHARE_SET: usize = 2048;

/**
 * BP_RAS_COUNT
 */
pub const BP_RAS_COUNT: usize = 32;

/**
 * The list of plugins.
 */
use crate::components::Plugin;

#[derive(PluginHelper)]
pub struct PluginList {
    _pb: crate::BranchPredictorPlugin,
    // _ts_m : crate::TimeStampedMemoryPlugin,
    _vt: crate::VirtualTimePlugin,
    _mk: crate::MarkerPlugin,
    // to : crate::TouchOnePlugin,
    // pwl : crate::PageWalkLoggerPlugin,
    _lm: crate::LockedMemoryPlugin,
    // tr : crate::TracePlugin,
    // _mtrec: crate::MTRMemoryPlugin,
    // _dm: crate::DelayedMemoryPlugin,
}

/**
 * Whether to enable the statistics collection.
 */

pub const ENABLE_STATISTICS: bool = false;

/**
 * Whether to enable the cache line history.
 *
 * This is used to record the cache line coherence history so that you can debug the cache coherence protocol.
 */
pub const ENABLE_CACHE_LINE_HISTORY: bool = true;
