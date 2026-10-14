#!/usr/bin/env python3
"""Compare the actual before/after public ListHandle in separate cost series."""
import argparse
import hashlib
import itertools
import json
import os
import platform
import re
import statistics
import subprocess
from pathlib import Path


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for side in ['before', 'after']:
        for mode in ['count', 'time']:
            parser.add_argument(f'--{side}-{mode}', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=6)
    args = parser.parse_args()
    assert platform.system() == 'Darwin'
    assert args.samples >= 2 and args.samples % 2 == 0
    assert 'PROBE_VERIFY' not in os.environ
    builds = {side: {mode: json.loads(getattr(args, f'{side}_{mode}').read_text()) for mode in ['count', 'time']} for side in ['before', 'after']}
    for side in builds.values():
        for mode, build in side.items():
            assert build['allocator_counting'] == (mode == 'count')
            assert sha(Path(build['binary'])) == build['binary_sha256']
    configs = list(itertools.product([10000, 100000], ['int', 'string', 'nested'], ['suffix', 'siblings', 'checkpoint']))
    rows = []
    args.output.parent.mkdir(parents=True, exist_ok=True)
    assert not args.output.with_suffix('.summary.json').exists()
    with args.output.with_suffix('.jsonl').open('x') as out:
        for series in ['verify', 'count', 'time']:
            for n, payload, ownership in configs:
                for sample in range(1 if series == 'verify' else args.samples):
                    order = ['before', 'after'] if sample % 2 == 0 else ['after', 'before']
                    for position, side in enumerate(order):
                        build = builds[side]['time' if series == 'time' else 'count']
                        command = [build['binary'], 'shared', str(n), payload, ownership]
                        env = dict(os.environ)
                        if series == 'verify':
                            env['PROBE_VERIFY'] = '1'
                        if series == 'time':
                            command = ['/usr/bin/time', '-l', *command]
                        result = subprocess.run(command, env=env, capture_output=True, text=True, check=True)
                        parsed = [json.loads(line) for line in result.stdout.splitlines()]
                        assert len(parsed) == 7 and all(row['verification'] == (series == 'verify') for row in parsed)
                        assert all(row['allocator_counting'] == build['allocator_counting'] for row in parsed)
                        assert parsed[-1]['workload_retained_bytes'] == 0
                        assert parsed[1]['allocations'] == parsed[1]['reallocations'] == 0
                        rss = None
                        if series == 'time':
                            matches = re.findall(r'^\s*(\d+)\s+maximum resident set size\s*$', result.stderr, re.MULTILINE)
                            assert len(matches) == 1
                            rss = int(matches[0])
                        for row in parsed:
                            assert (row['n'], row['payload'], row['ownership']) == (n, payload, ownership)
                            if series != 'time':
                                # Count overhead and content checks are not timing results.
                                del row['ns']
                            else:
                                for metric in ['allocations', 'reallocations', 'requested_bytes', 'freed_bytes', 'live_delta_bytes', 'peak_extra_bytes', 'workload_retained_bytes']:
                                    row[metric] = None
                            row.update(series=series, variant=side, sample=sample, order_position=position)
                            if rss is not None:
                                row['process_peak_rss_bytes'] = rss
                            out.write(json.dumps(row, sort_keys=True) + '\n')
                            rows.append(row)
    summaries = []
    for series, n, payload, ownership, side in itertools.product(['count', 'time'], [10000, 100000], ['int', 'string', 'nested'], ['suffix', 'siblings', 'checkpoint'], ['before', 'after']):
        for phase in ['build', 'advance', 'compact', 'drop_root', 'drop_sibling', 'drop_checkpoint', 'drop_last']:
            selected = [row for row in rows if (row['series'], row['n'], row['payload'], row['ownership'], row['variant'], row['phase']) == (series, n, payload, ownership, side, phase)]
            record = dict(series=series, n=n, payload=payload, ownership=ownership, variant=side, phase=phase)
            metrics = ['ns', 'process_peak_rss_bytes'] if series == 'time' else ['requested_bytes', 'freed_bytes', 'allocations', 'reallocations', 'workload_retained_bytes', 'peak_extra_bytes']
            for metric in metrics:
                values = [row[metric] for row in selected]
                record[metric] = dict(median=statistics.median(values), min=min(values), max=max(values))
            summaries.append(record)
    for side in builds.values():
        for build in side.values():
            assert sha(Path(build['binary'])) == build['binary_sha256']
    summary = dict(scope='actual product ListHandle::from_items / tail / drop; historical local chunk model unused',
                   builds=builds, runner_sha256=sha(Path(__file__)), samples=args.samples,
                   verification_processes=2 * len(configs), measurement_processes=2 * 2 * len(configs) * args.samples,
                   rss_scope='whole fresh time-only process, including preparation and all drops',
                   unmeasured=['ordinary CLI', 'physical copy bytes', 'allocator usable size', 'actual VM checkpoint'],
                   measurements=summaries)
    args.output.with_suffix('.summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'{len(rows)} phase rows; 36 verify / {2 * 2 * len(configs) * args.samples} measurement processes')


if __name__ == '__main__':
    main()
