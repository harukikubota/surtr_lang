#!/usr/bin/env python3
"""Verify complete shared-input results, then run an immutable Regex adoption pair."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for side in ["before", "after"]:
        for mode in ["count", "time"]:
            parser.add_argument(f"--{side}-{mode}", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=6)
    args = parser.parse_args()
    assert "PROBE_VERIFY" not in os.environ
    runner = Path(__file__).with_name("run.py")
    builds = {f"{side}-{mode}": json.loads(getattr(args, f"{side}_{mode}").read_text()) for side in ["before", "after"] for mode in ["count", "time"]}
    for label, build in builds.items():
        assert build["allocator_counting"] == label.endswith("count")
        assert sha(Path(build["binary"])) == build["binary_sha256"]
        assert all(sha(Path(build["source_root"]) / file) == digest for file, digest in build["source_sha256"].items())
    assert args.samples >= 2 and args.samples % 2 == 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    verify_path = args.output.with_suffix(".verify.jsonl")
    assert not args.output.with_suffix(".jsonl").exists() and not args.output.with_suffix(".summary.json").exists()
    configs = [(n, case) for n in [1024, 2048, 4096] for case in ["dense", "single", "none"]] + [(128, "utf8_empty")]
    with verify_path.open("x") as raw:
        for label, build in builds.items():
            for n, case in configs:
                result = subprocess.run([build["binary"], str(n), case], env=dict(os.environ, PROBE_VERIFY="1"), capture_output=True, text=True, check=True)
                rows = [json.loads(line) for line in result.stdout.splitlines()]
                assert len(rows) == 6
                for row in rows:
                    assert row["correctness_verification"]
                    assert row["verified_unique_input_buffers"] == (0 if case == "none" else 1)
                    row.pop("ns")
                    row["series"] = label
                    raw.write(json.dumps(row, sort_keys=True) + "\n")
    command = [sys.executable, str(runner), "--baseline-root", builds["before-count"]["source_root"], "--output", str(args.output), "--samples", str(args.samples)]
    for label, build in builds.items():
        command += [f"--{label}", build["binary"]]
    subprocess.run(command, check=True)
    summary = json.loads(args.output.with_suffix(".summary.json").read_text())
    summary["builds"] = builds
    summary["adoption_runner_sha256"] = sha(Path(__file__))
    summary["verification"] = {"processes": 40, "path": str(verify_path), "sha256": sha(verify_path)}
    for build in builds.values():
        assert sha(Path(build["binary"])) == build["binary_sha256"]
    args.output.with_suffix(".summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    main()
