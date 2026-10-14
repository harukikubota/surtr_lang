from pathlib import Path
import subprocess,time,json
base=Path('/tmp/surtr-runtime-six');root=Path('/Users/haruca/work/rust/surtr');scripts=root/'tests/profile/runtime_rss_recheck';dest=base/'rss-results';dest.mkdir(exist_ok=False)
while not (base/'rss-cli-after/builds.json').exists():time.sleep(2)
for name,command in [
 ('native',['python3',str(scripts/'native.py'),'--builds',str(base/'rss-rebuilt'),'--output',str(dest/'native'),'--samples','6']),
 ('builder',['python3',str(scripts/'builder.py'),'--builds',str(base/'rss-rebuilt'),'--runner',str(base/'rss-historical/shared/tests/profile/checkpoint_builder_vm_memory/run.py'),'--output',str(dest/'builder'),'--samples','6']),
 ('batch',['python3',str(scripts/'batch.py'),'--before',str(base/'rss-cli-before/builds.json'),'--after',str(base/'rss-cli-after/builds.json'),'--output',str(dest/'batch'),'--samples','6']),
 ('diagnostics',['python3',str(scripts/'builder.py'),'--builds',str(base/'rss-rebuilt'),'--runner',str(base/'rss-historical/shared/tests/profile/checkpoint_builder_vm_memory/run.py'),'--output',str(dest/'diagnostics'),'--samples','2','--diagnose'])]:
 print('start',name,flush=True);start=time.monotonic()
 with (dest/(name+'.log')).open('w') as log:r=subprocess.run(command,cwd=root,stdout=log,stderr=subprocess.STDOUT)
 (dest/(name+'-result.json')).write_text(json.dumps(dict(command=command,exit_code=r.returncode,wall_seconds=time.monotonic()-start),indent=2)+'\n');print('done',name,r.returncode,flush=True);r.check_returncode()
