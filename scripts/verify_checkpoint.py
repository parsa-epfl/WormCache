#!/usr/bin/env python3
"""Verify that sequential and parallel checkpoint formats are equivalent.

Steps:
  1. Scan for matching *.uarch/ and *.uarch_par/ folder pairs.
  2. Run checkpoint_check_equivalence to binary-compare them.
  3. Run checkpoint_conversion on both to produce Flexus JSON output.
  4. Compare the resulting per-core JSON files byte-for-byte.

Pairs are processed in parallel across available CPU cores.

Usage:
    python3 scripts/verify_checkpoint.py <base_dir> <flexus_config.json>
    [--skip-equivalence] [--skip-conversion] [--bin-dir <dir>]
    [--jobs N] [--keep-out]
"""

import argparse
import os
import shutil
import subprocess
import sys
import threading
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

_print_lock = threading.Lock()


def locked_print(*args, **kwargs):
    with _print_lock:
        print(*args, **kwargs)


def find_pairs(base_dir: Path) -> list[tuple[Path, Path, str]]:
    """Find matching .uarch / .uarch_par folder pairs.

    Returns list of (seq_dir, par_dir, snapshot_name).
    """
    pairs = []
    seq_dirs = sorted(base_dir.glob("*.uarch"))
    for seq in seq_dirs:
        if not seq.is_dir():
            continue
        name = seq.name.removesuffix(".uarch")
        par = seq.with_name(f"{name}.uarch_par")
        if par.is_dir():
            pairs.append((seq, par, name))
    return pairs


def run_equivalence(bin_dir: Path, seq_dir: Path, par_dir: Path) -> bool:
    """Run checkpoint_check_equivalence binary. Returns True if equivalent."""
    exe = bin_dir / "checkpoint_check_equivalence"
    result = subprocess.run(
        [str(exe), str(seq_dir), str(par_dir)],
        capture_output=True, text=True,
    )
    locked_print(result.stdout, end="")
    if result.returncode != 0:
        locked_print(result.stderr, end="", file=sys.stderr)
    return result.returncode == 0


def run_conversion(
    bin_dir: Path, checkpoint_dir: Path, config: Path, output_dir: Path,
    resizing: bool = True,
) -> bool:
    """Run checkpoint_conversion binary. Returns True on success."""
    exe = bin_dir / "checkpoint_conversion"
    args = [
        str(exe),
        str(checkpoint_dir),
        str(config),
        str(output_dir),
        str(resizing).lower(),
    ]
    result = subprocess.run(args, capture_output=True, text=True)
    locked_print(result.stdout, end="")
    if result.returncode != 0:
        locked_print(f"  FAILED:\n{result.stderr}", end="", file=sys.stderr)
    return result.returncode == 0


def compare_output_dirs(seq_out: Path, par_out: Path, name: str) -> bool:
    """Compare per-core JSON files from two conversion output directories."""
    seq_files = sorted(f for f in seq_out.rglob("*.json") if f.is_file())
    par_files = sorted(f for f in par_out.rglob("*.json") if f.is_file())

    seq_names = {f.name for f in seq_files}
    par_names = {f.name for f in par_files}

    only_seq = seq_names - par_names
    only_par = par_names - seq_names
    if only_seq:
        locked_print(f"  [{name}] Files only in sequential: {only_seq}")
    if only_par:
        locked_print(f"  [{name}] Files only in parallel: {only_par}")

    ok = True
    seq_by_name = {f.name: f for f in seq_files}
    par_by_name = {f.name: f for f in par_files}
    for fname in sorted(seq_names & par_names):
        a = seq_by_name[fname].read_bytes()
        b = par_by_name[fname].read_bytes()
        if a != b:
            locked_print(f"  [{name}] DIFFER: {fname}")
            ok = False

    if ok:
        common = len(seq_names & par_names)
        locked_print(f"  [{name}] {common} JSON files EQUIVALENT")
    return ok


def process_pair(
    bin_dir: Path, base_dir: Path, config: Path,
    seq_dir: Path, par_dir: Path, name: str,
    skip_equivalence: bool, skip_conversion: bool, keep_out: bool,
) -> bool:
    """Process a single checkpoint pair. Returns True on pass."""
    ok = True

    if not skip_equivalence:
        if not run_equivalence(bin_dir, seq_dir, par_dir):
            ok = False

    if not skip_conversion:
        seq_out = base_dir / f"_conv_seq_{name}"
        par_out = base_dir / f"_conv_par_{name}"
        seq_out.mkdir(parents=True, exist_ok=True)
        par_out.mkdir(parents=True, exist_ok=True)

        ok_seq = run_conversion(bin_dir, seq_dir, config, seq_out)
        ok_par = run_conversion(bin_dir, par_dir, config, par_out)

        if ok_seq and ok_par:
            if not compare_output_dirs(seq_out, par_out, name):
                ok = False
        else:
            ok = False

        if not keep_out:
            for d in (seq_out, par_out):
                if d.exists():
                    shutil.rmtree(d)

    return ok


def main():
    parser = argparse.ArgumentParser(
        description="Verify sequential vs parallel checkpoints.",
    )
    parser.add_argument("base_dir", type=Path,
                        help="Directory containing .uarch and .uarch_par folders")
    parser.add_argument("flexus_config", type=Path,
                        help="Flexus JSON configuration file")
    parser.add_argument("--skip-equivalence", action="store_true",
                        help="Skip binary equivalence check")
    parser.add_argument("--skip-conversion", action="store_true",
                        help="Skip checkpoint conversion step")
    parser.add_argument("--bin-dir", type=Path,
                        default=Path("/home/sqlin/pf.ipc-model/WormCache/target/release"),
                        help="Directory containing the built binaries")
    parser.add_argument("--keep-out", action="store_true",
                        help="Keep conversion output dirs")
    parser.add_argument("--jobs", "-j", type=int, default=None,
                        help="Number of parallel jobs (default: CPU count)")
    args = parser.parse_args()

    bin_dir = args.bin_dir.resolve()
    base_dir = args.base_dir.resolve()
    config = args.flexus_config.resolve()

    if not config.is_file():
        print(f"Config file not found: {config}", file=sys.stderr)
        sys.exit(1)

    pairs = find_pairs(base_dir)
    if not pairs:
        print("No .uarch / .uarch_par pairs found.", file=sys.stderr)
        sys.exit(1)

    print(f"Found {len(pairs)} pair(s):")
    for seq, par, name in pairs:
        print(f"  {name}: {seq.name}  <->  {par.name}")
    print()

    max_workers = args.jobs if args.jobs else os.cpu_count()
    all_ok = True

    with ThreadPoolExecutor(max_workers=max_workers) as executor:
        futures = {}
        for seq_dir, par_dir, name in pairs:
            locked_print(f"[{name}]")
            fut = executor.submit(
                process_pair,
                bin_dir, base_dir, config,
                seq_dir, par_dir, name,
                args.skip_equivalence, args.skip_conversion, args.keep_out,
            )
            futures[fut] = name

        for fut in as_completed(futures):
            name = futures[fut]
            if not fut.result():
                all_ok = False
                locked_print(f"[{name}] FAILED", file=sys.stderr)
            locked_print()

    if all_ok:
        print("ALL OK")
    else:
        print("SOME CHECKS FAILED", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
