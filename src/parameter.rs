/// This file contains the parameters for the whole plugin.
/// All of them are compilation constants that the the compiler can propagate them during compilation.
use plugin_helper::PluginHelper;

/**
 * CORE_COUNT
 *
 * Number of vCPUs of QEMU.
 */
pub const CORE_COUNT: usize = 8;

/**
 * CACHE_HIERARCHY_FOR_HALF_OF_CORES
 *
 * Whether to use the cache hierarchy for half of the cores [0, CORE_COUNT/2)
 *
 * This option is specially for profiling online services images.
 *
 * TODO: This is not the best way to implement this feature. We should use a more flexible way to specify the cores.
 *
 * This option impact both the cache hierarchy component and the branch predictor component.
 */
pub const CACHE_HIERARCHY_FOR_HALF_OF_CORES: bool = true;

// An assertion checker to make sure the CORE_COUNT is even if we use the CACHE_HIERARCHY_FOR_HALF_OF_CORES.
static_assertions::const_assert!(!CACHE_HIERARCHY_FOR_HALF_OF_CORES || CORE_COUNT % 2 == 0);

/**
 * CACHE_LINE_SIZE
 *
 * The size of a cache line, in number of bytes.
 */

pub const CACHE_LINE_SIZE: usize = 64;
static_assertions::const_assert!(CACHE_LINE_SIZE.is_power_of_two());

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
static_assertions::const_assert!(TLB_SET.is_power_of_two());

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
static_assertions::const_assert!(UNIFIED_PRI_CACHE_SET.is_power_of_two());

/**
 * HARVARD_PRI_I_CACHE_ASSO
 *
 * The associativity of the private instruction cache.
 * This parameter is only used when the unified private cache is disabled.
 */
pub const HARVARD_PRI_I_CACHE_ASSO: usize = 4;

/**
 * HARVARD_PRI_I_CACHE_SET
 *
 * The number of sets of the private instruction cache.
 * This parameter is only used when the unified private cache is disabled.
 */
pub const HARVARD_PRI_I_CACHE_SET: usize = 256;
static_assertions::const_assert!(HARVARD_PRI_I_CACHE_SET.is_power_of_two());

/**
 * HARVARD_PRI_D_CACHE_ASSO
 *
 * The associativity of the private data cache.
 * This parameter is only used when the unified private cache is disabled.
 */
pub const HARVARD_PRI_D_CACHE_ASSO: usize = 4;

/**
 * HARVARD_PRI_D_CACHE_SET
 *
 * The number of sets of the private data cache.
 * This parameter is only used when the unified private cache is disabled.
 */
pub const HARVARD_PRI_D_CACHE_SET: usize = 256;
static_assertions::const_assert!(HARVARD_PRI_D_CACHE_SET.is_power_of_two());

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
pub const SHARED_CACHE_SET: usize = 4 * 1024; // 1GB shared cache.
static_assertions::const_assert!(SHARED_CACHE_SET.is_power_of_two());

/**
 * SHARED_CACHE_EXCLUSIVE
 *
 * Whether the shared cache is exclusive.
 * True if it is exclusive, false if it is non-inclusive.
 * Currently, we don't support inclusive shared cache.
 */
pub const SHARED_CACHE_EXCLUSIVE: bool = false;

/**
 * BP_GSHARE_SET
 *
 * The number of sets of the gshare branch predictor.
 */
pub const BP_GSHARE_SET: usize = 2048;
static_assertions::const_assert!(BP_GSHARE_SET.is_power_of_two());

/**
 * BP_RAS_COUNT
 */
pub const BP_RAS_COUNT: usize = 32;

/**
 * HOST_TIME_SCALE
 *
 * The denominator of taking host time to advance CPU clock.
 *
 * The virtual_time plugin overrides the function to calculate the CPU clock.
 * Each time the QEMU polls the CPU clock, the plugin calculates the CPU clock based on the instruction count.
 * However, when the instruction count is zero, the plugin uses the escaped real time to calculate the CPU clock.
 * The real time is calculated by the host system, which escapes very fast for the heavily instrumented QEMU.
 * Therefore, we need to scale the real time to make the CPU clock advance slower.
 *
 * For example, if the host time scale is 1000, the CPU clock advances 1 ns for every 1000 ns of the real time.
 *
 * Please set this value to 1 if you want to use the real time directly. This is helpful when only virtual time is used.
 */
pub const HOST_TIME_SCALE: usize = 1000;

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
    _lm: crate::ParallelCacheHierarchyPlugin,
    // tr : crate::TracePlugin,
    // _mtrec: crate::MTRMemoryPlugin,
    // _dm: crate::DelayedMemoryPlugin,
}

/**
 * Whether to enable the statistics collection.
 */
pub const ENABLE_STATISTICS: bool = true;

//////////////////////////////////////////////////////////////
/// The following parameters are for debugging and testing.
/// They are not supposed to be played for production use.
//////////////////////////////////////////////////////////////

/**
 * Whether to enable the cache line history.
 *
 * This is used to record the cache line coherence history so that you can debug the cache coherence protocol.
 */
pub const ENABLE_CACHE_LINE_HISTORY: bool = false;

/**
 * Whether to disable precise coherence message reconstruction.
 *
 * This option is for testing the accuracy of the functional warming model.
 */
pub const DISABLE_PRECISE_COHERENCE_STATE_RECONSTRUCTION: bool = false;
