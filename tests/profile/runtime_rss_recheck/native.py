#!/usr/bin/env python3
"""Recheck historical native RSS with frozen artifacts and explicit child controls."""
import argparse,collections,hashlib,itertools,json,os,re,statistics,subprocess
from pathlib import Path

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--builds',required=True,type=Path);p.add_argument('--output',required=True,type=Path);p.add_argument('--samples',type=int,default=6);a=p.parse_args();assert a.samples>=2 and a.samples%2==0
 builds={}
 for kind in ['future','map']:
  for side,mode in itertools.product(['before','after'],['time','count']):
   record=json.loads((a.builds/f'{kind}-{side}-{mode}/build.json').read_text());assert sha(Path(record['binary']))==record['binary_sha256'];assert all(sha(Path(record['source_root'])/x)==v for x,v in record['source_sha256'].items());builds[kind,side,mode]=record
 controls={'default':{},'nano_off':{'MallocNanoZone':'0'},'secure_off':{'MallocSecureAllocator':'0','MallocNanoZone':'0'}}
 configs=[('future',c,n,k) for c,n,k in itertools.product(['chain','nested'],[1000,10000],['hashmap'])]+[('future','chain',10000,'tuple')]+[('map',c,n,k) for c,n,k in itertools.product(['vm_distinct','vm_map_values','sort'],[1024],['string','int'])]
 a.output.parent.mkdir(parents=True,exist_ok=True);rows=[];verified=[]
 def run(config,side,mode,control,verify):
  kind,case,n,payload=config;b=builds[kind,side,mode];env=dict(os.environ);env.pop('PROBE_VERIFY',None)
  for name in ['MallocNanoZone','MallocSecureAllocator','MallocXZoneEnabled','MallocReportConfig']:env.pop(name,None)
  env.update(controls[control]);
  if verify:env['PROBE_VERIFY']='1'
  cmd=[b['binary'],case,str(n),payload];
  if mode=='time' and not verify:cmd=['/usr/bin/time','-l',*cmd]
  r=subprocess.run(cmd,env=env,text=True,capture_output=True);assert r.returncode==0,(cmd,r.stderr)
  data=[json.loads(l) for l in r.stdout.splitlines()];assert len(data)==(6 if kind=='future' else 3);phases={x['phase'] for x in data};assert phases==({'prepare_bytecode','prepare_vm','run','extract_output','drop_vm','drop_output'} if kind=='future' else {'prepare','run','drop'})
  for x in data:
   assert (x['case'],x['n'],x['payload'],x['allocator_counting'],x['correctness_verification'])==(case,n,payload,mode=='count',verify)
   if mode=='count':assert x['requested_bytes']-x['freed_bytes']==x['live_delta_bytes']
   else:assert x['requested_bytes'] is None
  peaks=re.findall(r'^\s*(\d+)\s+maximum resident set size\s*$',r.stderr,re.M);assert len(peaks)==(1 if mode=='time' and not verify else 0)
  return dict(kind=kind,side=side,mode=mode,control=control,case=case,n=n,payload=payload,verified=verify,binary_sha256=b['binary_sha256'],child_allocator_environment={k:v for k,v in env.items() if k.startswith('Malloc')},process_peak_rss_bytes=int(peaks[0]) if peaks else None,rows=data,stderr=r.stderr)
 for cfg in configs:
  for side,mode in itertools.product(['before','after'],['time','count']):verified.append(run(cfg,side,mode,'default',True))
 a.output.with_suffix('.verify.json').write_text(json.dumps(verified,indent=2)+'\n')
 with a.output.with_suffix('.jsonl').open('x') as f:
  for cfg,control in itertools.product(configs,controls):
   for sample in range(a.samples):
    for side in ['before','after'] if sample%2==0 else ['after','before']:
     row=run(cfg,side,'time',control,False);row['sample']=sample;f.write(json.dumps(row,sort_keys=True)+'\n');f.flush();rows.append(row)
   # Counting is independent of both full verification and RSS process sampling.
   for side in ['before','after']:
    row=run(cfg,side,'count',control,False);row['sample']=0;f.write(json.dumps(row,sort_keys=True)+'\n');f.flush();rows.append(row)
   print(cfg,control,'complete',flush=True)
 groups=collections.defaultdict(list)
 for row in rows:groups[tuple(row[k] for k in ['kind','case','n','payload','control','side','mode'])].append(row)
 summaries=[]
 for key,items in groups.items():
  item=dict(zip(['kind','case','n','payload','control','side','mode'],key));item['samples']=len(items)
  vals=[x['process_peak_rss_bytes'] for x in items if x['process_peak_rss_bytes'] is not None];item['rss']={'median':statistics.median(vals),'min':min(vals),'max':max(vals)} if vals else None
  item['phases']={}
  for phase in items[0]['rows']:
   name=phase['phase'];out={}
   for metric in ['ns','requested_bytes','freed_bytes','live_delta_bytes','peak_extra_bytes']:
    vals=[next(z for z in x['rows'] if z['phase']==name)[metric] for x in items];out[metric]=dict(median=statistics.median(vals),min=min(vals),max=max(vals)) if vals[0] is not None else None
   item['phases'][name]=out
  summaries.append(item)
 a.output.with_suffix('.summary.json').write_text(json.dumps(dict(builds=list(builds.values()),runner_sha256=sha(Path(__file__)),measurements=summaries,controls=controls,control_effectiveness='Environment requests only; separate vmmap diagnostic required.'),indent=2)+'\n')
 for b in builds.values():assert sha(Path(b['binary']))==b['binary_sha256']
if __name__=='__main__':main()
