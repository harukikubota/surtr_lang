#!/usr/bin/env python3
"""Alternate saved before/after binaries to recheck time and process RSS."""
import argparse
import collections
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
from run import digest

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", required=True, type=Path)
parser.add_argument("--samples", type=int, default=10)
args = parser.parse_args()
if args.samples < 1 or platform.system() != "Darwin" or "PROBE_VERIFY" in os.environ:
    parser.error("positive samples, macOS and unset PROBE_VERIFY are required")
package = Path(__file__).resolve().parent
root = package.parents[2]
references = {
    "before": root / "doc/hashmap_cost_2026_10_11_time.summary.json",
    "after": root / "doc/hashmap_cost_2026_10_11_after_time.summary.json",
}
binaries = {}
for side, reference in references.items():
    meta = json.loads(reference.read_text())["metadata"]
    binary = Path(meta["binary"]).resolve(strict=True)
    if digest(binary) != meta["binary_sha256"]:
        raise RuntimeError(f"{side} binary differs from saved measurement")
    binaries[side] = binary
metadata = {
    "samples_per_side": args.samples, "ordering": "paired, reversed on odd samples",
    "runner_sha256": digest(Path(__file__).resolve()),
    "references": {s: {"path": str(p), "sha256": digest(p),
                       "binary": str(binaries[s]), "binary_sha256": digest(binaries[s])}
                   for s, p in references.items()},
    "platform": platform.platform(), "process_max_rss_unit": "bytes",
}
groups = collections.defaultdict(list)
output = args.output.resolve()
output.parent.mkdir(parents=True, exist_ok=True)
with output.with_suffix(".jsonl").open("w") as raw:
    for case in ["vm_same", "vm_distinct", "vm_map_values", "sort"]:
        config = [case, "1024", "string"]
        for sample in range(args.samples):
            order = ["before", "after"] if sample % 2 == 0 else ["after", "before"]
            for side in order:
                result = subprocess.run(["/usr/bin/time", "-l", str(binaries[side]), *config],
                                        cwd=root, text=True, capture_output=True, check=True)
                rss = re.search(r"^\s*(\d+)\s+maximum resident set size\s*$", result.stderr, re.MULTILINE)
                if rss is None:
                    raise RuntimeError("RSS missing")
                rows = [json.loads(line) for line in result.stdout.splitlines()]
                if len(rows) != 3 or {r["phase"] for r in rows} != {"prepare", "run", "drop"}:
                    raise RuntimeError("unexpected phases")
                for row in rows:
                    if row["allocator_counting"] or row["correctness_verification"] or row["case"] != case or row["n"] != 1024 or row["payload"] != "string":
                        raise RuntimeError("unexpected workload or allocator mode")
                    row.update(side=side, sample=sample, args=config,
                               process_max_rss_bytes=int(rss.group(1)))
                    raw.write(json.dumps(row, sort_keys=True) + "\n")
                    groups[(side, case, row["phase"])].append(row)
        print(f"{case}: {args.samples} paired samples", flush=True)
summary = []
for (side, case, phase), rows in groups.items():
    assert len(rows) == args.samples
    item = {"side": side, "case": case, "phase": phase, "samples": len(rows)}
    for field in ["ns", "process_max_rss_bytes"]:
        values = [r[field] for r in rows]
        item[field] = dict(median=statistics.median(values), min=min(values), max=max(values))
    summary.append(item)
output.with_suffix(".summary.json").write_text(json.dumps(
    dict(metadata=metadata, measurements=summary), indent=2) + "\n")
print(f"Saved {sum(map(len, groups.values()))} interval rows and {len(groups)} summaries")
