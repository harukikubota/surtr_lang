#!/usr/bin/env python3
"""Build two standalone probes from exact private sources and the Cargo sindr artifact."""
import argparse, hashlib, json, os, subprocess
from pathlib import Path

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--before-source',type=Path,required=True)
    p.add_argument('--after-source',type=Path,required=True)
    p.add_argument('--sindr-build',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True)
    a=p.parse_args();a.output_dir.mkdir(parents=True,exist_ok=True)
    probe=Path(__file__).with_name('probe.rs').resolve();root=probe.parents[3]
    artifact=next(j for l in a.sindr_build.read_text().splitlines() if l.startswith('{')
                  for j in [json.loads(l)] if j.get('reason')=='compiler-artifact' and j['target']['name']=='sindr')
    assert Path(artifact['manifest_path']).resolve()==root/'crates/sindr/Cargo.toml'
    library=Path(next(f for f in artifact['filenames'] if f.endswith('.rlib')))
    dependencies=library.parent/'deps' if (library.parent/'deps').is_dir() else library.parent
    sindr_paths=sorted((root/'crates/sindr/src').rglob('*.rs'))+[root/'Cargo.toml',root/'Cargo.lock',root/'crates/sindr/Cargo.toml']
    hashes={str(f.relative_to(root)):sha(f) for f in sindr_paths}
    records={}
    for side,src in [('before',a.before_source),('after',a.after_source)]:
        saved=a.output_dir/(side+'_operand_stack.rs');saved.write_bytes(src.read_bytes())
        binary=(a.output_dir/(side+'-probe')).resolve()
        cmd=['rustc','--edition=2021','-C','opt-level=3','-C','debuginfo=0','-C','codegen-units=16',str(probe),
             '--extern','sindr='+str(library),'-L','dependency='+str(dependencies),'-o',str(binary)]
        env=dict(os.environ,SURTR_OPERAND_STACK_SOURCE=str(saved.resolve()))
        completed=subprocess.run(cmd,env=env,capture_output=True,text=True)
        (a.output_dir/(side+'_compiler.log')).write_text(completed.stderr);completed.check_returncode()
        records[side]={'command':cmd,'source_path':str(src.resolve()),'source_sha256':sha(src),
                       'saved_source':str(saved.resolve()),'binary':str(binary),'binary_sha256':sha(binary)}
    assert hashes=={str(f.relative_to(root)):sha(f) for f in sindr_paths}
    result={'scope':'isolated OperandStack, not full Eldr VM','records':records,
            'sindr_artifact':artifact,'sindr_library_sha256':sha(library),'sindr_source_sha256':hashes,
            'probe_source':str(probe),'probe_sha256':sha(probe),'builder_sha256':sha(Path(__file__)),
            'rustc_version':subprocess.check_output(['rustc','-Vv'],text=True)}
    (a.output_dir/'builds.json').write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
