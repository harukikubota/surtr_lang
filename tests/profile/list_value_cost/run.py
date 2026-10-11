#!/usr/bin/env python3
"""Measure RT-3 in fresh macOS processes; keep verification runs separate."""
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


def configurations(entry):
    kinds = ["int", "string", "tuple", "list", "hashmap"] if entry == "vm" else ["int", "string"]
    return [
        [case, str(n), kind, "8"]
        for case in ["singleton", "empty", "huge", "chain", "nested", "product"]
        for n in ([10, 20] if case == "product" else [1000, 10000])
        for kind in kinds
    ]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--entry", required=True, choices=["vm", "do"])
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
    cases = configurations(args.entry)
    metadata = {
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(),
        "platform": platform.platform(), "machine": platform.machine(),
        "binary": str(binary), "binary_sha256": digest(binary),
        "probe_sources_sha256": {str(p.relative_to(package)): digest(p) for p in sorted((package / "src").rglob("*.rs"))},
        "probe_lock_sha256": digest(package / "Cargo.lock"),
        "runner_sha256": digest(Path(__file__).resolve()),
        "entry": args.entry, "samples": args.samples, "workloads": len(cases),
        "build_profile": "release", "process_max_rss_unit": "bytes",
    }
    groups = collections.defaultdict(list)
    expected_entry = {"vm": "manual_bytecode", "do": "compiler_generic_do"}[args.entry]
    expected_phases = ({"prepare_bytecode", "prepare_vm", "run", "extract_output", "drop_vm", "drop_output"}
                       if args.entry == "vm" else {"setup_stdlib", "compile", "prepare_vm", "run_vm", "drop_vm"})
    cache_states = set()
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
                if any(row["entry"] != expected_entry for row in rows):
                    raise RuntimeError("binary does not match --entry")
                if args.entry == "do":
                    cache_states.update(row["stdlib_cache_state"] for row in rows)
                    if any(row["stdlib_cache_state"] != "Some(DiskHit)" for row in rows):
                        raise RuntimeError("warm the stdlib cache first; performance runs require DiskHit")
                for row in rows:
                    row.update(sample=sample, args=config, process_max_rss_bytes=int(rss.group(1)))
                    raw.write(json.dumps(row, sort_keys=True) + "\n")
                    groups[tuple(config) + (row["phase"],)].append(row)
            print(f"{args.entry}: {' '.join(config)} ({args.samples} samples)", flush=True)
    summaries = []
    metadata["stdlib_cache_states"] = sorted(cache_states)
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
