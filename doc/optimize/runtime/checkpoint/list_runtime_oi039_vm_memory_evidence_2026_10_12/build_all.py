from pathlib import Path
import importlib.util,json,subprocess,os,shutil
repo=Path('/Users/haruca/.codex/worktrees/9ea8/surtr')
tmp=Path(__file__).parent
spec=importlib.util.spec_from_file_location('runner',repo/'tests/profile/checkpoint_builder_vm_memory/run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
for variant in ['shared','baseline']:
 for mode in ['count','rss']:
  name=variant+'-'+mode;root=tmp/variant;feature='vm-memory-count' if mode=='count' else 'vm-memory-profile'
  snapshot=r.sources(root);(tmp/'builds'/(name+'-before.json')).write_text(json.dumps(snapshot,indent=2)+'\n')
  cmd=['cargo','test','--release','--locked','-p','eldr','--lib','--features',feature,'--no-run','--message-format=json']
  env=dict(os.environ,CARGO_TARGET_DIR=str(repo/'target'))
  with (tmp/'builds'/(name+'-build.jsonl')).open('w') as out,(tmp/'builds'/(name+'-build.stderr')).open('w') as errors:code=subprocess.run(cmd,cwd=root,env=env,stdout=out,stderr=errors).returncode
  if code:
   print(name,'FAILED',code,flush=True);print((tmp/'builds'/(name+'-build.stderr')).read_text(),flush=True);raise SystemExit(code)
  after=r.sources(root);assert after==snapshot;(tmp/'builds'/(name+'-after.json')).write_text(json.dumps(after,indent=2)+'\n')
  records=[json.loads(l) for l in (tmp/'builds'/(name+'-build.jsonl')).read_text().splitlines()]
  paths=[d['executable'] for d in records if d.get('reason')=='compiler-artifact' and d.get('executable') and d.get('target',{}).get('name')=='eldr'];assert len(paths)==1
  shutil.copy2(paths[0],tmp/name);assert r.sha(paths[0])==r.sha(tmp/name)
  args=['python3',str(repo/'tests/profile/checkpoint_builder_vm_memory/run.py'),'record','--binary',str(tmp/name),'--source-root',str(root),'--variant',variant,'--output',str(tmp/(name+'.json'))]
  if mode=='count':args.append('--counted')
  subprocess.run(args,check=True);print(name,'saved',r.sha(tmp/name),flush=True)
