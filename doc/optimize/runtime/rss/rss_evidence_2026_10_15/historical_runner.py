#!/usr/bin/env python3
"""Observe allocation balance and resident boundaries in the actual VM."""
import argparse
import collections
import datetime
import hashlib
import itertools
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import tempfile

REPO = Path(__file__).resolve().parents[3]
TEST = "vm::list_flat_map_tests::memory_profile::vm_memory_profile"
PHASES = ["prepare_shared", "save", "save_unchanged", "drop_unchanged", "first_resume",
          "second_resume", "resave", "resume_after_resave", "rollback", "complete_shared",
          "drop_changed", "drop_shared_vm", "prepare_unique", "unique_complete", "drop_unique_vm"]
COUNTERS = ["allocations", "reallocations", "requested_bytes", "freed_bytes", "live_start_bytes",
            "live_end_bytes", "live_delta_bytes", "peak_extra_bytes"]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sources(root):
    paths = sorted(p for c in ["eldr", "sindr", "diagnostics"] for p in (root / "crates" / c / "src").rglob("*.rs"))
    paths += sorted((root / "lib").rglob("*.srt"))
    paths += [root / p for p in ["Cargo.toml", "Cargo.lock", "crates/eldr/Cargo.toml", "crates/sindr/Cargo.toml", "crates/diagnostics/Cargo.toml"]]
    return {str(p.relative_to(root)): sha(p) for p in paths}


def read_build(path):
    d = json.loads(path.read_text())
    if d["test_name"] != TEST or d["runner_sha256"] != sha(__file__):
        raise RuntimeError("test/runner differs from recorded build")
    if sha(d["binary"]) != d["binary_sha256"] or sources(Path(d["source_root"])) != d["sources_sha256"]:
        raise RuntimeError("saved binary or source changed")
    if d["rustc"] != subprocess.check_output(["rustc", "-V"], text=True).strip() or d["platform"] != platform.platform():
        raise RuntimeError("build environment changed")
    return d


def rss(pid):
    s = subprocess.check_output(["/bin/ps", "-o", "rss=", "-p", str(pid)], text=True).strip()
    if not s.isdigit() or int(s) <= 0:
        raise RuntimeError("invalid process resident observation")
    return int(s) * 1024


def validate(rows, build, tasks, length, payload, repeats, verify):
    common = {"tasks": tasks, "builder_len": length, "payload": payload, "repeats": repeats,
              "copy_mode": build["variant"], "allocator_counting": build["counted"]}
    for r in rows:
        if any(r.get(k) != v for k, v in common.items()):
            raise RuntimeError(f"invalid workload row: {r}")
    if verify:
        if len(rows) != 1 or rows[0].get("profile") != "checkpoint_builder_vm_memory_verify" or rows[0].get("verified") is not True or rows[0].get("workload_live_restored") is not build["counted"]:
            raise RuntimeError("invalid content verification")
        return
    expected = {(i, p) for i in range(repeats) for p in PHASES}
    seen = set()
    grouped = collections.defaultdict(dict)
    for r in rows:
        key = r.get("repeat"), r.get("phase")
        if key in seen or key not in expected or r.get("profile") != "checkpoint_builder_vm_memory" or r.get("output_len") != length + 2048:
            raise RuntimeError("unexpected phase/repeat/profile")
        seen.add(key)
        grouped[key[0]][key[1]] = r
        for field in ["elapsed_ns", "copies", "pushes", "callback_calls", "input_visits", "finishes"]:
            if type(r.get(field)) is not int or r[field] < 0:
                raise RuntimeError(f"invalid metric {field}")
        prepare = r["phase"].startswith("prepare_")
        clones = tasks if r["phase"] in ["first_resume", "resume_after_resave"] else 0
        pushes = tasks * (length if prepare else 256 if r["phase"] in ["first_resume", "second_resume", "resume_after_resave"] else 1536 if r["phase"] == "complete_shared" else 2048 if r["phase"] == "unique_complete" else 0)
        if (r["copies"] != clones or r["pushes"] != pushes or r["callback_calls"] != (tasks if prepare else 0)
                or r["input_visits"] != (tasks if prepare else 0)
                or r["finishes"] != (tasks if r["phase"] in ["complete_shared", "unique_complete"] else 0)):
            raise RuntimeError(f"scheduler/callback contract mismatch: {r}")
        for field in COUNTERS:
            val = r.get(field)
            if build["counted"]:
                if type(val) is not int or (field != "live_delta_bytes" and val < 0):
                    raise RuntimeError(f"invalid allocation metric {field}")
            elif val is not None:
                raise RuntimeError("uncounted allocation values must be null")
        if build["counted"] and (r["requested_bytes"] - r["freed_bytes"] != r["live_delta_bytes"] or r["live_end_bytes"] - r["live_start_bytes"] != r["live_delta_bytes"]):
            raise RuntimeError("allocation balance mismatch")
    if seen != expected:
        raise RuntimeError("missing phases")
    if build["counted"]:
        for phases in grouped.values():
            baseline = phases["prepare_shared"]["live_start_bytes"]
            if any(phases[p]["live_end_bytes"] != baseline for p in ["drop_shared_vm", "drop_unique_vm"]):
                raise RuntimeError("all workload owners did not restore live baseline")
            for prev, cur in zip(PHASES, PHASES[1:]):
                if phases[prev]["live_end_bytes"] != phases[cur]["live_start_bytes"]:
                    raise RuntimeError("allocation ledger gap between phases")


def run_probe(build, tasks, length, payload, repeats, verify, barrier):
    env = dict(os.environ, SURTR_VM_MEMORY_PROFILE="1", SURTR_VM_MEMORY_TASKS=str(tasks),
               SURTR_VM_MEMORY_LEN=str(length), SURTR_VM_MEMORY_PAYLOAD=payload,
               SURTR_VM_MEMORY_REPEATS=str(repeats), SURTR_VM_MEMORY_COPY_MODE=build["variant"],
               SURTR_VM_MEMORY_BARRIER="1" if barrier else "0", PROBE_VERIFY="1" if verify else "0")
    cmd = [build["binary"], "--exact", TEST, "--nocapture", "--test-threads=1"]
    if not build["counted"] and not verify:
        cmd = ["/usr/bin/time", "-l", *cmd]
    rows, endpoints, startup, child_pid = [], {}, None, None
    # stderr goes to a file while stdout is streamed: no pipe-capacity deadlock.
    with tempfile.TemporaryFile(mode="w+t") as errors:
        with subprocess.Popen(cmd, cwd=REPO, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=errors, text=True, bufsize=1) as proc:
            for line in proc.stdout:
                line = line.strip()
                if line.startswith("START "):
                    if not barrier or child_pid is not None:
                        raise RuntimeError("unexpected startup barrier")
                    child_pid = int(line.split()[1]); startup = rss(child_pid)
                    proc.stdin.write("go\n"); proc.stdin.flush()
                elif line.startswith("READY "):
                    _, repeat, phase = line.split(); key = int(repeat), phase
                    if child_pid is None or key in endpoints:
                        raise RuntimeError("unexpected endpoint barrier")
                    endpoints[key] = rss(child_pid)
                    proc.stdin.write("go\n"); proc.stdin.flush()
                elif line.startswith("{"):
                    rows.append(json.loads(line))
                elif line and not (line == "ok" or line.startswith("running ") or line.startswith("test ") or line.startswith("test result:")):
                    raise RuntimeError(f"unexpected stdout: {line}")
            code = proc.wait()
        errors.seek(0); stderr = errors.read()
    if code:
        raise RuntimeError(f"probe exited {code}: {stderr}")
    validate(rows, build, tasks, length, payload, repeats, verify)
    if not verify:
        peaks = re.findall(r"^\s*(\d+)\s+maximum resident set size\s*$", stderr, re.MULTILINE)
        if not build["counted"] and (len(peaks) != 1 or int(peaks[0]) <= 0):
            raise RuntimeError("missing whole-process peak")
        if barrier and (len(endpoints) != len(rows) or startup is None):
            raise RuntimeError("missing resident endpoints")
        for r in rows:
            r.update(resident_end_bytes=endpoints.get((r["repeat"], r["phase"])), startup_resident_bytes=startup,
                     process_peak_rss_bytes=int(peaks[0]) if peaks else None)
    return rows


def main():
    p = argparse.ArgumentParser(description=__doc__); sub = p.add_subparsers(dest="action", required=True)
    rec = sub.add_parser("record"); rec.add_argument("--binary", type=Path, required=True); rec.add_argument("--source-root", type=Path, required=True)
    rec.add_argument("--variant", choices=["baseline", "shared"], required=True); rec.add_argument("--counted", action="store_true"); rec.add_argument("--output", type=Path, required=True)
    run = sub.add_parser("run"); run.add_argument("--build", type=Path, action="append", required=True); run.add_argument("--output", type=Path, required=True)
    run.add_argument("--samples", type=int, default=2); run.add_argument("--tasks", type=int, default=32); run.add_argument("--length", type=int, default=16384)
    run.add_argument("--repeats", type=int, nargs="+", default=[1, 8]); run.add_argument("--payloads", nargs="+", choices=["int", "string"], default=["int", "string"])
    run.add_argument("--verify", action="store_true"); run.add_argument("--no-barrier", action="store_true")
    a = p.parse_args()
    if a.action == "record":
        root = a.source_root.resolve(); binary = a.binary.resolve(strict=True)
        d = {"source_root": str(root), "binary": str(binary), "binary_sha256": sha(binary), "sources_sha256": sources(root),
             "variant": a.variant, "counted": a.counted, "test_name": TEST, "runner_sha256": sha(__file__),
             "rustc": subprocess.check_output(["rustc", "-V"], text=True).strip(), "platform": platform.platform(),
             "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
             "anchor_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()}
        with a.output.open("x") as out: json.dump(d, out, indent=2); out.write("\n")
        return
    if min(a.samples, a.tasks, a.length, *a.repeats) < 1 or a.samples % 2 and not a.verify:
        p.error("positive arguments and balanced even process samples required")
    if not a.verify and platform.system() != "Darwin":
        p.error("RSS requires macOS")
    builds = [read_build(q) for q in a.build]
    rows = []; groups = collections.defaultdict(list)
    with a.output.with_suffix(".jsonl").open("x") as raw:
        for payload, repeats in itertools.product(a.payloads, a.repeats):
            for sample in range(a.samples):
                order = builds if sample % 2 == 0 else list(reversed(builds))
                for build in order:
                    barrier = not build["counted"] and not a.verify and not a.no_barrier
                    result = run_probe(build, a.tasks, a.length, payload, repeats, a.verify, barrier)
                    for r in result:
                        r.update(sample=sample, binary_sha256=build["binary_sha256"], rss_barrier=barrier)
                        raw.write(json.dumps(r, sort_keys=True) + "\n"); rows.append(r)
                    raw.flush()
                    print(f"{build['variant']} counted={build['counted']} {payload} repeats={repeats} sample={sample}", flush=True)
    if not a.verify:
        for r in rows: groups[(r["copy_mode"], r["allocator_counting"], r["payload"], r["repeats"], r["phase"])].append(r)
    summaries = []
    for key, group in sorted(groups.items()):
        item = dict(zip(["variant", "counted", "payload", "repeats", "phase"], key))
        for field in ["elapsed_ns", *COUNTERS, "resident_end_bytes", "process_peak_rss_bytes"]:
            values = [r[field] for r in group if r.get(field) is not None]
            if values:
                # Time: per-process mean, then median across fresh processes.
                if field == "elapsed_ns":
                    proc = collections.defaultdict(list)
                    for r in group: proc[r["sample"]].append(r[field])
                    values = [sum(v) / len(v) for v in proc.values()]
                item[field] = {"min": min(values), "median": statistics.median(values), "max": max(values)}
            else: item[field] = None
        summaries.append(item)
    for q in a.build: read_build(q)
    with a.output.with_suffix(".summary.json").open("x") as out:
        json.dump({"metadata": {"builds": builds, "samples": a.samples, "tasks": a.tasks, "length": a.length,
                               "repeats": a.repeats, "payloads": a.payloads, "verification": a.verify, "no_barrier": a.no_barrier},
                   "verified": rows if a.verify else [], "summaries": summaries}, out, indent=2); out.write("\n")


if __name__ == "__main__":
    main()
