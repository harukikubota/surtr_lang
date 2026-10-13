#!/usr/bin/env python3
"""Compare the public cached batch entry with ordinary Regex/List composition."""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import tempfile
import time

TEMPLATE = """outcome = do {
  re <- Regex::compile(PATTERN)
  matches = Regex::find_all(re, INPUT)
  texts = List::map(matches, {|item| RegexMatch::text(item)})
  Ok(List::len(texts))
}
print(inspect(outcome))
"""
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True, type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--samples", type=int, default=5)
    args = parser.parse_args()
    if args.samples < 2 or platform.system() != "Darwin":
        parser.error("at least two samples and Darwin RSS support required")
    root = Path(__file__).resolve().parents[3]
    binaries = {"before": args.before.resolve(strict=True), "after": args.after.resolve(strict=True)}
    hashes = {side: sha(binary) for side, binary in binaries.items()}
    folder = Path(tempfile.mkdtemp(prefix="surtr-regex-batch-20261013-"))
    env = dict(os.environ, SURTR_RUN_CACHE="1", SURTR_RUN_CACHE_DIR=str(folder / "cache"))
    configs = [(n, case) for n in [1024, 4096] for case in ["dense", "single", "none"]]
    sources = {}
    for n, case in configs:
        pattern = {"dense": "a", "single": "a+", "none": "z"}[case]
        file = folder / f"{case}_{n}.srt"
        file.write_text(TEMPLATE.replace("PATTERN", json.dumps(pattern)).replace("INPUT", json.dumps("a" * n)))
        sources[n, case] = file
    def run(side, n, case, measured):
        file = sources[n, case]
        command = [str(binaries[side]), "run", str(file), "--phase-times-json"]
        if measured:
            command = ["/usr/bin/time", "-l", *command]
        started = time.perf_counter_ns()
        done = subprocess.run(command, cwd=root, env=env, text=True, capture_output=True, check=True)
        elapsed = time.perf_counter_ns() - started
        expected = n if case == "dense" else int(case == "single")
        if done.stdout != f"Ok({expected})\n":
            raise RuntimeError(f"unexpected batch result: {done.stdout}")
        timings = [json.loads(line) for line in done.stderr.splitlines() if line.startswith("{")]
        if len(timings) != 1:
            raise RuntimeError("missing phase timing object")
        timing = timings[0]
        if measured and timing["cache"]["artifact"] != "hit":
            raise RuntimeError("cold artifact sample must not enter measured series")
        row = {"series": side, "n": n, "case": case, "matches": expected, "process_wall_ns": elapsed, "execute_us": timing["phases"]["execute"]["duration_us"], "total_us": timing["total_us"], "compile_total_us": timing["compile_total_us"], "stdout": done.stdout, "phase_timing": timing}
        if measured:
            match = re.search(r"^\s*(\d+)\s+maximum resident set size\s*$", done.stderr, re.MULTILINE)
            if not match:
                raise RuntimeError("missing process RSS")
            row["process_max_rss_bytes"] = int(match.group(1))
        return row
    # Compile and warm both executable/source combinations outside measurements.
    warmups = [run(side, n, case, False) for n, case in configs for side in binaries]
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    groups = collections.defaultdict(list)
    with output.with_suffix(".jsonl").open("w") as raw:
        for n, case in configs:
            for sample in range(args.samples):
                for side in (["before", "after"] if sample % 2 == 0 else ["after", "before"]):
                    row = run(side, n, case, True)
                    row["sample"] = sample
                    raw.write(json.dumps(row, sort_keys=True) + "\n")
                    raw.flush()
                    groups[side, n, case].append(row)
    if any(sha(binary) != hashes[side] for side, binary in binaries.items()):
        raise RuntimeError("binary changed during series")
    summaries = []
    for (side, n, case), rows in groups.items():
        summary = {"series": side, "n": n, "case": case, "samples": len(rows)}
        for key in ["process_wall_ns", "execute_us", "total_us", "compile_total_us", "process_max_rss_bytes"]:
            values = [row[key] for row in rows]
            summary[key] = {"min": min(values), "median": statistics.median(values), "max": max(values)}
        summaries.append(summary)
    metadata = {"binary": {side: {"path": str(binary), "sha256": hashes[side]} for side, binary in binaries.items()}, "samples": args.samples, "folder": str(folder), "script_sha256": {str(file): sha(file) for file in sources.values()}, "runner_sha256": sha(Path(__file__)), "platform": platform.platform(), "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(), "environment": {key: value for key, value in env.items() if key.startswith(("SURTR_RUN_CACHE", "Malloc", "DYLD_"))}, "warmups": warmups, "observer_enabled": False, "trace_enabled": False, "allocation_metrics": "not measured", "profile": "release"}
    output.with_suffix(".summary.json").write_text(json.dumps({"metadata": metadata, "measurements": summaries}, indent=2) + "\n")
    print(f"Saved {sum(map(len, groups.values()))} cached batch samples")

if __name__ == "__main__":
    main()
