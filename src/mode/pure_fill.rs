// Mode: pure_fill
//
// This mode is implemented in the parallel cache hierarchy component:
//   src/components/cache_hierarchy/parallel_hierarchy/pure_fill.rs
//
// It monitors the shared cache warm-up progress via a periodic check callback.
// Once the cache reaches the configured warm_ratio, it takes a snapshot and exits.
//
// Options:
//   mode=pure_fill   (scoped to the parallel cache hierarchy plugin)
//   prefix           Snapshot name prefix
//   warm_ratio       Fraction of cache sets to warm before snapshot (0.0–1.0)
