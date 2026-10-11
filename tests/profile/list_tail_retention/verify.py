#!/usr/bin/env python3
"""Check indexed contents in separate processes; never save performance samples."""
import argparse
import itertools
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    cases = itertools.product(
        ["shared", "chunk", "compact"],
        [2, 255, 256, 257, 511, 512, 513, 10_000, 100_000],
        ["int", "string", "nested"],
        ["suffix", "siblings", "checkpoint"],
    )
    count = 0
    for mode, n, kind, ownership in cases:
        run = subprocess.run(
            [str(binary), mode, str(n), kind, ownership],
            env={**os.environ, "PROBE_VERIFY": "1"},
            text=True, capture_output=True, check=True,
        )
        rows = [json.loads(line) for line in run.stdout.splitlines()]
        assert len(rows) == 7 and all(r["verification"] for r in rows)
        assert all(r["allocator_counting"] for r in rows), "use the count binary"
        assert rows[-1]["workload_retained_bytes"] == 0
        count += 1
    print(f"{count} separate verification processes passed; no performance data saved")


if __name__ == "__main__":
    main()
