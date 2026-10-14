#!/usr/bin/env python3
"""Reconstruct historical products without restoring or modifying managed worktrees."""
import argparse,hashlib,json,shutil,subprocess
from pathlib import Path

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--repo',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args();root=a.repo.resolve();a.output.mkdir(parents=True,exist_ok=False)
 revisions={'baseline':'72154707','shared':'3d030ad54','future-before':'adefb5fc','future-after':'282f78151','map-before':'547c6a68','map-after':'981464e44'}
 for name,revision in revisions.items():
  dest=a.output/name;dest.mkdir();archive=subprocess.check_output(['git','archive',revision],cwd=root);subprocess.run(['tar','-x','-C',str(dest)],input=archive,check=True)
 evidence=root/'doc/optimize/runtime/checkpoint/list_runtime_oi039_vm_memory_evidence_2026_10_12'
 for f in (evidence/'baseline_source').rglob('*'):
  if f.is_file():saved=a.output/'baseline'/f.relative_to(evidence/'baseline_source');saved.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(f,saved)
 for file in ['crates/eldr/src/vm/process_continuation.rs','crates/eldr/Cargo.toml']:shutil.copy2(a.output/'shared'/file,a.output/'baseline'/file)
 for name in ['baseline','shared']:
  old=json.loads((evidence/f'{name}-rss-final.json').read_text());hashes=old['sources_sha256'];diff=[x for x,v in hashes.items() if sha(a.output/name/x)!=v];assert not diff,diff
  (a.output/f'{name}-reconstruction-audit.json').write_text(json.dumps(dict(archive=revisions[name],source_count=len(hashes),differences=diff),indent=2)+'\n')
 for kind,probe in [('future','list_value_cost'),('map','hashmap_cost')]:
  src=a.output/(kind+'-after')/'tests/profile'/probe;dest=a.output/(kind+'-before')/'tests/profile'/probe
  if dest.exists():shutil.rmtree(dest)
  shutil.copytree(src,dest)
 (a.output/'revisions.json').write_text(json.dumps(revisions,indent=2)+'\n')
if __name__=='__main__':main()
