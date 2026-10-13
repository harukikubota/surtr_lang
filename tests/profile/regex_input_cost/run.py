#!/usr/bin/env python3
"""Interleave unchanged/changed VM probes; separate allocation and time series."""
import argparse, collections, hashlib, json, os, platform, re, statistics, subprocess
from pathlib import Path

SOURCES = ["crates/sindr/src/runtime.rs", "crates/sindr/src/ir.rs", "crates/eldr/src/builtin.rs", "crates/eldr/src/vm.rs", "Cargo.lock"]
PHASES = ["run", "collect_handles", "clone_handles", "drop_vm", "drop_handles", "drop_clones"]
CONFIGS = [(n, case) for n in [1024, 2048, 4096] for case in ["dense", "single", "none"]] + [(128, "utf8_empty")]
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for label in ["before-count", "before-time", "after-count", "after-time"]:
        parser.add_argument("--" + label, required=True, type=Path)
    parser.add_argument("--baseline-root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--samples", type=int, default=5)
    args = parser.parse_args()
    if args.samples < 2: parser.error("at least two samples required")
    if platform.system() != "Darwin": parser.error("RSS collection requires Darwin /usr/bin/time -l")
    if "PROBE_VERIFY" in os.environ: parser.error("PROBE_VERIFY must be unset in performance series")
    root = Path(__file__).resolve().parents[3]
    binaries = {label: getattr(args, label.replace("-", "_")).resolve(strict=True) for label in ["before-count", "before-time", "after-count", "after-time"]}
    def sources(r): return {s: sha(r / s) for s in SOURCES}
    info = {label: {"binary": str(binary), "binary_sha256": sha(binary), "source_root": str(args.baseline_root if label.startswith("before") else root), "source_sha256": sources(args.baseline_root if label.startswith("before") else root)} for label,binary in binaries.items()}
    metadata = {"series":info,"samples":args.samples,"configs":CONFIGS,"phases":PHASES,"platform":platform.platform(),"machine":platform.machine(),"rustc":subprocess.check_output(["rustc","-V"],text=True).strip(),"source_commit":subprocess.check_output(["git","rev-parse","HEAD"],cwd=root,text=True).strip(),"git_status":subprocess.check_output(["git","status","--porcelain"],cwd=root,text=True),"probe_source_sha256":{str(f.relative_to(root)):sha(f) for f in [Path(__file__),Path(__file__).with_name("Cargo.lock"),*Path(__file__).with_name("src").glob("*.rs")]},"allocator_environment":{k:v for k,v in os.environ.items() if k.startswith(("Malloc","DYLD_"))},"profile":"release","rss_unit":"Darwin process maximum resident bytes"}
    output=args.output.resolve(); output.parent.mkdir(parents=True,exist_ok=True)
    groups=collections.defaultdict(list)
    with output.with_suffix(".jsonl").open("w") as raw:
        for config in CONFIGS:
            for sample in range(args.samples):
                for counting in ["count","time"]:
                    for side in (["before","after"] if sample%2==0 else ["after","before"]):
                        label=side+"-"+counting
                        completed=subprocess.run(["/usr/bin/time","-l",str(binaries[label]),*map(str,config)],cwd=root,text=True,capture_output=True,check=True)
                        match=re.search(r"^\s*(\d+)\s+maximum resident set size\s*$",completed.stderr,re.MULTILINE)
                        if not match: raise RuntimeError("missing process RSS")
                        rows=[json.loads(line) for line in completed.stdout.splitlines()]
                        if [r["phase"] for r in rows] != PHASES: raise RuntimeError("wrong phase inventory")
                        for row in rows:
                            if row["allocator_counting"] != (counting=="count") or row["correctness_verification"]: raise RuntimeError("wrong probe mode")
                            if row["n"] != config[0] or row["case"] != config[1]: raise RuntimeError("wrong workload")
                            row.update(series=label,sample=sample,process_max_rss_bytes=int(match.group(1)))
                            raw.write(json.dumps(row,sort_keys=True)+"\n"); raw.flush()
                            groups[(label,*config,row["phase"])].append(row)
    for label,binary in binaries.items():
        if sha(binary) != info[label]["binary_sha256"]: raise RuntimeError("binary changed during measurement")
        if sources(Path(info[label]["source_root"])) != info[label]["source_sha256"]: raise RuntimeError("product source changed during measurement")
    summaries=[]
    for (label,n,case,phase),rows in groups.items():
        summary={"series":label,"n":n,"case":case,"phase":phase,"samples":len(rows)}
        if len(rows)!=args.samples: raise RuntimeError("incomplete group")
        for key,value in rows[0].items():
            if isinstance(value,(int,float)) and not isinstance(value,bool) and key not in ["n","sample"]:
                values=[r[key] for r in rows]; summary[key]={"min":min(values),"median":statistics.median(values),"max":max(values)}
        summaries.append(summary)
    output.with_suffix(".summary.json").write_text(json.dumps({"metadata":metadata,"measurements":summaries},indent=2)+"\n")
    print(f"Saved {sum(map(len,groups.values()))} rows, {len(summaries)} groups")
if __name__ == "__main__": main()
