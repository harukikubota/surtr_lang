from pathlib import Path
import subprocess,concurrent.futures,json
base=Path('/tmp/surtr-runtime-six')
# Two independent roots may compile concurrently; no measurements run in this stage.
scripts=[]
source=(base/'rss_build.py').read_text()
for kind,pair,probe in [('builder',['baseline','shared'],None),('future',['future-before','future-after'],'list_value_cost'),('map',['map-before','map-after'],'hashmap_cost')]:
 for variant in pair:
  s=source[:source.index('for kind,')]+source[source.index('for kind,'):]
  start=s.index('for kind,');end=s.index(':\n for variant',start)
  s=s[:start]+f'for kind,pair,probe in {[(kind,[variant],probe)]!r}'+s[end:]
  p=base/('rss-build-'+variant+'.py');p.write_text(s);scripts.append(p)
isos=(base/'rss_isolate_build.py').read_text()
for variant in ['baseline-isolation','shared-isolation']:
 start=isos.index('for kind,');end=isos.index(':\n for variant',start);s=isos[:start]+f'for kind,pair,probe in {[("builder",[variant],None)]!r}'+isos[end:];p=base/('rss-build-'+variant+'.py');p.write_text(s);scripts.append(p)
def build(p):
 with (base/(p.stem+'.log')).open('w') as log:r=subprocess.run(['python3',str(p)],stdout=log,stderr=subprocess.STDOUT)
 print(p.name,r.returncode,flush=True);return r.returncode
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as ex:results=list(ex.map(build,scripts))
assert not any(results),results
# CLI builds use their own target root too.
cmd=['python3','/Users/haruca/work/rust/surtr/tests/profile/runtime_adoption_build.py','--root','/Users/haruca/work/rust/surtr','--revision','761c6771','--target-dir',str(base/'rss-cli-after-target'),'--output-dir',str(base/'rss-cli-after')]
subprocess.run(cmd,check=True)
