#!/usr/bin/env python3
"""Measure actual ready snapshot allocation traffic and resolve time separately."""
import argparse
import hashlib
import itertools
import json
import os
import platform
import statistics
import subprocess
from pathlib import Path

TEST = 'vm::future_stack_tests::ready_snapshot_cost_profile'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=6)
    args = parser.parse_args()
    assert args.samples >= 2 and args.samples % 2 == 0
    builds = {side: json.loads(path.read_text())
              for side, path in [('before', args.before), ('after', args.after)]}
    for build in builds.values():
        assert '--release' in build['command']
        assert sha(Path(build['binary'])) == build['binary_sha256']
    args.output.parent.mkdir(parents=True, exist_ok=True)
    assert not args.output.with_suffix('.summary.json').exists()
    configurations = list(itertools.product([256, 4096], [0, 64],
                                            ['repeated', 'distinct'], ['ready', 'running', 'ordinary']))
    rows = []
    with args.output.with_suffix('.jsonl').open('x') as output:
        # Full contents have their own fresh processes and no cost metrics.
        for mode in ['verify', 'allocation', 'time']:
            for n, width, pattern, case in configurations:
                for sample in range(1 if mode == 'verify' else args.samples):
                    order = ['before', 'after'] if sample % 2 == 0 else ['after', 'before']
                    for position, side in enumerate(order):
                        env = dict(os.environ, OI036_COST_PROFILE='1', OI036_N=str(n),
                                   OI036_WIDTH=str(width), OI036_PATTERN=pattern,
                                   OI036_CASE=case, OI036_MODE=mode)
                        command = [builds[side]['binary'], '--exact', TEST,
                                   '--nocapture', '--test-threads=1']
                        result = subprocess.run(command, env=env, capture_output=True,
                                                text=True, check=True)
                        parsed = [json.loads(line) for line in result.stderr.splitlines()
                                  if line.startswith('{')]
                        assert len(parsed) == 1, result.stderr
                        row = parsed[0]
                        assert (row['n'], row['width'], row['pattern'], row['case'], row['mode']) == (n, width, pattern, case, mode)
                        assert row['contents_verified'] == (mode == 'verify')
                        assert ('elapsed_ns' in row) == (mode == 'time')
                        assert ('requested_bytes' in row) == (mode == 'allocation')
                        expected_lookups = 0 if case == 'ordinary' else (1 if pattern == 'repeated' else n)
                        assert row['ready_lookups'] == expected_lookups, row
                        assert row['source_value_clones'] == (expected_lookups if side == 'before' and case == 'ready' else 0), row
                        assert row['read_visits'] == (0 if case == 'ordinary' else n), row
                        assert row['write_visits'] == (n if case == 'ready' else 0), row
                        if mode == 'allocation':
                            expected_delta = n * width * row['value_bytes'] if case == 'ready' else 0
                            assert row['requested_bytes'] - row['freed_bytes'] == expected_delta, row
                            if case != 'ready':
                                assert (row['requested_bytes'], row['alloc_calls'], row['realloc_calls']) == (0, 0, 0), row
                        row.update(variant=side, sample=sample, order_position=position)
                        output.write(json.dumps(row, sort_keys=True) + '\n')
                        rows.append(row)
    grouped = []
    for n, width, pattern, case in configurations:
        for side in builds:
            record = dict(n=n, width=width, pattern=pattern, case=case, variant=side)
            for mode, metrics in [('allocation', ['requested_bytes', 'freed_bytes', 'alloc_calls', 'realloc_calls', 'dealloc_calls']), ('time', ['elapsed_ns'])]:
                selected = [row for row in rows if (row['n'], row['width'], row['pattern'], row['case'], row['variant'], row['mode']) == (n, width, pattern, case, side, mode)]
                for metric in metrics:
                    values = [row[metric] for row in selected]
                    record[metric] = dict(median=statistics.median(values), min=min(values), max=max(values))
            grouped.append(record)
    for build in builds.values():
        assert sha(Path(build['binary'])) == build['binary_sha256']
    summary = {
        'scope': 'one actual VM stack resolve call; preparation and verification excluded',
        'allocation_metric': 'successful GlobalAlloc layout requests on the measurement thread; realloc counts full new request and old free; not allocator usable size',
        'time_metric': 'Instant interval; allocation counting disabled, test-only logical counters and System allocator wrapper remain enabled in both variants',
        'unmeasured': ['RSS', 'peak/live retained bytes', 'ordinary CLI', 'allocator usable size', 'cleanup-bearing tasks', 'physical copy traffic'],
        'builds': builds, 'runner_sha256': sha(Path(__file__)),
        'host': platform.platform(), 'architecture': platform.machine(),
        'processes': len(rows), 'verification_processes': len(configurations) * 2,
        'measurement_processes': len(configurations) * 2 * 2 * args.samples,
        'samples_per_variant_per_metric': args.samples, 'measurements': grouped,
    }
    args.output.with_suffix('.summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'{len(rows)} fresh processes: allocation, time and contents verified')


if __name__ == '__main__':
    main()
