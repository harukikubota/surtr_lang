#!/usr/bin/env python3
"""Compare historical Vec and current integrated Builder through ordinary Rune."""
import argparse,collections,hashlib,itertools,json,os,re,statistics,subprocess,tempfile
from pathlib import Path

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--before',required=True,type=Path);p.add_argument('--after',required=True,type=Path);p.add_argument('--output',required=True,type=Path);p.add_argument('--samples',default=6,type=int);a=p.parse_args();assert a.samples>=2 and a.samples%2==0
 builds={side:json.loads(getattr(a,side).read_text()) for side in ['before','after']}
 for b in builds.values():assert sha(Path(b['binary']))==b['binary_sha256'];assert all(sha(Path(b['source_root'])/x)==v for x,v in b['source_sha256'].items())
 folder=Path(tempfile.mkdtemp(prefix='surtr-builder-batch-'));configs=list(itertools.product([8,256,10000],['int','string'],['map','flat_one','flat_two']));sources={};env=dict(os.environ,SURTR_RUN_CACHE='1',SURTR_RUN_CACHE_DIR=str(folder/'cache'))
 for n,kind,case in configs:
  vals=[str(i) if kind=='int' else json.dumps(f'v{i:05}') for i in range(n)];initial='values = ['+', '.join(vals)+']\n';call='List::map(values, {|x| x})' if case=='map' else 'List::flat_map(values, {|x| '+('[x]' if case=='flat_one' else '[x, x]')+'})';prefix=initial+'output = '+call+'\n';paths={}
  for verify in [False,True]:
   text=prefix+('List::map(output, {|x| print(inspect(x)); x})\n' if verify else 'print(inspect(List::len(output)))\n');path=folder/f'{n}_{kind}_{case}_{verify}.srt';path.write_text(text);paths[verify]=path
  expected=''.join(v+'\n' for v in vals for _ in range(2 if case=='flat_two' else 1));sources[n,kind,case]=dict(paths=paths,expected=expected)
 def run(side,cfg,verify,measure):
  b=builds[side];n,kind,case=cfg;command=[b['binary'],'run',str(sources[cfg]['paths'][verify]),'--phase-times-json'];
  if measure:command=['/usr/bin/time','-l',*command]
  r=subprocess.run(command,cwd=b['source_root'],env=env,text=True,capture_output=True);assert r.returncode==0,(cfg,side,r.stderr)
  expected=sources[cfg]['expected'] if verify else str(n*(2 if case=='flat_two' else 1))+'\n';assert r.stdout==expected,(cfg,side,r.stdout[:300],expected[:300])
  times=[json.loads(l) for l in r.stderr.splitlines() if l.startswith('{')];assert len(times)==1;t=times[0]
  if measure:assert t['cache']['artifact']=='hit'
  peaks=re.findall(r'^\s*(\d+)\s+maximum resident set size\s*$',r.stderr,re.M);assert len(peaks)==int(measure)
  return dict(side=side,n=n,payload=kind,case=case,verified=verify,stdout_sha256=hashlib.sha256(r.stdout.encode()).hexdigest(),execute_us=t['phases']['execute']['duration_us'],total_us=t['total_us'],phase_timing=t,process_peak_rss_bytes=int(peaks[0]) if peaks else None)
 a.output.parent.mkdir(parents=True,exist_ok=True);verification=[run(side,cfg,True,False) for cfg in configs for side in builds];warmups=[run(side,cfg,False,False) for cfg in configs for side in builds];rows=[]
 a.output.with_suffix('.verify.json').write_text(json.dumps(verification,indent=2)+'\n')
 with a.output.with_suffix('.jsonl').open('x') as f:
  for cfg in configs:
   for sample in range(a.samples):
    for side in ['before','after'] if sample%2==0 else ['after','before']:
     row=run(side,cfg,False,True);row['sample']=sample;rows.append(row);f.write(json.dumps(row,sort_keys=True)+'\n');f.flush()
   print(cfg,'complete',flush=True)
 groups=collections.defaultdict(list)
 for r in rows:groups[r['side'],r['n'],r['payload'],r['case']].append(r)
 summary=[]
 for key,items in groups.items():
  row=dict(zip(['side','n','payload','case'],key))
  for metric in ['execute_us','total_us','process_peak_rss_bytes']:
   vals=[r[metric] for r in items];row[metric]=dict(median=statistics.median(vals),min=min(vals),max=max(vals))
  summary.append(row)
 metadata=dict(builds=builds,source={str(p):dict(sha256=sha(p),text=p.read_text()) for s in sources.values() for p in s['paths'].values()},warmups=warmups,runner_sha256=sha(Path(__file__)),cache_dir=env['SURTR_RUN_CACHE_DIR'],allocation_metrics='not measured',scope='Historical integrated CLI comparison; current binary contains additional runtime/compiler changes, not isolated Builder causality.')
 a.output.with_suffix('.summary.json').write_text(json.dumps(dict(metadata=metadata,measurements=summary),indent=2)+'\n')
 for b in builds.values():assert sha(Path(b['binary']))==b['binary_sha256']
if __name__=='__main__':main()
