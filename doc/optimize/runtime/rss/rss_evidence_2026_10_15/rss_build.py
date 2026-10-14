from pathlib import Path
import subprocess,json,hashlib,shutil,platform,importlib.util
base=Path('/tmp/surtr-runtime-six'); hist=base/'rss-historical'; out=base/'rss-rebuilt'; out.mkdir(exist_ok=True)
runner=hist/'shared/tests/profile/checkpoint_builder_vm_memory/run.py'
spec=importlib.util.spec_from_file_location('oldrun',runner); old=importlib.util.module_from_spec(spec); spec.loader.exec_module(old)
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for kind,pair,probe in [('builder',['baseline','shared'],None),('future',['future-before','future-after'],'list_value_cost'),('map',['map-before','map-after'],'hashmap_cost')]:
 for variant in pair:
  root=hist/variant
  for counted in [False,True]:
   dest=out/(variant+('-count' if counted else '-time')); dest.mkdir(exist_ok=True)
   if (dest/'build.json').exists(): continue
   paths=sorted((root/'crates').rglob('*.rs'))+sorted((root/'crates').rglob('Cargo.toml'))+sorted((root/'lib').rglob('*.srt'))+[root/'Cargo.toml',root/'Cargo.lock']
   if probe: paths+=sorted((root/'tests/profile'/probe).rglob('*.rs'))+[root/'tests/profile'/probe/'Cargo.toml',root/'tests/profile'/probe/'Cargo.lock']
   hashes={str(p.relative_to(root)):sha(p) for p in paths}
   if kind=='builder': command=['cargo','test','--release','--locked','-p','eldr','--lib','--features','vm-memory-count' if counted else 'vm-memory-profile','--no-run','--message-format=json']
   else:
    command=['cargo','build','--release','--locked','--manifest-path',str(root/'tests/profile'/probe/'Cargo.toml'),'--bin','list-value-cost-probe' if kind=='future' else 'hashmap-cost-probe','--message-format=json']
    if not counted:command+=['--no-default-features']
   command+=['--target-dir',str(base/('rss-target-'+variant))]
   print('building',dest.name,flush=True)
   r=subprocess.run(command,cwd=root,text=True,capture_output=True); (dest/'cargo.jsonl').write_text(r.stdout); (dest/'compiler.log').write_text(r.stderr); r.check_returncode()
   assert hashes=={str(p.relative_to(root)):sha(p) for p in paths}
   rows=[json.loads(l) for l in r.stdout.splitlines() if l.startswith('{')]; arts=[x for x in rows if x.get('reason')=='compiler-artifact' and x.get('executable')]; art=arts[-1]
   binary=dest/'probe';shutil.copy2(art['executable'],binary)
   meta=dict(source_root=str(root),binary=str(binary),binary_sha256=sha(binary),source_sha256=hashes,command=command,artifact=art,counted=counted,variant=variant,kind=kind,rustc=subprocess.check_output(['rustc','-V'],text=True).strip(),platform=platform.platform(),original_binary_reused=False)
   if kind=='builder': meta.update(sources_sha256=old.sources(root),runner_sha256=sha(runner),test_name=old.TEST)
   (dest/'build.json').write_text(json.dumps(meta,indent=2)+'\n');print('built',dest.name,flush=True)
