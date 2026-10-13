#!/usr/bin/env python3
"""Compare ordinary Task/List literal programs with cached compiler artifacts."""
import argparse, collections, hashlib, json, os, platform, re, statistics, subprocess, tempfile, time
from pathlib import Path


def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--before',required=True,type=Path);p.add_argument('--after',required=True,type=Path)
    p.add_argument('--output',required=True,type=Path);p.add_argument('--samples',type=int,default=6)
    a=p.parse_args()
    if platform.system()!='Darwin' or a.samples<2 or a.samples%2:p.error('Darwin and even samples >=2 required')
    binaries={'before':a.before.resolve(strict=True),'after':a.after.resolve(strict=True)}
    hashes={s:sha(b) for s,b in binaries.items()}
    folder=Path(tempfile.mkdtemp(prefix='surtr-oi036-batch-'))
    env=dict(os.environ,SURTR_RUN_CACHE='1',SURTR_RUN_CACHE_DIR=str(folder/'cache'))
    configs=[(n,case) for n in [1000,10000] for case in ['empty','unawaited_task']]
    sources={}
    for n,case in configs:
        text=('task = Task::async({|| Ok(())})\n' if case=='unawaited_task' else '')
        text+='values = ['+', '.join(map(str,range(n)))+']\nprint(inspect(List::len(values)))\n'
        if case=='unawaited_task':text+='print(inspect(Task::await(task)))\n'
        path=folder/f'{case}_{n}.srt';path.write_text(text);sources[n,case]=path
    def run(side,n,case,measured):
        cmd=[str(binaries[side]),'run',str(sources[n,case]),'--phase-times-json']
        if measured:cmd=['/usr/bin/time','-l',*cmd]
        started=time.perf_counter_ns();r=subprocess.run(cmd,cwd=Path(__file__).resolve().parents[3],env=env,text=True,capture_output=True)
        if r.returncode != 0: raise RuntimeError((side, r.returncode, r.stdout, r.stderr))
        wall=time.perf_counter_ns()-started
        expected=f'{n}\n'+('Ok(())\n' if case=='unawaited_task' else '')
        if r.stdout!=expected:raise RuntimeError((side,r.stdout,r.stderr))
        times=[json.loads(l) for l in r.stderr.splitlines() if l.startswith('{')]
        if len(times)!=1:raise RuntimeError('missing phase timing')
        t=times[0]
        if measured and t['cache']['artifact']!='hit':raise RuntimeError('cold compiler sample')
        row={'variant':side,'n':n,'case':case,'stdout':r.stdout,'execute_us':t['phases']['execute']['duration_us'],'total_us':t['total_us'],'process_wall_ns':wall,'phase_timing':t}
        if measured:
            rss=re.findall(r'^\s*(\d+)\s+maximum resident set size\s*$',r.stderr,re.M)
            if len(rss)!=1:raise RuntimeError('missing Darwin RSS')
            row['process_peak_rss_bytes']=int(rss[0])
        return row
    warmups=[run(side,n,case,False) for n,case in configs for side in binaries]
    groups=collections.defaultdict(list);a.output.parent.mkdir(parents=True,exist_ok=True)
    with a.output.with_suffix('.jsonl').open('x') as raw:
        for n,case in configs:
            for sample in range(a.samples):
                for side in ['before','after'] if sample%2==0 else ['after','before']:
                    row=run(side,n,case,True);row['sample']=sample
                    raw.write(json.dumps(row,sort_keys=True)+'\n');raw.flush();groups[side,n,case].append(row)
            print(n,case,'complete',flush=True)
    summaries=[]
    for (side,n,case),rows in groups.items():
        item={'variant':side,'n':n,'case':case,'samples':len(rows)}
        for k in ['execute_us','total_us','process_wall_ns','process_peak_rss_bytes']:
            vals=[r[k] for r in rows];item[k]={'median':statistics.median(vals),'min':min(vals),'max':max(vals)}
        summaries.append(item)
    if any(sha(b)!=hashes[s] for s,b in binaries.items()):raise RuntimeError('binary changed')
    metadata={'binaries':{s:{'path':str(b),'sha256':hashes[s]} for s,b in binaries.items()},'source':{str(f):{'sha256':sha(f),'text':f.read_text()} for f in sources.values()},'warmups':warmups,'runner_sha256':sha(Path(__file__)),'cache_dir':env['SURTR_RUN_CACHE_DIR'],'allocation_metrics':'not measured','observer':False,'trace':False,'samples':a.samples}
    a.output.with_suffix('.summary.json').write_text(json.dumps({'metadata':metadata,'measurements':summaries},indent=2)+'\n')

if __name__=='__main__':main()
