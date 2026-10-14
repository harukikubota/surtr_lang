#!/usr/bin/env python3
"""Separate shared/unique Builder lifetimes and allocator controls in fresh processes."""
import argparse,collections,hashlib,importlib.util,itertools,json,os,statistics,subprocess,tempfile
from pathlib import Path

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--builds',required=True,type=Path);p.add_argument('--runner',required=True,type=Path);p.add_argument('--output',required=True,type=Path);p.add_argument('--samples',type=int,default=6);p.add_argument('--diagnose',action='store_true');a=p.parse_args();assert a.samples>=2 and a.samples%2==0
 spec=importlib.util.spec_from_file_location('historical',a.runner);old=importlib.util.module_from_spec(spec);spec.loader.exec_module(old)
 builds={}
 for side,mode in itertools.product(['baseline','shared'],['time','count']):
  b=old.read_build(a.builds/f'{side}-isolation-{mode}/build.json');b['source_variant']=b['variant'];b['variant']=side;builds[side,mode]=b
 controls={'default':{},'nano_off':{'MallocNanoZone':'0'},'secure_off':{'MallocSecureAllocator':'0','MallocNanoZone':'0'}}
 def validate(rows,b,tasks,length,payload,repeats,verify):
  if verify:assert len(rows)==1 and rows[0]['verified'] is True and rows[0]['workload_live_restored']==b['counted'];return
  phases=old.PHASES if ownership=='sequential' else old.PHASES[:12] if ownership=='shared' else old.PHASES[12:];assert {(r['repeat'],r['phase']) for r in rows}==set(itertools.product(range(repeats),phases));assert len(rows)==len(phases)*repeats
  for r in rows:
   assert (r['tasks'],r['builder_len'],r['payload'],r['copy_mode'],r['allocator_counting'],r['output_len'])==(tasks,length,payload,b['variant'],b['counted'],length+2048)
   if b['counted']:assert r['requested_bytes']-r['freed_bytes']==r['live_delta_bytes']==r['live_end_bytes']-r['live_start_bytes']
   else:assert r['requested_bytes'] is None
  if b['counted']:
   for i in range(repeats):
    rs=[r for r in rows if r['repeat']==i];baseline=rs[0]['live_start_bytes'];assert rs[-1]['live_end_bytes']==baseline
    for first,last in zip(rs,rs[1:]):assert first['live_end_bytes']==last['live_start_bytes']
 old.validate=validate
 a.output.parent.mkdir(parents=True,exist_ok=True);records=[];verified=[];parent={k:v for k,v in os.environ.items() if k.startswith('Malloc')}
 def setenv(control):
  for name in ['MallocNanoZone','MallocSecureAllocator','MallocXZoneEnabled','MallocReportConfig']:os.environ.pop(name,None)
  os.environ.update(controls[control]);os.environ['SURTR_VM_MEMORY_OWNERSHIP']=ownership
 def run(side,mode,payload,repeats,control,verify,barrier):
  setenv(control);b=builds[side,mode];rows=old.run_probe(b,32,16384,payload,repeats,verify,barrier)
  return dict(side=side,mode=mode,ownership=ownership,payload=payload,repeats=repeats,control=control,verified=verify,barrier=barrier,binary_sha256=b['binary_sha256'],child_allocator_environment={k:v for k,v in os.environ.items() if k.startswith('Malloc')},rows=rows)
 for ownership,payload,side,mode in itertools.product(['shared','unique','sequential'],['int','string'],['baseline','shared'],['time','count']):verified.append(run(side,mode,payload,1,'default',True,False))
 a.output.with_suffix('.verify.json').write_text(json.dumps(verified,indent=2)+'\n')
 if a.diagnose:
  # Heavy OS diagnostics are a separate series; never include these elapsed/peak values in timings.
  with a.output.with_suffix('.diagnostics.jsonl').open('x') as f:
   ownership='sequential'
   for control,side in itertools.product(['default','secure_off'],['baseline','shared']):
    setenv(control);b=builds[side,'time'];env=dict(os.environ,MallocReportConfig='1',SURTR_VM_MEMORY_PROFILE='1',SURTR_VM_MEMORY_TASKS='32',SURTR_VM_MEMORY_LEN='16384',SURTR_VM_MEMORY_PAYLOAD='int',SURTR_VM_MEMORY_REPEATS='1',SURTR_VM_MEMORY_COPY_MODE=side,SURTR_VM_MEMORY_BARRIER='1',PROBE_VERIFY='0')
    with tempfile.TemporaryFile(mode='w+t') as err:
     proc=subprocess.Popen([b['binary'],'--exact',old.TEST,'--nocapture','--test-threads=1'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=err,text=True)
     for line in proc.stdout:
      if line.startswith(('START ','READY ')):
       phase='startup' if line.startswith('START') else line.strip().split()[-1]
       if phase in ['startup','prepare_shared','complete_shared','drop_changed','drop_shared_vm','prepare_unique','unique_complete','drop_unique_vm']:
        r=subprocess.run(['/usr/bin/vmmap','-summary',str(proc.pid)],text=True,capture_output=True);name=f'{a.output.name}-{control}-{side}-{phase}.vmmap.txt';(a.output.parent/name).write_text(r.stdout+r.stderr);f.write(json.dumps(dict(control=control,side=side,phase=phase,command=['vmmap','-summary',str(proc.pid)],exit_code=r.returncode,log=name,sha256=sha(a.output.parent/name),ps_resident_bytes=old.rss(proc.pid),binary_sha256=b['binary_sha256'],child_allocator_environment={k:v for k,v in env.items() if k.startswith('Malloc')}))+'\n');f.flush()
       proc.stdin.write('go\n');proc.stdin.flush()
     assert proc.wait()==0;err.seek(0);(a.output.parent/f'{a.output.name}-{control}-{side}.stderr').write_text(err.read())
  return
 with a.output.with_suffix('.jsonl').open('x') as f:
  for ownership,payload,control,repeats in itertools.product(['shared','unique','sequential'],['int','string'],controls,[1,8]):
   for sample in range(a.samples):
    for side in ['baseline','shared'] if sample%2==0 else ['shared','baseline']:
     record=run(side,'time',payload,repeats,control,False,False);record['sample']=sample;f.write(json.dumps(record,sort_keys=True)+'\n');f.flush();records.append(record)
   for side in ['baseline','shared']:
    record=run(side,'count',payload,repeats,control,False,False);record['sample']=0;f.write(json.dumps(record,sort_keys=True)+'\n');f.flush();records.append(record)
   print(ownership,payload,control,repeats,'complete',flush=True)
 groups=collections.defaultdict(list)
 for r in records:groups[tuple(r[k] for k in ['side','mode','ownership','payload','control','repeats'])].append(r)
 summaries=[]
 for key,items in groups.items():
  row=dict(zip(['side','mode','ownership','payload','control','repeats'],key));vals=[x['rows'][0]['process_peak_rss_bytes'] for x in items if x['rows'][0]['process_peak_rss_bytes'] is not None];row['rss']=dict(median=statistics.median(vals),min=min(vals),max=max(vals)) if vals else None;row['phases']={}
  for phase in sorted({x['phase'] for x in items[0]['rows']}):
   out={}
   for metric in ['elapsed_ns','requested_bytes','freed_bytes','live_delta_bytes','peak_extra_bytes','live_end_bytes']:
    perproc=[[r[metric] for r in x['rows'] if r['phase']==phase] for x in items];values=[sum(v)/len(v) for v in perproc] if perproc[0][0] is not None else [];out[metric]=dict(median=statistics.median(values),min=min(values),max=max(values)) if values else None
   row['phases'][phase]=out
  summaries.append(row)
 a.output.with_suffix('.summary.json').write_text(json.dumps(dict(measurements=summaries,builds=list(builds.values()),runner_sha256=sha(Path(__file__)),historical_runner_sha256=sha(a.runner),parent_allocator_environment=parent,controls=controls,scope='No-barrier timing/peak; allocation ledger independent. vmmap diagnostics separately.'),indent=2)+'\n')
if __name__=='__main__':main()
