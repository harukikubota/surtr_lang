#!/usr/bin/env python3
"""Run the crate-local RT-5 collections probe in fresh release-test processes."""
import argparse
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess


SIZES = [16, 128, 1024]
PHASES = {"init", "save", "drop", "first_write", "second_write", "rollback",
          "chunk_shared", "chunk_unique"}
TEST_NAME = "vm::checkpoint_collections_tests::checkpoint_collections_profile"
ROOT = Path(__file__).resolve().parents[3]
RSS_SCOPE = "whole fresh test process high-water mark; not a per-phase peak or live bytes"


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def source_snapshot():
    probe = ROOT / "crates/eldr/src/vm/checkpoint_collections_tests.rs"
    runtime_paths = sorted(path for path in (ROOT / "crates/eldr/src").rglob("*.rs")
                           if not path.name.endswith("_tests.rs"))
    return {
        "probe_source_sha256": digest(probe),
        "production_sources_sha256": {
            str(path.relative_to(ROOT)): digest(path) for path in runtime_paths},
        "lock_sha256": digest(ROOT / "Cargo.lock"),
        "manifest_sha256": {str(path.relative_to(ROOT)): digest(path) for path in [
            ROOT / "Cargo.toml", ROOT / "crates/eldr/Cargo.toml"]},
        "git_diff_head_sha256": hashlib.sha256(subprocess.check_output(
            ["git", "diff", "HEAD", "--binary"], cwd=ROOT)).hexdigest(),
    }


def run_probe(binary, n, repeats):
    if platform.system() != "Darwin":
        raise RuntimeError("process RSS measurement requires macOS /usr/bin/time -l")
    env = dict(os.environ, SURTR_COLLECTION_PROFILE="1", SURTR_COLLECTION_N=str(n),
               SURTR_COLLECTION_REPEATS=str(repeats))
    result = subprocess.run(["/usr/bin/time", "-l", str(binary), "--exact", TEST_NAME, "--nocapture",
                             "--test-threads=1"], cwd=ROOT, env=env,
                            capture_output=True, text=True, check=True)
    rows = [json.loads(line) for line in result.stderr.splitlines() if line.startswith("{")]
    rss_values = re.findall(r"^\s*(\d+)\s+maximum resident set size\s*$", result.stderr, re.MULTILINE)
    if len(rss_values) != 1 or int(rss_values[0]) <= 0:
        raise RuntimeError(f"missing or invalid macOS process peak RSS: {result.stderr}")
    if len(rows) != len(PHASES) or {row.get("phase") for row in rows} != PHASES:
        raise RuntimeError(f"unexpected phases: {result.stdout}\n{result.stderr}")
    for row in rows:
        values = row.get("samples_ns")
        if (type(row.get("n")) is not int or row["n"] != n
                or type(row.get("repeats")) is not int or row["repeats"] != repeats
                or type(row.get("ns")) is not int or row["ns"] < 0
                or not isinstance(values, list) or len(values) != repeats
                or any(type(value) is not int or value < 0 for value in values)
                or sum(values) != row["ns"]):
            raise RuntimeError(f"probe parameters or aggregate mismatch: {row}")
        row["process_peak_rss_bytes"] = int(rss_values[0])
    return rows


def summarize(groups, repeats, variants=False):
    summaries = []
    for key, rows in sorted(groups.items()):
        values = [row["ns"] for row in rows]
        rss_values = [row["process_peak_rss_bytes"] for row in rows]
        row = {"n": key[0], "phase": key[1], "repeats": repeats,
               "samples": len(values), "ns": {"median": statistics.median(values),
                                              "min": min(values), "max": max(values)}}
        row["process_peak_rss_bytes"] = {"median": statistics.median(rss_values),
                                         "min": min(rss_values), "max": max(rss_values)}
        if variants:
            row["variant"] = key[2]
        summaries.append(row)
    return summaries


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
    snapshot = source_snapshot()
    metadata = {
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "source_snapshot_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "root": str(ROOT),
        "git_status_porcelain": subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=ROOT, text=True),
        "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
        "platform": platform.platform(), "machine": platform.machine(),
        "build_profile": "release unit-test binary", "test_name": TEST_NAME,
        "binary": str(binary), "binary_sha256": digest(binary),
        "runner_sha256": digest(Path(__file__).resolve()),
        "sizes": SIZES, "samples": args.samples, "repeats": args.repeats,
        "allocator_counting": False, "rss_measured": True, "rss_scope": RSS_SCOPE,
        "rss_command": "/usr/bin/time -l", "rss_unit": "bytes (macOS)",
        **snapshot,
    }
    groups = collections.defaultdict(list)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix(".jsonl").open("w") as raw:
        for n in SIZES:
            for sample in range(args.samples):
                for row in run_probe(binary, n, args.repeats):
                    row["sample"] = sample
                    raw.write(json.dumps(row, sort_keys=True) + "\n")
                    groups[(n, row["phase"])].append(row)
            print(f"n={n}: {args.samples} fresh processes", flush=True)
    if source_snapshot() != snapshot or digest(binary) != metadata["binary_sha256"]:
        raise RuntimeError("source or binary changed during measurement; summary not saved")
    args.output.with_suffix(".summary.json").write_text(json.dumps({
        "metadata": metadata, "summaries": summarize(groups, args.repeats)}, indent=2) + "\n")


if __name__ == "__main__":
    main()
