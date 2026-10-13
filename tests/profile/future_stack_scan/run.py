#!/usr/bin/env python3
"""Compare saved release unit-test binaries; verify contents in separate processes."""
import argparse, collections, hashlib, json, os, re, statistics, subprocess
from pathlib import Path

TEST = 'vm::future_stack_tests::nonempty_future_stack_profile'
CASES = ['empty', 'unrelated_running', 'unrelated_ready', 'pending_ready', 'pending_running']

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()

def run(binary, n, case, verify):
    env = dict(os.environ, OI036_PROFILE='1', OI036_N=str(n), OI036_CASE=case,
               PROBE_VERIFY='1' if verify else '0')
    cmd = [str(binary), '--exact', TEST, '--nocapture', '--test-threads=1']
    if not verify: cmd = ['/usr/bin/time', '-l', *cmd]
    p = subprocess.run(cmd, env=env, capture_output=True, text=True, check=True)
    rows = [json.loads(l) for l in p.stderr.splitlines() if l.startswith('{')]
    if len(rows) != 1: raise RuntimeError(p.stderr)
    row = rows[0]
    if (row['case'], row['n'], row['verify']) != (case, n, verify): raise RuntimeError(row)
    if not verify:
        rss = re.findall(r'^\s*(\d+)\s+maximum resident set size\s*$', p.stderr, re.M)
        if len(rss) != 1: raise RuntimeError('missing Darwin RSS')
        row['process_peak_rss_bytes'] = int(rss[0])
    return row

def check(row, side):
    n, case = row['n'], row['case']
    expected_read = 0 if case == 'empty' else n*(n+1)//2+1
    if case.startswith('pending_'): expected_read = (n+1)*(n+2)//2+2
    expected_write = 1 if case == 'pending_ready' else 0
    if side == 'after' and case != 'pending_running':
        expected_read = 1 if case == 'pending_ready' else 0
    if (row['read_visits'], row['write_visits']) != (expected_read, expected_write):
        raise RuntimeError((side, row, expected_read, expected_write))

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--before',required=True,type=Path);p.add_argument('--after',required=True,type=Path)
    p.add_argument('--output',required=True,type=Path);p.add_argument('--samples',type=int,default=6)
    a=p.parse_args()
    if a.samples<2 or a.samples%2: p.error('samples must be positive and even')
    builds={s:json.loads(path.read_text()) for s,path in [('before',a.before),('after',a.after)]}
    binaries={s:Path(next(r['binary'] for r in b['records'] if r['artifact']['target']['name']=='eldr')) for s,b in builds.items()}
    for side,b in builds.items():
        record=next(r for r in b['records'] if r['artifact']['target']['name']=='eldr')
        if sha(binaries[side])!=record['binary_sha256']:raise RuntimeError('binary hash mismatch')
    groups=collections.defaultdict(list);verify=[]
    a.output.parent.mkdir(parents=True,exist_ok=True)
    with a.output.with_suffix('.jsonl').open('x') as raw:
        for n in [1000,10000]:
            for case in CASES:
                for sample in range(a.samples):
                    for position,side in enumerate(['before','after'] if sample%2==0 else ['after','before']):
                        row=run(binaries[side],n,case,False);check(row,side)
                        row.update(variant=side,sample=sample,order_position=position)
                        raw.write(json.dumps(row,sort_keys=True)+'\n');raw.flush()
                        groups[(side,n,case)].append(row)
                for side in binaries:
                    row=run(binaries[side],n,case,True);check(row,side)
                    row.pop('elapsed_ns');row['variant']=side;verify.append(row)
                print(n,case,'complete',flush=True)
    summaries=[]
    for (side,n,case),rows in groups.items():
        summary={'variant':side,'n':n,'case':case,'samples':len(rows)}
        for key in ['elapsed_ns','read_visits','write_visits','process_peak_rss_bytes','context_bytes','vm_bytes']:
            vals=[r[key] for r in rows];summary[key]={'min':min(vals),'median':statistics.median(vals),'max':max(vals)}
        summaries.append(summary)
    for side,b in builds.items():
        rec=next(r for r in b['records'] if r['artifact']['target']['name']=='eldr')
        if sha(binaries[side])!=rec['binary_sha256']:raise RuntimeError('binary changed')
    a.output.with_suffix('.summary.json').write_text(json.dumps({'builds':builds,'runner_sha256':sha(Path(__file__)),'measurements':summaries},indent=2)+'\n')
    a.output.with_name(a.output.name+'_verify.json').write_text(json.dumps(verify,indent=2)+'\n')

if __name__=='__main__':main()
