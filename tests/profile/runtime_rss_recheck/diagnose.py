#!/usr/bin/env python3
"""Record resident bytes before and after heavy vmmap inspection at each VM boundary."""
import argparse,hashlib,importlib.util,json,os,subprocess,tempfile
from pathlib import Path

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--builds',required=True,type=Path);p.add_argument('--runner',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args();spec=importlib.util.spec_from_file_location('historical',a.runner);old=importlib.util.module_from_spec(spec);spec.loader.exec_module(old);a.output.parent.mkdir(parents=True,exist_ok=True)
 with a.output.with_suffix('.jsonl').open('x') as f:
  for control in ['default','secure_off']:
   for side in ['baseline','shared']:
    b=old.read_build(a.builds/f'{side}-isolation-time/build.json');env=dict(os.environ,SURTR_VM_MEMORY_OWNERSHIP='sequential',SURTR_VM_MEMORY_PROFILE='1',SURTR_VM_MEMORY_TASKS='32',SURTR_VM_MEMORY_LEN='16384',SURTR_VM_MEMORY_PAYLOAD='int',SURTR_VM_MEMORY_REPEATS='1',SURTR_VM_MEMORY_COPY_MODE=side,SURTR_VM_MEMORY_BARRIER='1',PROBE_VERIFY='0',MallocReportConfig='1')
    for name in ['MallocNanoZone','MallocSecureAllocator','MallocXZoneEnabled']:env.pop(name,None)
    if control=='secure_off':env.update(MallocSecureAllocator='0',MallocNanoZone='0')
    with tempfile.TemporaryFile(mode='w+t') as err:
     proc=subprocess.Popen([b['binary'],'--exact',old.TEST,'--nocapture','--test-threads=1'],env=env,text=True,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=err)
     for line in proc.stdout:
      if line.startswith(('START ','READY ')):
       phase='startup' if line.startswith('START') else line.strip().split()[-1]
       if phase in ['startup','prepare_shared','complete_shared','drop_changed','drop_shared_vm','prepare_unique','unique_complete','drop_unique_vm']:
        before=old.rss(proc.pid);r=subprocess.run(['/usr/bin/vmmap','-summary',str(proc.pid)],text=True,capture_output=True);after=old.rss(proc.pid);name=f'{a.output.name}-{control}-{side}-{phase}.vmmap.txt';(a.output.parent/name).write_text(r.stdout+r.stderr)
        f.write(json.dumps(dict(control=control,side=side,phase=phase,exit_code=r.returncode,command=['vmmap','-summary',str(proc.pid)],log=name,log_sha256=sha(a.output.parent/name),ps_before_bytes=before,ps_after_bytes=after,binary_sha256=b['binary_sha256'],runner_sha256=sha(Path(__file__)),child_allocator_environment={k:v for k,v in env.items() if k.startswith('Malloc')}))+'\n');f.flush()
       proc.stdin.write('go\n');proc.stdin.flush()
     assert proc.wait()==0;err.seek(0);(a.output.parent/f'{a.output.name}-{control}-{side}.stderr').write_text(err.read())
if __name__=='__main__':main()
