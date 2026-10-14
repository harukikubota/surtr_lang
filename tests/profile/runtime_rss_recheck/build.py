#!/usr/bin/env python3
"""Save native probes using a target directory dedicated to one source root."""
import argparse,hashlib,importlib.util,json,platform,shutil,subprocess
from pathlib import Path

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--root',required=True,type=Path);p.add_argument('--variant',required=True);p.add_argument('--kind',required=True,choices=['builder','future','map']);p.add_argument('--target-dir',required=True,type=Path);p.add_argument('--output-dir',required=True,type=Path);p.add_argument('--runner',type=Path);a=p.parse_args();root=a.root.resolve();a.output_dir.mkdir(parents=True,exist_ok=True)
 ownership=a.target_dir/'source-owner.json';a.target_dir.mkdir(parents=True,exist_ok=True)
 if ownership.exists():assert json.loads(ownership.read_text())['root']==str(root),'target directory belongs to another root'
 else:ownership.write_text(json.dumps(dict(root=str(root)))+'\n')
 probe={'future':'list_value_cost','map':'hashmap_cost'}.get(a.kind)
 for counted in [False,True]:
  dest=a.output_dir/(a.variant+('-count' if counted else '-time'));dest.mkdir(exist_ok=False)
  paths=sorted((root/'crates').rglob('*.rs'))+sorted((root/'crates').rglob('Cargo.toml'))+sorted((root/'lib').rglob('*.srt'))+[root/'Cargo.toml',root/'Cargo.lock']
  if probe:paths+=sorted((root/'tests/profile'/probe).rglob('*.rs'))+[root/'tests/profile'/probe/'Cargo.toml',root/'tests/profile'/probe/'Cargo.lock']
  hashes={str(f.relative_to(root)):sha(f) for f in paths}
  if a.kind=='builder':command=['cargo','test','--release','--locked','-p','eldr','--lib','--features','vm-memory-count' if counted else 'vm-memory-profile','--no-run','--message-format=json']
  else:
   command=['cargo','build','--release','--locked','--manifest-path',str(root/'tests/profile'/probe/'Cargo.toml'),'--bin','list-value-cost-probe' if a.kind=='future' else 'hashmap-cost-probe','--message-format=json']
   if not counted:command+=['--no-default-features']
  command+=['--target-dir',str(a.target_dir.resolve())];r=subprocess.run(command,cwd=root,text=True,capture_output=True);(dest/'cargo.jsonl').write_text(r.stdout);(dest/'compiler.log').write_text(r.stderr);r.check_returncode();assert hashes=={str(f.relative_to(root)):sha(f) for f in paths}
  artifacts=[json.loads(l) for l in r.stdout.splitlines() if l.startswith('{')];executable=[x for x in artifacts if x.get('reason')=='compiler-artifact' and x.get('executable')][-1];binary=(dest/'probe').resolve();shutil.copy2(executable['executable'],binary)
  meta=dict(source_root=str(root),variant=a.variant,kind=a.kind,counted=counted,source_sha256=hashes,binary=str(binary),binary_sha256=sha(binary),command=command,artifact=executable,rustc=subprocess.check_output(['rustc','-V'],text=True).strip(),platform=platform.platform(),original_binary_reused=False)
  if a.kind=='builder':
   assert a.runner;spec=importlib.util.spec_from_file_location('historical',a.runner);old=importlib.util.module_from_spec(spec);spec.loader.exec_module(old);meta.update(sources_sha256=old.sources(root),runner_sha256=sha(a.runner),test_name=old.TEST)
  (dest/'build.json').write_text(json.dumps(meta,indent=2)+'\n')
if __name__=='__main__':main()
