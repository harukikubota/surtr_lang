#!/usr/bin/env python3
"""Run the crate-local RT-5 probe in independent release-test processes."""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--repeats", type=int, default=32)
    args = parser.parse_args()
    if min(args.samples, args.repeats) < 1:
        parser.error("samples and repeats must be positive")
    binary = args.binary.resolve(strict=True)
    root = Path(__file__).resolve().parents[3]
    source = root / "crates/eldr/src/vm/checkpoint_metadata_tests.rs"
    metadata = {
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "build_profile": "release unit-test binary",
        "binary": str(binary), "binary_sha256": digest(binary),
        "probe_source_sha256": digest(source),
        "production_sources_sha256": {str(p.relative_to(root)): digest(p) for p in [
            root / "crates/eldr/src/vm.rs", root / "crates/eldr/src/vm/process_continuation.rs"]},
        "lock_sha256": digest(root / "Cargo.lock"),
        "runner_sha256": digest(Path(__file__).resolve()),
        "samples": args.samples, "repeats": args.repeats,
        "allocator_counting": False, "rss_measured": False,
    }
    groups = collections.defaultdict(list)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix(".jsonl").open("w") as raw:
        for n in [16, 128, 1024]:
            for sample in range(args.samples):
                env = dict(os.environ, SURTR_METADATA_PROFILE="1", SURTR_METADATA_N=str(n),
                           SURTR_METADATA_REPEATS=str(args.repeats))
                result = subprocess.run([str(binary), "--exact",
                    "vm::checkpoint_metadata_tests::process_metadata_profile", "--nocapture", "--test-threads=1"],
                    cwd=root, env=env, capture_output=True, text=True, check=True)
                rows = [json.loads(line) for line in result.stderr.splitlines() if line.startswith("{")]
                expected = {"save", "drop", "register", "rollback", "chunk_shared", "chunk_unique"}
                if len(rows) != len(expected) or {r["phase"] for r in rows} != expected:
                    raise RuntimeError(f"unexpected phases: {result.stdout}\n{result.stderr}")
                for row in rows:
                    if row["n"] != n or row["repeats"] != args.repeats:
                        raise RuntimeError("probe parameters do not match")
                    row["sample"] = sample
                    raw.write(json.dumps(row, sort_keys=True) + "\n")
                    groups[(n, row["phase"])].append(row)
            print(f"n={n}: {args.samples} samples", flush=True)
    summaries = []
    for (n, phase), rows in sorted(groups.items()):
        values = [r["ns"] for r in rows]
        summaries.append({"n": n, "phase": phase, "repeats": args.repeats,
                          "samples": len(rows), "ns": {"median": statistics.median(values),
                          "min": min(values), "max": max(values)}})
    args.output.with_suffix(".summary.json").write_text(
        json.dumps({"metadata": metadata, "summaries": summaries}, indent=2) + "\n")


if __name__ == "__main__":
    main()
