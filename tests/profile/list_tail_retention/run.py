#!/usr/bin/env python3
"""Run each manual workload in a fresh process; keep raw samples and summaries."""
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path, help="output filename prefix")
    parser.add_argument("--samples", type=int, default=5)
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("--samples must be positive")
    if platform.system() != "Darwin":
        parser.error("this runner expects macOS /usr/bin/time -l (max RSS in bytes)")
    if "PROBE_VERIFY" in os.environ:
        parser.error("unset PROBE_VERIFY: validation runs must be separate from measurements")
    binary = args.binary.resolve(strict=True)
    root = Path(__file__).resolve().parents[3]
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    configurations = [
        [mode, str(n), kind, ownership]
        for mode in ["shared", "chunk", "compact"]
        for n in [10_000, 100_000]
        for kind in ["int", "string", "nested"]
        for ownership in ["suffix", "siblings", "checkpoint"]
    ]

    metadata = {
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
        "platform": platform.platform(), "machine": platform.machine(),
        "binary": str(binary), "samples": args.samples,
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "probe_source_sha256": hashlib.sha256(Path(__file__).with_name("src").joinpath("main.rs").read_bytes()).hexdigest(),
        "probe_lock_sha256": hashlib.sha256(Path(__file__).with_name("Cargo.lock").read_bytes()).hexdigest(),
        "rss_unit": "bytes for process max",
        "build_profile": "release", "workloads": len(configurations),
    }
    groups = collections.defaultdict(list)
    raw_path = output.with_suffix(".jsonl")
    with raw_path.open("w") as raw:
        for config in configurations:
            for sample in range(args.samples):
                completed = subprocess.run(
                    ["/usr/bin/time", "-l", str(binary), *config],
                    cwd=root, text=True, capture_output=True, check=True,
                )
                match = re.search(r"^\s*(\d+)\s+maximum resident set size\s*$", completed.stderr, re.MULTILINE)
                if not match:
                    raise RuntimeError(f"max RSS absent: {completed.stderr}")
                rows = [json.loads(line) for line in completed.stdout.splitlines()]
                if len(rows) != 7 or any(row["verification"] for row in rows):
                    raise RuntimeError("invalid phase count or verification contaminated measurements")
                if not rows:
                    raise RuntimeError("workload emitted no measurements")
                for row in rows:
                    row.update(sample=sample, args=config, process_max_rss_bytes=int(match.group(1)))
                    raw.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")
                    groups[tuple(config) + (row["phase"],)].append(row)
    summaries = []
    for key, rows in groups.items():
        summary = {"args": list(key[:-1]), "phase": key[-1], "samples": len(rows)}
        for field, value in rows[0].items():
            if isinstance(value, (int, float)) and not isinstance(value, bool) and field != "sample":
                values = [row[field] for row in rows]
                summary[field] = {"median": statistics.median(values), "min": min(values), "max": max(values)}
        summary["allocator_counting"] = rows[0]["allocator_counting"]
        summaries.append(summary)
    summary_path = output.with_suffix(".summary.json")
    # One row per phase keeps the complete raw evidence reviewable in a diff.
    summary_path.write_text(
        '{\n  "metadata": ' + json.dumps(metadata, ensure_ascii=False, indent=2).replace('\n', '\n  ')
        + ',\n  "measurements": [\n    '
        + ',\n    '.join(json.dumps(s, ensure_ascii=False, sort_keys=True) for s in summaries)
        + '\n  ]\n}\n'
    )
    print(f"Saved {sum(map(len, groups.values()))} phase samples to {raw_path}")
    print(f"Saved {len(summaries)} phase summaries to {summary_path}")


if __name__ == "__main__":
    main()
