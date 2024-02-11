/// This file contains the parameters for the whole plugin.
/// All of them are compilation constants that the the compiler can propagate them during compilation.

use plugin_helper::PluginHelper;

/**
 * CORE_COUNT
 * 
 * Number of vCPUs of QEMU. 
 */
pub const CORE_COUNT: usize = 1;

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
 * PRI_CACHE_ASSO
 * 
 * The associativity of the private cache for traffic recording.
 */
pub const PRI_CACHE_ASSO: usize = 16; // with 16 and 64, each cache set is 1KB.


/**
 * PRI_CACHE_SET
 * 
 * The number of sets of the private cache for traffic recording.
 */
pub const PRI_CACHE_SET: usize = 2048; 


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
    // _dm : crate::DelayedMemoryPlugin,
}

/**
 * whether to enable the statistics collection.
 */

pub const ENABLE_STATISTICS: bool = false;