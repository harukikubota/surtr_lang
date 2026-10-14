#!/usr/bin/env python3
"""Compare real VM Builder completion before/after bounded packed chunks."""
import argparse
import collections
import hashlib
import itertools
import json
import os
import platform
import statistics
import sys
from pathlib import Path

sys.dont_write_bytecode = True
from run import PHASES, COUNTERS, run_probe


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=6)
    parser.add_argument('--repeats', type=int, default=8)
    args = parser.parse_args()
    assert args.samples >= 2 and args.samples % 2 == 0 and args.repeats > 0
    builds = {side: json.loads(path.read_text()) for side, path in [('before', args.before), ('after', args.after)]}
    for build in builds.values():
        assert '--release' in build['command']
        assert sha(Path(build['binary'])) == build['binary_sha256']
    configs = list(itertools.product([1, 32], [1024, 16384], ['int', 'string']))
    rows = []
    args.output.parent.mkdir(parents=True, exist_ok=True)
    assert not args.output.with_suffix('.summary.json').exists()
    with args.output.with_suffix('.jsonl').open('x') as out:
        for verify in [True, False]:
            for tasks, length, payload in configs:
                for sample in range(1 if verify else args.samples):
                    for position, side in enumerate(['before', 'after'] if sample % 2 == 0 else ['after', 'before']):
                        parsed = run_probe(Path(builds[side]['binary']), tasks, length, payload,
                                           1 if verify else args.repeats, verify, 'shared')
                        for row in parsed:
                            row.update(variant=side, sample=sample, order_position=position, verification=verify)
                            out.write(json.dumps(row, sort_keys=True) + '\n')
                            rows.append(row)
    groups = collections.defaultdict(list)
    for row in rows:
        if not row['verification']:
            groups[(row['tasks'], row['builder_len'], row['payload'], row['phase'], row['variant'])].append(row)
    summaries = []
    for key, values in sorted(groups.items()):
        tasks, length, payload, phase, side = key
        per_process = collections.defaultdict(list)
        for row in values:
            per_process[row['sample']].append(row)
        times = [sum(row['elapsed_ns'] for row in process) / args.repeats for process in per_process.values()]
        rss = [process[0]['process_peak_rss_bytes'] for process in per_process.values()]
        record = dict(tasks=tasks, builder_len=length, payload=payload, phase=phase, variant=side,
                      elapsed_ns=dict(median=statistics.median(times), min=min(times), max=max(times)),
                      process_peak_rss_bytes=dict(median=statistics.median(rss), min=min(rss), max=max(rss)))
        for metric in COUNTERS:
            counts = [row[metric] for row in values]
            record[metric] = dict(min=min(counts), max=max(counts))
        summaries.append(record)
    assert len(summaries) == len(configs) * len(PHASES) * 2
    for build in builds.values():
        assert sha(Path(build['binary'])) == build['binary_sha256']
    summary = dict(scope='real suspended VM Tasks, checkpoint/resume/shared and unique completion',
                   builds=builds, runner_sha256=sha(Path(__file__)), probe_runner_sha256=sha(Path(__file__).with_name('run.py')),
                   samples=args.samples, repeats=args.repeats, verification_processes=2 * len(configs),
                   measurement_processes=2 * len(configs) * args.samples,
                   logical_counters=True, allocator_counting=False,
                   allocator_environment={key: value for key, value in os.environ.items() if key.startswith('Malloc')},
                   platform=platform.platform(), rss_scope='whole fresh process including preparation, all phases, repeats, asserts and JSON',
                   unmeasured=['allocation requests', 'logical live bytes', 'ordinary CLI', 'allocator usable size'], summaries=summaries)
    args.output.with_suffix('.summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'{len(rows)} rows; {2 * len(configs)} verification and {2 * len(configs) * args.samples} measured processes')


if __name__ == '__main__':
    main()
