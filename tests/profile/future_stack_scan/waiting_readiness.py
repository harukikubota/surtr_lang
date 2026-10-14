#!/usr/bin/env python3
"""Compare readiness-only and actual delivery clone counts in waiting Eldr Tasks."""
import argparse
import hashlib
import json
import os
import subprocess
from pathlib import Path

TEST = 'vm::future_stack_tests::waiting_future_readiness_profile'


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
                for case in ['ready', 'cancelled', 'running', 'unknown']:
                    for sample in range(2):
                        for position, side in enumerate(['before', 'after'] if sample == 0 else ['after', 'before']):
                            env = dict(os.environ, OI036_WAITING_PROFILE='1', OI036_QUERIES=str(queries),
                                       OI036_WIDTH=str(width), OI036_CASE=case)
                            command = [builds[side]['binary'], '--exact', TEST, '--nocapture', '--test-threads=1']
                            result = subprocess.run(command, env=env, capture_output=True, text=True, check=True)
                            parsed = [json.loads(line) for line in result.stderr.splitlines() if line.startswith('{')]
                            assert len(parsed) == 1, result.stderr
                            row = parsed[0]
                            assert (row['queries'], row['width'], row['case'], row['verified']) == (queries, width, case, True)
                            ready = case in ['ready', 'cancelled']
                            assert row['query_source_clones'] == (queries if side == 'before' and ready else 0), row
                            assert row['delivery_source_clones'] == ((2 if side == 'before' else 1) if ready else 0), row
                            row.update(variant=side, sample=sample, order_position=position)
                            output.write(json.dumps(row, sort_keys=True) + '\n')
                            rows.append(row)
    assert len(rows) == 96
    for record in builds.values():
        assert sha(Path(record['binary'])) == record['binary_sha256']
    assert len({(row['context_bytes'], row['vm_bytes']) for row in rows}) == 1
    summary = {
        'scope': 'actual waiting-task readiness queries and one driver delivery; logical source Value clones',
        'unmeasured': ['elapsed time', 'allocator requests', 'logical live bytes', 'RSS', 'ordinary CLI', 'nested clone calls'],
        'builds': builds, 'runner_sha256': sha(Path(__file__)),
        'processes': len(rows), 'samples_per_variant': 2,
        'measurements': [row for row in rows if row['sample'] == 0],
    }
    args.output.with_suffix('.summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'{len(rows)} fresh processes: query/delivery clones and task results verified')


if __name__ == '__main__':
    main()
