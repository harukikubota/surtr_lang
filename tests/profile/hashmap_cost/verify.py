#!/usr/bin/env python3
"""Check complete contents in separate, non-performance processes."""
import argparse
import json
import os
from pathlib import Path
import subprocess
from run import configurations

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", required=True, type=Path)
args = parser.parse_args()
binary = args.binary.resolve(strict=True)
cases = configurations()
cases += [[case, str(n), kind] for case in
          ["rust_distinct", "rust_same", "sort", "vm_distinct", "vm_same", "vm_map_values"]
          for n in [0, 1] for kind in ["int", "string"]]
for config in cases:
    result = subprocess.run([str(binary), *config], check=True, capture_output=True,
                            text=True, env={**os.environ, "PROBE_VERIFY": "1"})
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    if len(rows) != 3 or not all(row["correctness_verification"] and row["allocator_counting"] for row in rows):
        raise RuntimeError("expected three verification rows from counting binary")
print(f"{len(cases)} verification processes passed; no performance data saved")
