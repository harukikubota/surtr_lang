#!/usr/bin/env python3
"""Measure suspended Task checkpoint copies, or verify contents in separate processes."""
import argparse
import collections
import datetime
import hashlib
import itertools
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess


ROOT = Path(__file__).resolve().parents[3]
TEST_NAME = "vm::list_flat_map_tests::checkpoint_builder_profile"
TASKS = [1, 8, 32]
LENGTHS = [32, 1024, 16384]
PAYLOADS = ["int", "string"]
PHASES = {"save", "drop_unchanged", "first_resume", "second_resume", "resave",
          "resume_after_resave", "rollback", "complete_shared", "drop_changed", "unique_complete"}
COUNTERS = ["copies", "copied_items", "pushes", "callback_calls", "finishes",
            "capacity_growths", "finish_steps", "finish_copied_items"]


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def source_snapshot():
    sources = sorted(path for name in ["eldr", "sindr"]
                     for path in (ROOT / "crates" / name / "src").rglob("*.rs"))
    manifests = [ROOT / "Cargo.toml", ROOT / "crates/eldr/Cargo.toml",
                 ROOT / "crates/sindr/Cargo.toml", ROOT / "Cargo.lock"]
    return {
        "sources_sha256": {str(p.relative_to(ROOT)): digest(p) for p in sources},
        "manifests_and_lock_sha256": {str(p.relative_to(ROOT)): digest(p) for p in manifests},
        "git_diff_head_sha256": hashlib.sha256(subprocess.check_output(
            ["git", "diff", "HEAD", "--binary"], cwd=ROOT)).hexdigest(),
    }


def nonnegative_int(value):
    return type(value) is int and value >= 0


def validate(rows, tasks, length, payload, repeats, mode):
    if len(rows) != len(PHASES) * repeats:
        raise RuntimeError(f"expected {len(PHASES) * repeats} rows, got {len(rows)}")
    grouped = collections.defaultdict(dict)
    for row in rows:
        if (row.get("profile") != "checkpoint_builder" or row.get("copy_mode") != mode or row.get("tasks") != tasks
                or row.get("builder_len") != length or row.get("payload") != payload
                or row.get("quantum") != 256 or row.get("output_len") != length + 2048
                or not nonnegative_int(row.get("repeat")) or row["repeat"] >= repeats
                or row.get("phase") not in PHASES
                or not nonnegative_int(row.get("elapsed_ns"))
                or any(not nonnegative_int(row.get(c)) for c in COUNTERS)):
            raise RuntimeError(f"invalid probe row: {row}")
        if row["phase"] in grouped[row["repeat"]]:
            raise RuntimeError("duplicate phase within repeat")
        grouped[row["repeat"]][row["phase"]] = row
    if set(grouped) != set(range(repeats)):
        raise RuntimeError("missing repeat")
    for phases in grouped.values():
        if set(phases) != PHASES:
            raise RuntimeError("missing phase")
        progressed = phases["first_resume"]["pushes"] + phases["second_resume"]["pushes"]
        for phase, row in phases.items():
            copies = tasks if phase in {"first_resume", "resume_after_resave"} else 0
            copied = (tasks * length if phase == "first_resume" else (
                tasks * length + progressed if phase == "resume_after_resave" else 0)) if mode == "baseline" else 0
            finishes = tasks if phase in {"complete_shared", "unique_complete"} else 0
            finish_copied = tasks * length if phase == "complete_shared" and mode == "shared" else 0
            if (row["copies"] != copies or row["copied_items"] != copied
                    or row["callback_calls"] != 0 or row["finishes"] != finishes
                    or row["finish_copied_items"] != finish_copied):
                raise RuntimeError(f"copy or continuation contract mismatch: {row}")
            if phase in {"first_resume", "second_resume", "resume_after_resave"}:
                if row["pushes"] != tasks * 256:
                    raise RuntimeError(f"unexpected scheduler progress: {row}")
            elif phase in {"complete_shared", "unique_complete"}:
                expected = tasks * (2048 if phase == "unique_complete" else 1536)
                if row["pushes"] != expected:
                    raise RuntimeError(f"unexpected completion progress: {row}")
            elif row["pushes"] != 0 or row["capacity_growths"] != 0:
                raise RuntimeError(f"unexpected construction outside resume: {row}")


def run_probe(binary, tasks, length, payload, repeats, verify, mode):
    env = dict(os.environ, SURTR_BUILDER_PROFILE="1", SURTR_BUILDER_TASKS=str(tasks),
               SURTR_BUILDER_LEN=str(length), SURTR_BUILDER_PAYLOAD=payload,
               SURTR_BUILDER_REPEATS=str(repeats), SURTR_BUILDER_COPY_MODE=mode,
               PROBE_VERIFY="1" if verify else "0")
    cmd = [str(binary), "--exact", TEST_NAME, "--nocapture", "--test-threads=1"]
    if not verify:
        cmd = ["/usr/bin/time", "-l", *cmd]
    result = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True, check=True)
    rows = [json.loads(line) for line in result.stderr.splitlines() if line.startswith("{")]
    if verify:
        if (len(rows) != 1 or rows[0].get("profile") != "checkpoint_builder_verify"
                or rows[0].get("tasks") != tasks or rows[0].get("builder_len") != length
                or rows[0].get("payload") != payload or rows[0].get("repeats") != repeats
                or rows[0].get("copy_mode") != mode or rows[0].get("verified") is not True):
            raise RuntimeError(f"invalid verification output: {result.stdout}\n{result.stderr}")
    else:
        rss = re.findall(r"^\s*(\d+)\s+maximum resident set size\s*$", result.stderr, re.MULTILINE)
        if len(rss) != 1 or int(rss[0]) <= 0:
            raise RuntimeError(f"missing macOS RSS: {result.stderr}")
        validate(rows, tasks, length, payload, repeats, mode)
        for row in rows:
            row["process_peak_rss_bytes"] = int(rss[0])
    return rows


def summarize(groups, repeats):
    result = []
    for (tasks, length, payload, phase), rows in sorted(groups.items()):
        values = collections.defaultdict(list)
        for row in rows:
            values[row["sample"]].append(row)
        totals = [sum(r["elapsed_ns"] for r in rs) for rs in values.values()]
        rss = [rs[0]["process_peak_rss_bytes"] for rs in values.values()]
        summary = {"tasks": tasks, "builder_len": length, "payload": payload, "phase": phase,
                   "process_samples": len(values), "repeats": repeats,
                   "elapsed_ns_per_operation": {
                       "min": min(totals) / repeats, "median": statistics.median(totals) / repeats,
                       "max": max(totals) / repeats},
                   "process_peak_rss_bytes": {
                       "min": min(rss), "median": statistics.median(rss), "max": max(rss)}}
        for counter in COUNTERS:
            counts = [r[counter] for r in rows]
            summary[counter] = {"min": min(counts), "median": statistics.median(counts),
                                "max": max(counts)}
        result.append(summary)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--repeats", type=int, default=8)
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--copy-mode", choices=["baseline", "shared"], default="shared")
    args = parser.parse_args()
    if min(args.samples, args.repeats) < 1:
        parser.error("samples and repeats must be positive")
    if not args.verify and platform.system() != "Darwin":
        parser.error("RSS measurement requires macOS /usr/bin/time -l")
    binary = args.binary.resolve(strict=True)
    snapshot = source_snapshot()
    runner_hash = digest(Path(__file__).resolve())
    metadata = {
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "root": str(ROOT), "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "git_status_porcelain": subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True),
        "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
        "platform": platform.platform(), "machine": platform.machine(),
        "binary": str(binary), "binary_sha256": digest(binary), "runner_sha256": runner_hash,
        "test_name": TEST_NAME, "build_profile": "release unit-test binary",
        "tasks": TASKS, "lengths": LENGTHS, "payloads": PAYLOADS,
        "samples": args.samples, "repeats": args.repeats, "verification": args.verify,
        "copy_mode": args.copy_mode,
        "allocator_counting": False, "logical_clone_counters": True,
        "rss_scope": "whole fresh test process high-water mark; includes all repeats and preparation",
        **snapshot,
    }
    groups = collections.defaultdict(list)
    verified = []
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix(".jsonl").open("x") as raw:
        for tasks, length, payload in itertools.product(TASKS, LENGTHS, PAYLOADS):
            for sample in range(args.samples):
                for row in run_probe(binary, tasks, length, payload, args.repeats, args.verify, args.copy_mode):
                    row["sample"] = sample
                    raw.write(json.dumps(row, sort_keys=True) + "\n")
                    if args.verify:
                        verified.append(row)
                    else:
                        groups[(tasks, length, payload, row["phase"])].append(row)
            raw.flush()
            print(f"tasks={tasks}, builder_len={length}, payload={payload}: {args.samples} processes", flush=True)
    if (source_snapshot() != snapshot or digest(binary) != metadata["binary_sha256"]
            or digest(Path(__file__).resolve()) != runner_hash):
        raise RuntimeError("source, runner or binary changed; summary not saved")
    with args.output.with_suffix(".summary.json").open("x") as out:
        json.dump({"metadata": metadata, "verified": verified} if args.verify else {
            "metadata": metadata, "summaries": summarize(groups, args.repeats)}, out, indent=2)
        out.write("\n")


if __name__ == "__main__":
    main()
