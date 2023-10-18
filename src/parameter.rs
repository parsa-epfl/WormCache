/// This file contains the parameters for the whole plugin.
/// All of them are compilation constants that the the compiler can propagate them during compilation.

/**
 * CORE_COUNT
 * 
 * Number of vCPUs of QEMU. 
 */
pub const CORE_COUNT: usize = 16;

/**
 * CACHE_LINE_SIZE
 * 
 * The size of a cache line, in number of bytes.
 */

pub const CACHE_LINE_SIZE: usize = 64;


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

