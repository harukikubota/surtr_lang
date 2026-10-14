from pathlib import Path
import subprocess,json,time,hashlib,os,itertools
base=Path('/tmp/surtr-runtime-six');dest=base/'rss-results';runner=base/'rss-historical/shared/tests/profile/checkpoint_builder_vm_memory/run.py';records=base/'rss-rebuilt'
for name,modes,samples,flags in [('original-verify',['time','count'],1,['--verify']),('original-time',['time'],4,['--no-barrier']),('original-count',['count'],2,['--no-barrier'])]:
 cmd=['python3',str(runner),'run']
 for mode,side in itertools.product(modes,['baseline','shared']):cmd+=['--build',str(records/f'{side}-{mode}/build.json')]
 cmd+=['--output',str(dest/name),'--samples',str(samples),'--tasks','32','--length','16384','--repeats','1','8','--payloads','int','string',*flags];print('start',name,flush=True);start=time.monotonic()
 with (dest/(name+'.log')).open('w') as f:r=subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT)
 (dest/(name+'-result.json')).write_text(json.dumps(dict(command=cmd,exit_code=r.returncode,wall_seconds=time.monotonic()-start),indent=2)+'\n');r.check_returncode();print('done',name,flush=True)
configrows=[]
for kind,side,control in itertools.product(['future','map'],['before','after'],['default','nano_off','secure_off']):
 b=json.loads((records/f'{kind}-{side}-time/build.json').read_text());env=dict(os.environ,PROBE_VERIFY='1',MallocReportConfig='1');
 for name in ['MallocNanoZone','MallocSecureAllocator','MallocXZoneEnabled']:env.pop(name,None)
 if control!='default':env['MallocNanoZone']='0'
 if control=='secure_off':env['MallocSecureAllocator']='0'
 cmd=[b['binary'],'chain' if kind=='future' else 'vm_distinct','1000' if kind=='future' else '1024','hashmap' if kind=='future' else 'string'];r=subprocess.run(cmd,env=env,text=True,capture_output=True);assert r.returncode==0,(cmd,r.stderr);name=f'{kind}-{side}-{control}-config.stderr';(dest/name).write_text(r.stderr);configrows.append(dict(kind=kind,side=side,control=control,command=cmd,exit_code=r.returncode,binary_sha256=b['binary_sha256'],child_allocator_environment={k:v for k,v in env.items() if k.startswith('Malloc')},stderr=name,stderr_sha256=hashlib.sha256(r.stderr.encode()).hexdigest(),rows=[json.loads(l) for l in r.stdout.splitlines()]))
(dest/'native-config.json').write_text(json.dumps(configrows,indent=2)+'\n')
