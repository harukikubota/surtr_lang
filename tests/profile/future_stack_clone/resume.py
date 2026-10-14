#!/usr/bin/env python3
"""Compare construction, clone, first mutation and drop without conflating their costs."""
import argparse,collections,hashlib,json,statistics,subprocess
from pathlib import Path
METRICS=['alloc_calls','realloc_calls','requested_bytes','freed_bytes','live_bytes','peak_bytes']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--builds',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--samples',type=int,default=6);p.add_argument('--clones',type=int,default=32)
    a=p.parse_args()
    if a.samples<2 or a.samples%2 or a.clones<1:p.error('positive clones and even samples >=2 required')
    builds=json.loads(a.builds.read_text());binaries={s:Path(r['binary']) for s,r in builds['records'].items()}
    for side,bin in binaries.items():assert sha(bin)==builds['records'][side]['binary_sha256']
    groups=collections.defaultdict(list);verifications=[]
    a.output.parent.mkdir(parents=True,exist_ok=True)
    with a.output.with_suffix('.jsonl').open('x') as raw:
        for n in [0,16]:
            for pending in [1,256,4096]:
                for operation in ['none','append','resolve']:
                    for mode in ['allocation','time']:
                        for sample in range(a.samples):
                            for position,side in enumerate(['before','after'] if sample%2==0 else ['after','before']):
                                r=subprocess.run([str(binaries[side]),str(n),str(pending),str(a.clones),mode,operation],check=True,text=True,capture_output=True)
                                rows=[json.loads(l) for l in r.stdout.splitlines()];assert len(rows)==4
                                by={row['phase']:row for row in rows}
                                if mode=='allocation':
                                    assert by['drop']['live_bytes']==0
                                    assert by['clone']['requested_bytes']+by['mutate']['requested_bytes']==by['clone']['freed_bytes']+by['mutate']['freed_bytes']+by['drop']['freed_bytes']
                                    expected=(n+pending)*by['clone']['value_bytes']*a.clones
                                    if side=='before':expected+=(by['clone']['metadata_bytes']+pending*by['clone']['position_bytes'])*a.clones
                                    assert by['clone']['requested_bytes']==expected,(side,by['clone'],expected)
                                    assert by['clone']['alloc_calls']==(3 if side=='before' else 1)*a.clones
                                    if operation in ['none','resolve']:assert by['mutate']['requested_bytes']==0
                                for row in rows:
                                    assert (row['n'],row['pending'],row['count'],row['mode'],row['operation'])==(n,pending,a.clones,mode,operation)
                                    if mode=='allocation':row.pop('elapsed_ns')
                                    else:
                                        for key in METRICS:row.pop(key)
                                    row.update(variant=side,sample=sample,order_position=position)
                                    raw.write(json.dumps(row,sort_keys=True)+'\n');raw.flush()
                                    groups[side,n,pending,operation,mode,row['phase']].append(row)
                    for side in binaries:
                        r=subprocess.run([str(binaries[side]),str(n),str(pending),str(a.clones),'verify',operation],check=True,text=True,capture_output=True)
                        row=json.loads(r.stdout);assert row['verified'];row['variant']=side;verifications.append(row)
                    print(n,pending,operation,'complete',flush=True)
    summaries=[]
    for (side,n,pending,operation,mode,phase),rows in groups.items():
        s={'variant':side,'n':n,'pending':pending,'operation':operation,'mode':mode,'phase':phase,'samples':len(rows)}
        for key in (METRICS if mode=='allocation' else ['elapsed_ns']):
            vals=[r[key] for r in rows];s[key]={'min':min(vals),'median':statistics.median(vals),'max':max(vals)}
        summaries.append(s)
    for side,bin in binaries.items():assert sha(bin)==builds['records'][side]['binary_sha256']
    a.output.with_suffix('.summary.json').write_text(json.dumps({'builds':builds,'runner_sha256':sha(Path(__file__)),'clones':a.clones,'measurements':summaries},indent=2)+'\n')
    a.output.with_name(a.output.name+'_verify.json').write_text(json.dumps(verifications,indent=2)+'\n')
if __name__=='__main__':main()
