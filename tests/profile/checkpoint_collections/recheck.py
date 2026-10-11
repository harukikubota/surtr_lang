#!/usr/bin/env python3
"""Recheck saved RT-5 collections binaries with balanced AB/BA process order."""
import argparse
import collections
import datetime
import json
from pathlib import Path
import platform
import subprocess
import sys

sys.dont_write_bytecode = True
from run import PHASES, RSS_SCOPE, SIZES, TEST_NAME, digest, run_probe, summarize


def read_build(path):
    document = json.loads(path.read_text())
    metadata = document["metadata"]
    required = {"source_commit", "source_snapshot_at", "root", "git_status_porcelain",
                "git_diff_head_sha256", "production_sources_sha256", "probe_source_sha256",
                "lock_sha256", "manifest_sha256", "rustc", "platform", "machine",
                "build_profile", "test_name", "binary", "binary_sha256", "runner_sha256",
                "sizes", "samples", "repeats", "allocator_counting", "rss_measured",
                "rss_scope", "rss_command", "rss_unit"}
    if not required.issubset(metadata):
        raise RuntimeError(f"missing build metadata in {path}")
    if (metadata["test_name"] != TEST_NAME or metadata["sizes"] != SIZES
            or metadata["build_profile"] != "release unit-test binary"
            or type(metadata["samples"]) is not int or metadata["samples"] < 1
            or type(metadata["repeats"]) is not int or metadata["repeats"] < 1
            or metadata["allocator_counting"] is not False or metadata["rss_measured"] is not True
            or metadata["rss_scope"] != RSS_SCOPE or metadata["rss_command"] != "/usr/bin/time -l"
            or metadata["rss_unit"] != "bytes (macOS)"):
        raise RuntimeError(f"unexpected measurement conditions in {path}")
    binary = Path(metadata["binary"])
    if not binary.is_absolute() or digest(binary) != metadata["binary_sha256"]:
        raise RuntimeError(f"saved binary path or hash mismatch in {path}")
    rows = document["summaries"]
    expected = {(n, phase) for n in SIZES for phase in PHASES}
    if len(rows) != len(expected) or {(row["n"], row["phase"]) for row in rows} != expected:
        raise RuntimeError(f"unexpected summary phases in {path}")
    for row in rows:
        if row["samples"] != metadata["samples"] or row["repeats"] != metadata["repeats"]:
            raise RuntimeError(f"summary parameters differ from metadata in {path}")
    return metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True, help="before summary JSON")
    parser.add_argument("--after", type=Path, required=True, help="after summary JSON")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=10)
    args = parser.parse_args()
    if args.samples < 2 or args.samples % 2:
        parser.error("samples must be positive and even to balance execution order")
    inputs = {label: read_build(path) for label, path in [
        ("before", args.before), ("after", args.after)]}
    for field in ["lock_sha256", "rustc", "platform", "machine", "build_profile",
                  "test_name", "sizes", "repeats", "samples", "runner_sha256",
                  "rss_scope", "rss_command", "rss_unit"]:
        if inputs["before"][field] != inputs["after"][field]:
            raise RuntimeError(f"before/after {field} differ")
    current = {"rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
               "platform": platform.platform(), "machine": platform.machine()}
    for field, value in current.items():
        if value != inputs["before"][field]:
            raise RuntimeError(f"current {field} differs from recorded measurement environment")
    repeats = inputs["before"]["repeats"]
    groups = collections.defaultdict(list)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix(".jsonl").open("w") as raw:
        for n in SIZES:
            for sample in range(args.samples):
                order = ["before", "after"] if sample % 2 == 0 else ["after", "before"]
                for position, label in enumerate(order):
                    for row in run_probe(inputs[label]["binary"], n, repeats):
                        row.update(variant=label, sample=sample, order_position=position)
                        raw.write(json.dumps(row, sort_keys=True) + "\n")
                        groups[(n, row["phase"], label)].append(row)
            print(f"n={n}: {args.samples} balanced pairs", flush=True)
    for metadata in inputs.values():
        if digest(Path(metadata["binary"])) != metadata["binary_sha256"]:
            raise RuntimeError("saved binary changed during recheck; summary not saved")
    args.output.with_suffix(".summary.json").write_text(json.dumps({
        "metadata": {"builds": inputs, "samples": args.samples,
                     "measurement_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                     "execution_order": "AB on even sample, BA on odd sample",
                     "current_environment": current,
                     "runner_sha256": digest(Path(__file__).resolve()),
                     "probe_parser_sha256": digest(Path(__file__).with_name("run.py"))},
        "summaries": summarize(groups, repeats, variants=True)}, indent=2) + "\n")


if __name__ == "__main__":
    main()
