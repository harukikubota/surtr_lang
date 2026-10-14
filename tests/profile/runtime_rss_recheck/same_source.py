#!/usr/bin/env python3
"""Compare binaries rebuilt from identical recorded source bytes through path aliases."""
import argparse,hashlib,json,os,re,statistics,subprocess
from pathlib import Path

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__)
 for name in ['old','new','old-count','new-count']:p.add_argument('--'+name,required=True,type=Path)
 p.add_argument('--output',required=True,type=Path);p.add_argument('--samples',type=int,default=6);a=p.parse_args();builds={name:json.loads(getattr(a,name.replace('-','_')).read_text()) for name in ['old','new','old-count','new-count']};assert a.samples>=2 and a.samples%2==0
 assert all(b['source_sha256']==builds['old']['source_sha256'] for b in builds.values())
 for b in builds.values():assert sha(Path(b['binary']))==b['binary_sha256']
 def run(name,case,verify):
  b=builds[name];count=name.endswith('count');env=dict(os.environ);env.pop('PROBE_VERIFY',None)
  for key in ['MallocNanoZone','MallocSecureAllocator','MallocXZoneEnabled','MallocReportConfig']:env.pop(key,None)
  if verify:env['PROBE_VERIFY']='1'
  cmd=[b['binary'],case,'10000','hashmap'];
  if not verify and not count:cmd=['/usr/bin/time','-l',*cmd]
  r=subprocess.run(cmd,env=env,text=True,capture_output=True);assert r.returncode==0,(cmd,r.stderr);rows=[json.loads(l) for l in r.stdout.splitlines()];assert len(rows)==6 and all(x['allocator_counting']==count and x['correctness_verification']==verify for x in rows)
  peak=re.findall(r'^\s*(\d+)\s+maximum resident set size\s*$',r.stderr,re.M);assert len(peak)==int(not verify and not count)
  return dict(build=name,case=case,n=10000,payload='hashmap',verified=verify,binary_sha256=b['binary_sha256'],rows=rows,process_peak_rss_bytes=int(peak[0]) if peak else None)
 verifies=[run(name,case,True) for case in ['chain','nested'] for name in builds];a.output.parent.mkdir(parents=True,exist_ok=True);a.output.with_suffix('.verify.json').write_text(json.dumps(verifies,indent=2)+'\n');rs=[]
 with a.output.with_suffix('.jsonl').open('x') as f:
  for case in ['chain','nested']:
   for sample in range(a.samples):
    for name in ['old','new'] if sample%2==0 else ['new','old']:
     r=run(name,case,False);r['sample']=sample;rs.append(r);f.write(json.dumps(r)+'\n');f.flush()
   counts=[run(name,case,False) for name in ['old-count','new-count']]
   for phase in range(6):
    for field in ['requested_bytes','freed_bytes','live_delta_bytes','peak_extra_bytes']:assert counts[0]['rows'][phase][field]==counts[1]['rows'][phase][field]
   for r in counts:rs.append(r);f.write(json.dumps(r)+'\n');f.flush()
 out=[]
 for case in ['chain','nested']:
  for name in ['old','new']:
   rows=[x for x in rs if x['build']==name and x['case']==case];vals=[x['process_peak_rss_bytes'] for x in rows];runvals=[next(z for z in x['rows'] if z['phase']=='run')['ns'] for x in rows];out.append(dict(case=case,build=name,rss=dict(median=statistics.median(vals),min=min(vals),max=max(vals)),run_ns=dict(median=statistics.median(runvals),min=min(runvals),max=max(runvals))))
 a.output.with_suffix('.summary.json').write_text(json.dumps(dict(builds=builds,measurements=out,all_recorded_source_bytes_equal=True,all_counted_phase_ledgers_equal=True,runner_sha256=sha(Path(__file__)),scope='Same physical root; source and feature flags equal, command path aliases differ. Not a bit-identical artifact reconstruction.'),indent=2)+'\n')
if __name__=='__main__':main()
