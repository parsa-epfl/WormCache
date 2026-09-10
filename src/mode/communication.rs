// Mode: communication
//
// This mode is implemented in the parallel cache hierarchy component:
//   src/components/communication/mod.rs
//
// It tracks the shared-memory communication as well as the interrupt interval.
//
// Options:
//   mode=communication   (scoped to the parallel cache hierarchy plugin)
//   quit_threshold_ns=Xns (number of nanosecond to quit)
