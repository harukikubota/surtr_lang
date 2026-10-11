#!/usr/bin/env python3
"""Measure RT-4 HashMap workloads in fresh macOS processes."""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess


def configurations():
    return [[case, str(n), kind]
            for case in ["rust_distinct", "rust_same", "sort", "vm_distinct", "vm_same", "vm_map_values"]
            for n in [256, 512, 1024] for kind in ["int", "string"]]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--samples", type=int, default=5)
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("--samples must be positive")
    if platform.system() != "Darwin":
        parser.error("this runner requires macOS /usr/bin/time -l (RSS bytes)")
    if "PROBE_VERIFY" in os.environ:
        parser.error("unset PROBE_VERIFY: verification must use separate processes")
    binary = args.binary.resolve(strict=True)
    package = Path(__file__).resolve().parent
    root = package.parents[2]
    cases = configurations()
    metadata = {
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
        "platform": platform.platform(), "machine": platform.machine(),
        "binary": str(binary), "binary_sha256": digest(binary),
        "probe_sources_sha256": {str(p.relative_to(package)): digest(p) for p in sorted((package / "src").rglob("*.rs"))},
        "probe_lock_sha256": digest(package / "Cargo.lock"),
        "production_sources_sha256": {str(p.relative_to(root)): digest(p) for p in [
            root / "crates/sindr/src/runtime.rs", root / "crates/eldr/src/builtin.rs",
            root / "crates/eldr/src/builtin/map_values.rs", root / "crates/eldr/src/vm.rs",
            root / "crates/eldr/src/vm/map_values_tests.rs"]},
        "production_diff_sha256": hashlib.sha256(subprocess.check_output(
            ["git", "diff", "--", "crates"], cwd=root)).hexdigest(),
        "runner_sha256": digest(Path(__file__).resolve()),
        "samples": args.samples, "workloads": len(cases),
        "build_profile": "release", "process_max_rss_unit": "bytes",
    }
    groups = collections.defaultdict(list)
    expected_phases = {"prepare", "run", "drop"}
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.with_suffix(".jsonl").open("w") as raw:
        for config in cases:
            for sample in range(args.samples):
                result = subprocess.run(["/usr/bin/time", "-l", str(binary), *config], cwd=root, text=True, capture_output=True, check=True)
                rss = re.search(r"^\s*(\d+)\s+maximum resident set size\s*$", result.stderr, re.MULTILINE)
                if rss is None:
                    raise RuntimeError(f"process peak RSS missing: {result.stderr}")
                rows = [json.loads(line) for line in result.stdout.splitlines()]
                if not rows or any(row["correctness_verification"] for row in rows):
                    raise RuntimeError("empty or verification-contaminated measurement")
                phases = [row["phase"] for row in rows]
                if len(phases) != len(set(phases)) or set(phases) != expected_phases:
                    raise RuntimeError("incorrect measurement phase set")
                expected_entry = "manual_bytecode" if config[0].startswith("vm_") else "public_rust_api"
                if any(row["entry"] != expected_entry or row["case"] != config[0]
                       or row["n"] != int(config[1]) or row["payload"] != config[2] for row in rows):
                    raise RuntimeError("binary does not match requested workload")
                for row in rows:
                    row.update(sample=sample, args=config, process_max_rss_bytes=int(rss.group(1)))
                    raw.write(json.dumps(row, sort_keys=True) + "\n")
                    groups[tuple(config) + (row["phase"],)].append(row)
            print(f"{' '.join(config)} ({args.samples} samples)", flush=True)
    summaries = []
    for key, rows in groups.items():
        if len(rows) != args.samples:
            raise RuntimeError("missing phase samples")
        summary = {"args": list(key[:-1]), "phase": key[-1], "samples": len(rows), "allocator_counting": rows[0]["allocator_counting"]}
        for field, value in rows[0].items():
            if isinstance(value, (int, float)) and not isinstance(value, bool) and field != "sample":
                values = [row[field] for row in rows]
                summary[field] = {"median": statistics.median(values), "min": min(values), "max": max(values)}
        summaries.append(summary)
    output.with_suffix(".summary.json").write_text(
        '{\n  "metadata": ' + json.dumps(metadata, indent=2).replace('\n', '\n  ')
        + ',\n  "measurements": [\n    '
        + ',\n    '.join(json.dumps(s, sort_keys=True) for s in summaries)
        + '\n  ]\n}\n'
    )
    print(f"Saved {sum(map(len, groups.values()))} samples, {len(groups)} phase summaries")


if __name__ == "__main__":
    main()
