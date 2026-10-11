#!/usr/bin/env python3
"""Recheck the two saved RT-5 binaries with balanced AB/BA execution order."""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True, help="before summary JSON")
    parser.add_argument("--after", type=Path, required=True, help="after summary JSON")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=10)
    args = parser.parse_args()
    if args.samples < 2 or args.samples % 2:
        parser.error("samples must be positive and even to balance execution order")
    root = Path(__file__).resolve().parents[3]
    inputs = {label: json.loads(path.read_text())["metadata"] for label, path in [
        ("before", args.before), ("after", args.after)]}
    for metadata in inputs.values():
        binary = Path(metadata["binary"])
        if hashlib.sha256(binary.read_bytes()).hexdigest() != metadata["binary_sha256"]:
            raise RuntimeError("saved binary hash mismatch")
    repeats = inputs["before"]["repeats"]
    if repeats != inputs["after"]["repeats"]:
        raise RuntimeError("repeat counts differ")
    groups = collections.defaultdict(list)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix(".jsonl").open("w") as raw:
        for n in [16, 128, 1024]:
            for sample in range(args.samples):
                order = ["before", "after"] if sample % 2 == 0 else ["after", "before"]
                for position, label in enumerate(order):
                    env = dict(os.environ, SURTR_METADATA_PROFILE="1", SURTR_METADATA_N=str(n),
                               SURTR_METADATA_REPEATS=str(repeats))
                    result = subprocess.run([inputs[label]["binary"], "--exact",
                        "vm::checkpoint_metadata_tests::process_metadata_profile", "--nocapture", "--test-threads=1"],
                        cwd=root, env=env, capture_output=True, text=True, check=True)
                    rows = [json.loads(line) for line in result.stderr.splitlines() if line.startswith("{")]
                    expected = {"save", "drop", "register", "rollback", "chunk_shared", "chunk_unique"}
                    if len(rows) != len(expected) or {r["phase"] for r in rows} != expected:
                        raise RuntimeError("unexpected phases")
                    for row in rows:
                        if row["n"] != n or row["repeats"] != repeats or sum(row["samples_ns"]) != row["ns"]:
                            raise RuntimeError("probe parameters or aggregate mismatch")
                        row.update(variant=label, sample=sample, order_position=position)
                        raw.write(json.dumps(row, sort_keys=True) + "\n")
                        groups[(n, row["phase"], label)].append(row["ns"])
            print(f"n={n}: {args.samples} balanced pairs", flush=True)
    summaries = []
    for (n, phase, label), values in sorted(groups.items()):
        summaries.append({"n": n, "phase": phase, "variant": label, "samples": len(values),
                          "repeats": repeats, "ns": {"median": statistics.median(values),
                          "min": min(values), "max": max(values)}})
    args.output.with_suffix(".summary.json").write_text(json.dumps({
        "metadata": {"builds": inputs, "samples": args.samples,
                     "execution_order": "AB on even sample, BA on odd sample",
                     "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()},
        "summaries": summaries}, indent=2) + "\n")


if __name__ == "__main__":
    main()
