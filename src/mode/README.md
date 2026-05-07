# Modes

## Chronic modes (`src/mode/`)

Controlled via `-plugin-arg worm_cache=mode=MODE`.

- **normal** (default): No periodic snapshot. If `quit_threshold_ns=N` is provided, accumulates simulated cycles via the periodic check callback and quits when reaching the threshold, dumping `statistics.final.csv` and timing breakdown.
- **warm**: Periodic warm-up snapshots based on cycle count. Requires `init_threshold`, `interval`, `count`.

### Normal options

| Option | Description |
|---|---|
| `quit_threshold_ns` | Cycle threshold to auto-quit |

### Warm options

| Option | Description |
|---|---|
| `init_threshold` | First snapshot cycle threshold |
| `interval` | Cycle interval between snapshots |
| `count` | Number of snapshots before quitting |
| `prefix` | Snapshot name prefix (default: `snapshot`) |
| `init_index` | Initial snapshot index (default: 0) |
| `no_qemu_snapshot` | Skip QEMU snapshot, serialize plugins only (default: false) |

## Cache hierarchy modes (`src/components/cache_hierarchy/parallel_hierarchy/`)

Controlled via the same `mode` option but scoped to the parallel cache hierarchy plugin.

- **pure_fill**: Takes a snapshot once the shared cache reaches a given warm ratio, then exits. Requires `prefix` and `warm_ratio` (0.0–1.0, default: 1.0).

