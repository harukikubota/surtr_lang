#!/usr/bin/env python3
"""Verify source-result clone counts and cleanup/delivery through actual Eldr VM calls."""
import argparse
import hashlib
import json
import os
import subprocess
from pathlib import Path

TEST = 'vm::future_stack_tests::future_ready_value_clone_profile'
CASES = ['raw_ready', 'raw_cancelled', 'task_ready', 'task_cancelled',
         'blocked_ready', 'blocked_cancelled', 'deliverable_ready',
         'deliverable_cancelled', 'running', 'unknown']


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    builds = {side: json.loads(path.read_text())
              for side, path in [('before', args.before), ('after', args.after)]}
    for record in builds.values():
        assert sha(Path(record['binary'])) == record['binary_sha256']
    args.output.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    with args.output.with_suffix('.jsonl').open('x') as output:
        for queries in [1, 256, 4096]:
            for width in [0, 64]:
                for case in CASES:
                    for sample in range(2):
                        for position, side in enumerate(['before', 'after'] if sample == 0 else ['after', 'before']):
                            env = dict(os.environ, OI036_VALUE_CLONE_PROFILE='1', OI036_QUERIES=str(queries),
                                       OI036_WIDTH=str(width), OI036_CASE=case)
                            command = [builds[side]['binary'], '--exact', TEST, '--nocapture', '--test-threads=1']
                            result = subprocess.run(command, env=env, capture_output=True, text=True, check=True)
                            parsed = [json.loads(line) for line in result.stderr.splitlines() if line.startswith('{')]
                            assert len(parsed) == 1, result.stderr
                            row = parsed[0]
                            assert (row['queries'], row['width'], row['case'], row['verified']) == (queries, width, case, True)
                            expected = queries if (side == 'before' and case not in ['running', 'unknown']) or case.startswith('deliverable_') else 0
                            assert row['source_value_clones'] == expected, row
                            row.update(variant=side, sample=sample, order_position=position)
                            output.write(json.dumps(row, sort_keys=True) + '\n')
                            rows.append(row)
    assert len(rows) == 240
    for record in builds.values():
        assert sha(Path(record['binary'])) == record['binary_sha256']
    assert len({(row['context_bytes'], row['vm_bytes']) for row in rows}) == 1
    summary = {
        'scope': 'logical Future-table source Value clones in actual Eldr VM; full result and cleanup transition verification',
        'unmeasured': ['elapsed time', 'allocator requests', 'logical live bytes', 'RSS', 'ordinary CLI', 'nested clone calls'],
        'builds': builds, 'runner_sha256': sha(Path(__file__)),
        'processes': len(rows), 'samples_per_variant': 2,
        'measurements': [row for row in rows if row['sample'] == 0],
    }
    args.output.with_suffix('.summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'{len(rows)} fresh processes: source clone counts, contents and cleanup verified')


if __name__ == '__main__':
    main()
