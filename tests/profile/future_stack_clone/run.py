#!/usr/bin/env python3
"""Balanced fresh-process clone/drop probes; allocation, timing and verification are separate."""
import argparse, collections, hashlib, json, statistics, subprocess
from pathlib import Path

CASES=['never','cleared','popped','truncated','resolved','pending']
METRICS=['alloc_calls','realloc_calls','requested_bytes','freed_bytes','live_bytes','peak_bytes']
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def check(row, metadata):
    if row['mode'] != 'allocation': return
    n,count,case=row['n'],row['count'],row['case']
    length=0 if case=='cleared' else n+(1 if case=='resolved' else 2 if case=='pending' else 0)
    bytes=length*row['value_bytes']
    calls=int(length>0)
    if case=='pending' and metadata!='shared':
        bytes+=row['metadata_bytes']+2*row['position_bytes'];calls+=2
    elif metadata=='legacy' and case not in ['never','pending']: bytes+=row['metadata_bytes'];calls+=1
    if row['phase']=='clone':
        expected={'alloc_calls':calls*count,'realloc_calls':0,'requested_bytes':bytes*count,'freed_bytes':0,'live_bytes':bytes*count,'peak_bytes':bytes*count}
    else:
        expected={'alloc_calls':0,'realloc_calls':0,'requested_bytes':0,'freed_bytes':bytes*count,'live_bytes':0,'peak_bytes':bytes*count}
    if any(row[k]!=v for k,v in expected.items()):raise RuntimeError((side,row,expected))
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--builds',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--samples',type=int,default=6);p.add_argument('--clones',type=int,default=1000)
    p.add_argument('--before-metadata',choices=['legacy','owned','shared'],default='legacy')
    p.add_argument('--after-metadata',choices=['legacy','owned','shared'],default='owned')
    a=p.parse_args()
    metadata={'before':a.before_metadata,'after':a.after_metadata}
    if a.samples<2 or a.samples%2 or a.clones<1:p.error('even samples >=2 and positive clones required')
    builds=json.loads(a.builds.read_text());binaries={s:Path(r['binary']) for s,r in builds['records'].items()}
    for side,bin in binaries.items():assert sha(bin)==builds['records'][side]['binary_sha256']
    groups=collections.defaultdict(list);verifications=[]
    a.output.parent.mkdir(parents=True,exist_ok=True)
    with a.output.with_suffix('.jsonl').open('x') as raw:
        for n in [0,16,256]:
            for case in CASES:
                for mode in ['allocation','time']:
                    for sample in range(a.samples):
                        order=['before','after'] if sample%2==0 else ['after','before']
                        for position,side in enumerate(order):
                            result=subprocess.run([str(binaries[side]),case,str(n),str(a.clones),mode],check=True,text=True,capture_output=True)
                            rows=[json.loads(l) for l in result.stdout.splitlines()]
                            assert len(rows)==2
                            for row in rows:
                                assert (row['n'],row['count'],row['case'],row['mode'])==(n,a.clones,case,mode)
                                check(row,metadata[side])
                                if mode=='allocation': row.pop('elapsed_ns')
                                else:
                                    for key in METRICS: row.pop(key)
                                row.update(variant=side,sample=sample,order_position=position)
                                raw.write(json.dumps(row,sort_keys=True)+'\n');raw.flush()
                                groups[side,n,case,mode,row['phase']].append(row)
                for side in binaries:
                    result=subprocess.run([str(binaries[side]),case,str(n),str(a.clones),'verify'],check=True,text=True,capture_output=True)
                    row=json.loads(result.stdout);assert row['verified'];row['variant']=side;verifications.append(row)
                print(n,case,'complete',flush=True)
    summaries=[]
    for (side,n,case,mode,phase),rows in groups.items():
        s={'variant':side,'n':n,'case':case,'mode':mode,'phase':phase,'samples':len(rows)}
        for key in (METRICS if mode=='allocation' else ['elapsed_ns']):
            vals=[r[key] for r in rows];s[key]={'min':min(vals),'median':statistics.median(vals),'max':max(vals)}
        summaries.append(s)
    for side,bin in binaries.items():assert sha(bin)==builds['records'][side]['binary_sha256']
    a.output.with_suffix('.summary.json').write_text(json.dumps({'builds':builds,'metadata_modes':metadata,'runner_sha256':sha(Path(__file__)),'clones':a.clones,'measurements':summaries},indent=2)+'\n')
    a.output.with_name(a.output.name+'_verify.json').write_text(json.dumps(verifications,indent=2)+'\n')
if __name__=='__main__':main()
