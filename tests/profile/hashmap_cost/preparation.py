#!/usr/bin/env python3
"""Compare owned preparation and budgeted HashMap operations in real VM runs."""
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
    assert platform.system() == 'Darwin' and 'PROBE_VERIFY' not in os.environ
    assert args.samples >= 2 and args.samples % 2 == 0
    builds = {side: {mode: json.loads(getattr(args, f'{side}_{mode}').read_text()) for mode in ['count', 'time']} for side in ['before', 'after']}
    for side in builds.values():
        for mode, build in side.items():
            assert build['allocator_counting'] == (mode == 'count')
            assert sha(Path(build['binary'])) == build['binary_sha256']
    configs = list(itertools.product(['rust_distinct', 'rust_same', 'sort', 'vm_distinct', 'vm_same', 'vm_map_values'], [256, 512, 1024], ['int', 'string']))
    rows = []
    args.output.parent.mkdir(parents=True, exist_ok=True)
    assert not args.output.with_suffix('.summary.json').exists()
    with args.output.with_suffix('.jsonl').open('x') as out:
        for series in ['verify', 'count', 'time']:
            for case, n, payload in configs:
                for sample in range(1 if series == 'verify' else args.samples):
                    for position, side in enumerate(['before', 'after'] if sample % 2 == 0 else ['after', 'before']):
                        build = builds[side]['time' if series == 'time' else 'count']
                        command = [build['binary'], case, str(n), payload]
                        env = dict(os.environ)
                        if series == 'verify':
                            env['PROBE_VERIFY'] = '1'
                        if series == 'time':
                            command = ['/usr/bin/time', '-l', *command]
                        result = subprocess.run(command, env=env, capture_output=True, text=True, check=True)
                        parsed = [json.loads(line) for line in result.stdout.splitlines()]
                        assert len(parsed) == 3
                        rss = None
                        if series == 'time':
                            matches = re.findall(r'^\s*(\d+)\s+maximum resident set size\s*$', result.stderr, re.MULTILINE)
                            assert len(matches) == 1
                            rss = int(matches[0])
                        for row in parsed:
                            assert (row['case'], row['n'], row['payload']) == (case, n, payload)
                            assert row['correctness_verification'] == (series == 'verify')
                            assert row['allocator_counting'] == build['allocator_counting']
                            assert row['callback_returns_observed'] == (n if case == 'vm_map_values' else 0)
                            assert row['final_output'] == (1 if case.endswith('_same') else n)
                            if series != 'time':
                                del row['ns']
                            else:
                                assert row['requested_bytes'] is None and row['live_delta_bytes'] is None
                            row.update(series=series, variant=side, sample=sample, order_position=position)
                            if rss is not None:
                                row['process_peak_rss_bytes'] = rss
                            out.write(json.dumps(row, sort_keys=True) + '\n')
                            rows.append(row)
                        if series != 'time':
                            assert sum(row['live_delta_bytes'] for row in parsed) == 0
    measurements = []
    for series, config, side, phase in itertools.product(['count', 'time'], configs, ['before', 'after'], ['prepare', 'run', 'drop']):
        case, n, payload = config
        selected = [row for row in rows if (row['series'], row['case'], row['n'], row['payload'], row['variant'], row['phase']) == (series, case, n, payload, side, phase)]
        record = dict(series=series, case=case, n=n, payload=payload, variant=side, phase=phase)
        fields = ['ns', 'process_peak_rss_bytes'] if series == 'time' else ['requested_bytes', 'freed_bytes', 'live_delta_bytes', 'peak_extra_bytes', 'allocations', 'reallocations']
        for field in fields:
            values = [row[field] for row in selected]
            record[field] = dict(median=statistics.median(values), min=min(values), max=max(values))
        measurements.append(record)
    for side in builds.values():
        for build in side.values():
            assert sha(Path(build['binary'])) == build['binary_sha256']
    summary = dict(scope='existing public Rust controls and whole manual-bytecode VM construction/map_values; not an isolated sort interval',
                   builds=builds, runner_sha256=sha(Path(__file__)), samples=args.samples,
                   verification_processes=2 * len(configs), measurement_processes=2 * 2 * len(configs) * args.samples,
                   platform=platform.platform(), allocator_environment={key: value for key, value in os.environ.items() if key.startswith('Malloc')},
                   rss_scope='whole fresh time-only process including preparation, VM boot/verification, asserts and JSON',
                   clone_field='existing_entry_clones_expected is derived for repeated insertion only, not a measured sort/source clone counter',
                   unmeasured=['ordinary CLI performance', 'allocator usable size', 'physical copy bytes', 'RSS cause'], measurements=measurements)
    args.output.with_suffix('.summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'{len(rows)} phase rows; {2 * len(configs)} verify / {2 * 2 * len(configs) * args.samples} measurement processes')


if __name__ == '__main__':
    main()
