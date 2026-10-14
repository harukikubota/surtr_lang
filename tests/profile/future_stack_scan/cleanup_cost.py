#!/usr/bin/env python3
"""Measure one cleanup-filtered Future snapshot; keep allocation and time separate."""
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
    configurations = [(n, width, pattern, case, cleanup)
                      for n, width, pattern, case, cleanup in itertools.product(
                          [256, 4096], [0, 64], ['repeated', 'alternating', 'distinct'],
                          ['ready', 'running', 'blocked', 'unknown'], [0, 32])
                      if cleanup == 0 or case in ['ready', 'running']]
    configurations += [(n, 0, 'distinct', 'ordinary', cleanup) for n, cleanup in itertools.product([256, 4096], [0, 32])]
    rows = []
    with args.output.with_suffix('.jsonl').open('x') as output:
        # Full contents have their own fresh processes and no cost metrics.
        for mode in ['verify', 'allocation', 'time']:
            for n, width, pattern, case, cleanup in configurations:
                for sample in range(1 if mode == 'verify' else args.samples):
                    order = ['before', 'after'] if sample % 2 == 0 else ['after', 'before']
                    for position, side in enumerate(order):
                        env = dict(os.environ, OI036_COST_PROFILE='1', OI036_N=str(n),
                                   OI036_WIDTH=str(width), OI036_PATTERN=pattern,
                                   OI036_CASE=case, OI036_MODE=mode, OI036_CLEANUP=str(cleanup))
                        command = [builds[side]['binary'], '--exact', TEST,
                                   '--nocapture', '--test-threads=1']
                        result = subprocess.run(command, env=env, capture_output=True,
                                                text=True, check=True)
                        parsed = [json.loads(line) for line in result.stderr.splitlines()
                                  if line.startswith('{')]
                        assert len(parsed) == 1, result.stderr
                        row = parsed[0]
                        assert (row['n'], row['width'], row['pattern'], row['case'], row['mode'], row['cleanup']) == (n, width, pattern, case, mode, cleanup)
                        assert row['contents_verified'] == (mode == 'verify')
                        assert ('elapsed_ns' in row) == (mode == 'time')
                        assert ('requested_bytes' in row) == (mode == 'allocation')
                        ids = {'repeated': 1, 'alternating': 2, 'distinct': n}[pattern]
                        cached = side == 'after' or case == 'ready'
                        expected_lookups = 0 if case == 'ordinary' else (ids if cached or pattern == 'repeated' else n)
                        assert row['ready_lookups'] == expected_lookups, row
                        assert row['source_value_clones'] == 0, row
                        if case == 'ready':
                            visits = cleanup if side == 'after' else ids * cleanup
                        elif case == 'blocked':
                            visits = cleanup + ids if side == 'after' else (
                                cleanup + 1 if pattern == 'repeated' else
                                n * cleanup + (3 * n // 2 if pattern == 'alternating' else n * (n + 1) // 2))
                        else:
                            visits = 0
                        assert row['cleanup_visits'] == visits, row
                        assert row['read_visits'] == (0 if case == 'ordinary' else n), row
                        assert row['write_visits'] == (n if case == 'ready' else 0), row
                        if mode == 'allocation':
                            expected_delta = n * width * row['value_bytes'] if case == 'ready' else 0
                            assert row['requested_bytes'] - row['freed_bytes'] == expected_delta, row
                            if case in ['running', 'unknown', 'ordinary']:
                                assert (row['requested_bytes'], row['alloc_calls'], row['realloc_calls']) == (0, 0, 0), row
                        row.update(variant=side, sample=sample, order_position=position)
                        output.write(json.dumps(row, sort_keys=True) + '\n')
                        rows.append(row)
    grouped = []
    for n, width, pattern, case, cleanup in configurations:
        for side in builds:
            record = dict(n=n, width=width, pattern=pattern, case=case, cleanup=cleanup, variant=side)
            for mode, metrics in [('allocation', ['requested_bytes', 'freed_bytes', 'alloc_calls', 'realloc_calls', 'dealloc_calls']), ('time', ['elapsed_ns'])]:
                selected = [row for row in rows if (row['n'], row['width'], row['pattern'], row['case'], row['cleanup'], row['variant'], row['mode']) == (n, width, pattern, case, cleanup, side, mode)]
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
        'unmeasured': ['RSS', 'peak/live retained bytes', 'ordinary CLI (separate series)', 'allocator usable size', 'physical copy traffic'],
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
