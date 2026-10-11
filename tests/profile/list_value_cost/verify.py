#!/usr/bin/env python3
"""Validate each RT-3 workload in a process excluded from performance samples."""
import argparse
import json
import os
from pathlib import Path
import subprocess
from run import configurations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--entry", required=True, choices=["vm", "do"])
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    cases = configurations(args.entry)
    # Small inputs verify all paths and empty/singleton boundaries cheaply.
    cases += [[case, "1", kind, "1"] for case in ["singleton", "empty", "huge", "chain", "nested", "product"] for kind in (["int", "string", "tuple", "list", "hashmap"] if args.entry == "vm" else ["int", "string"])]
    for config in cases:
        result = subprocess.run([str(binary), *config], env={**os.environ, "PROBE_VERIFY": "1"}, text=True, capture_output=True, check=True)
        rows = [json.loads(line) for line in result.stdout.splitlines()]
        if not rows or not all(row["correctness_verification"] and row["allocator_counting"] for row in rows):
            raise RuntimeError("use the count binary; all rows must be verification rows")
        expected_entry = {"vm": "manual_bytecode", "do": "compiler_generic_do"}[args.entry]
        if any(row["entry"] != expected_entry for row in rows):
            raise RuntimeError("binary does not match --entry")
    print(f"{len(cases)} {args.entry} verification processes passed; no performance data saved")


if __name__ == "__main__":
    main()
