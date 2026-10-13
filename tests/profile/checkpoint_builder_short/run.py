#!/usr/bin/env python3
"""Record builds and compare genuinely short Task outputs in separate processes."""
import argparse
import collections
import datetime
import hashlib
import itertools
import json
import os
import platform
import re
import statistics
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
TEST_NAME = "vm::list_flat_map_tests::true_short_builder_profile"
TASKS = [1, 32]
LENGTHS = [0, 1, 8, 32, 256]
PAYLOADS = ["int", "string"]
WORKLOADS = ["construct_complete", "snapshot_resume"]
COUNTERS = ["callback_calls", "input_visits", "pushes", "finishes", "copies", "copied_items",
            "finish_steps", "finish_copied_items"]


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def source_snapshot():
    paths = sorted(path for crate in ["eldr", "sindr"]
                   for path in (ROOT / "crates" / crate / "src").rglob("*.rs"))
    paths += [ROOT / name for name in ["Cargo.toml", "Cargo.lock", "crates/eldr/Cargo.toml", "crates/sindr/Cargo.toml"]]
    return {str(path.relative_to(ROOT)): digest(path) for path in paths}


def record_build(binary, output):
    binary = binary.resolve(strict=True)
    metadata = {
        "binary": str(binary), "binary_sha256": digest(binary), "test_name": TEST_NAME,
        "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "git_status_porcelain": subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True),
        "root": str(ROOT), "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
        "platform": platform.platform(), "machine": platform.machine(),
        "runner_sha256": digest(Path(__file__).resolve()), "sources_sha256": source_snapshot(),
        "build_profile": "release unit-test binary", "allocator_counting": False,
        "logical_counters": True, "tasks": TASKS, "lengths": LENGTHS,
        "payloads": PAYLOADS, "workloads": WORKLOADS,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("x") as destination:
        json.dump(metadata, destination, indent=2)
        destination.write("\n")


def read_build(path):
    metadata = json.loads(path.read_text())
    expected = {"test_name": TEST_NAME, "tasks": TASKS, "lengths": LENGTHS, "payloads": PAYLOADS,
                "workloads": WORKLOADS, "allocator_counting": False, "logical_counters": True,
                "build_profile": "release unit-test binary"}
    if any(metadata.get(key) != value for key, value in expected.items()):
        raise RuntimeError(f"invalid short-profile build metadata: {path}")
    binary = Path(metadata["binary"])
    if not binary.is_absolute() or digest(binary) != metadata["binary_sha256"]:
        raise RuntimeError(f"saved binary hash mismatch: {path}")
    if (metadata["runner_sha256"] != digest(Path(__file__).resolve())
            or metadata["rustc"] != subprocess.check_output(["rustc", "-V"], text=True).strip()
            or metadata["platform"] != platform.platform() or metadata["machine"] != platform.machine()):
        raise RuntimeError(f"environment/runner differs from recorded build: {path}")
    return metadata


def nonnegative_int(value):
    return type(value) is int and value >= 0


def run_probe(metadata, tasks, length, payload, workload, repeats, verify):
    env = dict(os.environ, SURTR_SHORT_PROFILE="1", SURTR_SHORT_TASKS=str(tasks), SURTR_SHORT_LEN=str(length),
               SURTR_SHORT_PAYLOAD=payload, SURTR_SHORT_WORKLOAD=workload, SURTR_SHORT_REPEATS=str(repeats),
               PROBE_VERIFY="1" if verify else "0")
    command = [metadata["binary"], "--exact", TEST_NAME, "--nocapture", "--test-threads=1"]
    if not verify:
        command = ["/usr/bin/time", "-l", *command]
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, check=True)
    rows = [json.loads(line) for line in result.stderr.splitlines() if line.startswith("{")]
    common = {"workload": workload, "tasks": tasks, "output_len": length, "payload": payload, "repeats": repeats}
    if verify:
        if (len(rows) != 1 or rows[0].get("profile") != "checkpoint_builder_short_verify"
                or rows[0].get("verified") is not True
                or any(rows[0].get(key) != value for key, value in common.items())):
            raise RuntimeError(f"invalid short verification result: {rows}")
        return rows
    observed = set()
    for row in rows:
        if (row.get("profile") != "checkpoint_builder_short"
                or any(row.get(key) != value for key, value in common.items())
                or not nonnegative_int(row.get("elapsed_ns"))
                or not nonnegative_int(row.get("repeat")) or row["repeat"] >= repeats
                or row["repeat"] in observed
                or any(not nonnegative_int(row.get(field)) for field in COUNTERS)):
            raise RuntimeError(f"invalid short profile result: {row}")
        observed.add(row["repeat"])
        snapshot = workload == "snapshot_resume"
        saved = length // 2 if snapshot else None
        expected = {"saved_len": saved, "callback_calls": 0 if snapshot else tasks,
                    "input_visits": 0 if snapshot else tasks,
                    "pushes": tasks * (length - saved if snapshot else length), "finishes": tasks,
                    "copies": tasks if snapshot else 0, "copied_items": 0,
                    "finish_copied_items": tasks * saved if snapshot else 0}
        if any(row.get(key) != value for key, value in expected.items()):
            raise RuntimeError(f"short ownership/callback counter mismatch: {row}")
    if observed != set(range(repeats)):
        raise RuntimeError("missing short repeat")
    rss = re.findall(r"^\s*(\d+)\s+maximum resident set size\s*$", result.stderr, re.MULTILINE)
    if len(rss) != 1 or int(rss[0]) <= 0:
        raise RuntimeError(f"missing whole-process RSS: {result.stderr}")
    for row in rows:
        row["process_peak_rss_bytes"] = int(rss[0])
    return rows


def summarize(groups, repeats):
    summaries = []
    for key, rows in sorted(groups.items()):
        variant, tasks, length, payload, workload = key
        processes = collections.defaultdict(list)
        for row in rows:
            processes[row["sample"]].append(row)
        times = [sum(row["elapsed_ns"] for row in current) / repeats for current in processes.values()]
        peaks = [current[0]["process_peak_rss_bytes"] for current in processes.values()]
        stats = lambda values: {"min": min(values), "median": statistics.median(values), "max": max(values)}
        summary = {"variant": variant, "tasks": tasks, "output_len": length, "payload": payload,
                   "workload": workload, "repeats": repeats, "process_samples": len(processes),
                   "elapsed_ns_per_operation": stats(times), "process_peak_rss_bytes": stats(peaks)}
        for counter in COUNTERS:
            summary[counter] = stats([row[counter] for row in rows])
        summaries.append(summary)
    return summaries


def run(args, verify):
    if verify:
        builds = {"verified": read_build(args.build)}
    else:
        builds = {"before": read_build(args.before), "after": read_build(args.after)}
    runner_hash = digest(Path(__file__).resolve())
    metadata = {
        "builds": builds, "samples": args.samples, "repeats": args.repeats,
        "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "verification": verify, "execution_order": "AB on even sample, BA on odd sample",
        "runner_sha256": runner_hash,
        "rss_scope": "whole fresh test process; preparation, all repeats, assertions, JSON, destruction and harness included",
    }
    groups, verified = collections.defaultdict(list), []
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix(".jsonl").open("x") as raw:
        for tasks, length, payload, workload in itertools.product(TASKS, LENGTHS, PAYLOADS, WORKLOADS):
            for sample in range(args.samples):
                order = list(builds) if verify or sample % 2 == 0 else list(reversed(builds))
                for order_position, variant in enumerate(order):
                    for row in run_probe(builds[variant], tasks, length, payload, workload, args.repeats, verify):
                        row.update(variant=variant, sample=sample, order_position=order_position)
                        raw.write(json.dumps(row, sort_keys=True) + "\n")
                        if verify:
                            verified.append(row)
                        else:
                            groups[(variant, tasks, length, payload, workload)].append(row)
            raw.flush()
            print(f"Task={tasks}, output={length}, {payload}, {workload}: {args.samples} process samples", flush=True)
    if any(digest(Path(build["binary"])) != build["binary_sha256"] for build in builds.values()) or runner_hash != digest(Path(__file__).resolve()):
        raise RuntimeError("binary or runner changed; summary not saved")
    with args.output.with_suffix(".summary.json").open("x") as destination:
        json.dump({"metadata": metadata, "verified": verified} if verify else
                  {"metadata": metadata, "summaries": summarize(groups, args.repeats)}, destination, indent=2)
        destination.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    record = commands.add_parser("record", help="record source and binary after a frozen build")
    record.add_argument("--binary", type=Path, required=True)
    record.add_argument("--output", type=Path, required=True)
    compare = commands.add_parser("compare", help="compare saved binaries in balanced order")
    compare.add_argument("--before", type=Path, required=True)
    compare.add_argument("--after", type=Path, required=True)
    compare.add_argument("--output", type=Path, required=True)
    compare.add_argument("--samples", type=int, default=6)
    compare.add_argument("--repeats", type=int, default=8)
    verify = commands.add_parser("verify", help="verify all complete outputs in separate processes")
    verify.add_argument("--build", type=Path, required=True)
    verify.add_argument("--output", type=Path, required=True)
    verify.add_argument("--samples", type=int, default=1)
    verify.add_argument("--repeats", type=int, default=1)
    args = parser.parse_args()
    if args.command == "record":
        record_build(args.binary, args.output)
        return
    if min(args.samples, args.repeats) < 1:
        parser.error("samples and repeats must be positive")
    if args.command == "compare" and (args.samples % 2 or platform.system() != "Darwin"):
        parser.error("compare requires even balanced samples and macOS RSS tools")
    run(args, args.command == "verify")


if __name__ == "__main__":
    main()
