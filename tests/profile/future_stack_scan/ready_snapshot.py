#!/usr/bin/env python3
"""Compare stack readiness calls and source clones across repeated and distinct IDs."""
import argparse
import hashlib
import json
import os
import subprocess
from pathlib import Path

TEST = 'vm::future_stack_tests::repeated_future_lookup_profile'
CASES = ['running', 'ready', 'cancelled', 'unknown', 'future_result', 'empty_slots']


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
        for n in [1, 256, 4096]:
            for case in CASES:
                for pattern in ['repeated', 'alternating', 'distinct']:
                    for sample in range(2):
                        for position, side in enumerate(['before', 'after'] if sample == 0 else ['after', 'before']):
                            env = dict(os.environ, OI036_LOOKUP_PROFILE='1', OI036_N=str(n), OI036_CASE=case,
                                       OI036_PATTERN=pattern, OI036_ALTERNATING=str(int(pattern == 'alternating')))
                            command = [builds[side]['binary'], '--exact', TEST, '--nocapture', '--test-threads=1']
                            result = subprocess.run(command, env=env, capture_output=True, text=True, check=True)
                            parsed = [json.loads(line) for line in result.stderr.splitlines() if line.startswith('{')]
                            assert len(parsed) == 1, result.stderr
                            row = parsed[0]
                            assert (row['n'], row['case'], row['pattern'], row['verified']) == (n, case, pattern, True)
                            pending = case != 'empty_slots'
                            ready = case in ['ready', 'cancelled', 'future_result']
                            expected = 0 if not pending else (1 if pattern == 'repeated' else n)
                            if side == 'after' and pattern == 'alternating' and ready:
                                expected = min(n, 2)
                            assert row['ready_lookups'] == expected, row
                            assert row['source_value_clones'] == (expected if ready else 0), row
                            assert row['table_empty_checks'] == int(pending), row
                            assert row['read_visits'] == (n if pending else 0), row
                            assert row['write_visits'] == (n if ready else 0), row
                            row.update(variant=side, sample=sample, order_position=position)
                            output.write(json.dumps(row, sort_keys=True) + '\n')
                            rows.append(row)
    assert len(rows) == 216
    for record in builds.values():
        assert sha(Path(record['binary'])) == record['binary_sha256']
    assert len({(row['context_bytes'], row['vm_bytes']) for row in rows}) == 1
    summary = {
        'scope': 'actual stack resolution readiness calls and source Value clones; full stack contents and one-step Future results verified',
        'unmeasured': ['elapsed time', 'allocator requests', 'logical live bytes', 'RSS', 'ordinary CLI', 'nested clone calls', 'slot clone calls'],
        'builds': builds, 'runner_sha256': sha(Path(__file__)),
        'processes': len(rows), 'samples_per_variant': 2,
        'measurements': [row for row in rows if row['sample'] == 0],
    }
    args.output.with_suffix('.summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'{len(rows)} fresh processes: readiness/source clones and all slot values verified')


if __name__ == '__main__':
    main()
