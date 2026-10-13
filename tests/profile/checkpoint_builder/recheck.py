#!/usr/bin/env python3
"""Compare saved Builder binaries in alternating AB/BA process order."""
import argparse
import collections
import datetime
import json
from pathlib import Path
import platform
import subprocess
import sys

sys.dont_write_bytecode = True
from run import TASKS, LENGTHS, PAYLOADS, PHASES, TEST_NAME, digest, run_probe, summarize


def read_build(path, mode):
    doc = json.loads(path.read_text())
    meta = doc["metadata"]
    if (meta["copy_mode"] != mode or meta["verification"] is not False
            or meta["test_name"] != TEST_NAME or meta["tasks"] != TASKS
            or meta["lengths"] != LENGTHS or meta["payloads"] != PAYLOADS
            or meta["logical_clone_counters"] is not True or meta["allocator_counting"] is not False):
        raise RuntimeError(f"unexpected build conditions: {path}")
    binary = Path(meta["binary"])
    if not binary.is_absolute() or digest(binary) != meta["binary_sha256"]:
        raise RuntimeError(f"saved binary hash mismatch: {path}")
    expected = {(n, length, payload, phase) for n in TASKS for length in LENGTHS
                for payload in PAYLOADS for phase in PHASES}
    observed = {(r["tasks"], r["builder_len"], r["payload"], r["phase"]) for r in doc["summaries"]}
    if observed != expected or len(doc["summaries"]) != len(expected):
        raise RuntimeError(f"unexpected summary phases: {path}")
    return meta


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=6)
    args = parser.parse_args()
    if args.samples < 2 or args.samples % 2:
        parser.error("samples must be positive and even")
    builds = {"before": read_build(args.before, "baseline"), "after": read_build(args.after, "shared")}
    for key in ["manifests_and_lock_sha256", "rustc", "platform", "machine", "build_profile",
                "runner_sha256", "repeats", "test_name", "rss_scope"]:
        if builds["before"][key] != builds["after"][key]:
            raise RuntimeError(f"before/after {key} differ")
    if (platform.platform() != builds["before"]["platform"]
            or platform.machine() != builds["before"]["machine"]
            or subprocess.check_output(["rustc", "-V"], text=True).strip() != builds["before"]["rustc"]):
        raise RuntimeError("current environment differs from saved builds")
    runner_hash = digest(Path(__file__).with_name("run.py"))
    if runner_hash != builds["before"]["runner_sha256"]:
        raise RuntimeError("probe runner differs from saved builds")
    repeats = builds["before"]["repeats"]
    groups = {label: collections.defaultdict(list) for label in builds}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix(".jsonl").open("x") as raw:
        for tasks in TASKS:
            for length in LENGTHS:
                for payload in PAYLOADS:
                    for sample in range(args.samples):
                        order = ["before", "after"] if sample % 2 == 0 else ["after", "before"]
                        for position, label in enumerate(order):
                            meta = builds[label]
                            for row in run_probe(Path(meta["binary"]), tasks, length, payload,
                                                 repeats, False, meta["copy_mode"]):
                                row.update(sample=sample, variant=label, order_position=position)
                                raw.write(json.dumps(row, sort_keys=True) + "\n")
                                groups[label][(tasks, length, payload, row["phase"])].append(row)
                    raw.flush()
                    print(f"tasks={tasks}, builder_len={length}, payload={payload}: {args.samples} balanced pairs", flush=True)
    for meta in builds.values():
        if digest(Path(meta["binary"])) != meta["binary_sha256"]:
            raise RuntimeError("saved binary changed; summary not saved")
    if digest(Path(__file__).with_name("run.py")) != runner_hash:
        raise RuntimeError("probe runner changed; summary not saved")
    summaries = []
    for label, group in groups.items():
        for row in summarize(group, repeats):
            row["variant"] = label
            summaries.append(row)
    with args.output.with_suffix(".summary.json").open("x") as out:
        json.dump({"metadata": {"builds": builds, "samples": args.samples,
                               "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                               "execution_order": "AB on even sample, BA on odd sample",
                               "recheck_runner_sha256": digest(Path(__file__)),
                               "probe_runner_sha256": runner_hash},
                   "summaries": summaries}, out, indent=2)
        out.write("\n")


if __name__ == "__main__":
    main()
