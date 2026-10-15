#!/usr/bin/env python3
"""Compare two immutable surtr binaries with isolated cold/disk-hit pairs."""

import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(block)
    return {"sha256": hasher.hexdigest(), "bytes": path.stat().st_size}


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def probe(args):
    try:
        result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True, timeout=15)
        return {"args": args, "exit_code": result.returncode,
                "stdout": result.stdout, "stderr": result.stderr}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"args": args, "unavailable": str(error)}


def source_hashes(cases):
    paths = {case["entry"] for case in cases}
    paths.update(path for case in cases for path in case["dependencies"])
    inputs = {name: digest(ROOT / name) for name in sorted(paths)}
    library = {str(path.relative_to(ROOT)): digest(path)
               for path in sorted((ROOT / "lib").rglob("*.srt"))}
    return {"inputs": inputs, "lib_sources": library}


def timing_report(stderr, expected_cache):
    reports = []
    for line in stderr.splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict) and "phases" in value and "cache" in value:
            reports.append(value)
    if len(reports) != 1:
        raise ValueError(f"expected exactly one timing report, found {len(reports)}")
    report = reports[0]
    cache, phases = report["cache"], report["phases"]
    if not isinstance(cache, dict) or not isinstance(phases, dict):
        raise ValueError("timing cache/phases must be objects")
    if cache.get("stdlib") != expected_cache or cache.get("artifact") != "not_used":
        raise ValueError(f"unexpected cache state: {cache}")
    if any(not isinstance(phase, dict) for phase in phases.values()):
        raise ValueError("all phases must be objects")
    for name in ("stdlib_load", "typecheck", "compile_total"):
        phase = phases.get(name, {})
        if phase.get("status") != "executed":
            raise ValueError(f"{name} must be executed")
    if phases.get("execute", {}).get("status") != "skipped":
        raise ValueError("execute must be skipped")
    for name, phase in phases.items():
        if not isinstance(phase, dict) or phase.get("status") not in ("executed", "skipped"):
            raise ValueError(f"invalid phase: {name}")
        if phase["status"] == "executed":
            value = phase.get("duration_us")
            if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
                raise ValueError(f"invalid duration_us: {name}")
    return report


def summarize(rows):
    grouped = {}
    for row in rows:
        key = (row["case"], row["cache_condition"], row["variant"])
        target = grouped.setdefault(key, {})
        for name, phase in row["timing"]["phases"].items():
            if phase["status"] == "executed":
                target.setdefault(name, []).append(phase["duration_us"])
    results = []
    for case, cache in sorted({key[:2] for key in grouped}):
        variants = {}
        for variant in ("before", "after"):
            variants[variant] = {
                phase: {"count": len(values), "median_us": statistics.median(values),
                        "min_us": min(values), "max_us": max(values)}
                for phase, values in grouped[(case, cache, variant)].items()
            }
        comparison = {}
        for phase in variants["before"].keys() & variants["after"].keys():
            before = variants["before"][phase]["median_us"]
            after = variants["after"][phase]["median_us"]
            comparison[phase] = {
                "delta_us": after - before,
                "change_percent": (after - before) / before * 100 if before else None,
            }
        results.append({"case": case, "cache_condition": cache,
                        "variants": variants, "after_vs_before": comparison})
    return {"duration_unit": "us", "change_percent": "(after-before)/before*100; negative is faster",
            "results": results}


def run_one(binary, case, variant, iteration, cache, condition, output, timeout, base_env):
    pair = cache.parent
    log_base = pair / condition
    command = [str(binary), "build", case["entry"], str(log_base.with_suffix(".eldr")),
               "--phase-times-json"]
    env = dict(base_env, SURTR_STDLIB_CACHE_DIR=str(cache))
    row = {"case": case["id"], "variant": variant, "iteration": iteration,
           "exit_code": None,
           "cache_condition": condition, "cache_dir": str(cache), "args": command,
           "cwd": str(ROOT), "env": {key: value for key, value in env.items()
                                     if key.startswith("SURTR_") or key in ("CARGO_TARGET_DIR", "RUSTFLAGS", "RUSTUP_TOOLCHAIN")
                                     or key.startswith("CARGO_PROFILE_")},
           "profile_env_unset": sorted({"SURTR_SCAR_PROFILE"} | {
               key for key in os.environ
               if key.startswith("SURTR_") and ("PROFILE" in key or "COUNTER" in key)}),
           "timeout_seconds": timeout,
           "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    start = time.monotonic()
    stdout, stderr = b"", b""
    try:
        result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, timeout=timeout)
        stdout, stderr = result.stdout, result.stderr
        row["exit_code"] = result.returncode
        if result.returncode != 0:
            raise ValueError(f"compiler exit code {result.returncode}")
        row["timing"] = timing_report(stderr.decode("utf-8", errors="replace"), condition)
    except subprocess.TimeoutExpired as error:
        stdout, stderr = error.stdout or b"", error.stderr or b""
        row.update(exit_code=None, error=f"timeout after {timeout}s")
    except (OSError, ValueError, TypeError, KeyError) as error:
        row["error"] = str(error)
    finally:
        row["wall_seconds"] = time.monotonic() - start
        for stream, data in (("stdout", stdout), ("stderr", stderr)):
            path = log_base.with_suffix(f".{stream}")
            path.write_bytes(data)
            row[stream] = str(path.relative_to(output))
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--before-manifest", type=Path, required=True,
                        help="JSON build provenance for before binary (copied into manifest)")
    parser.add_argument("--after-manifest", type=Path, required=True,
                        help="JSON build provenance for after binary (copied into manifest)")
    parser.add_argument("--output", type=Path, required=True, help="new, nonexistent output directory")
    parser.add_argument("--iterations", type=int, default=10)
    parser.add_argument("--timeout", type=float, default=120, help="per compiler process seconds")
    parser.add_argument("--case", action="append", dest="selected", help="case ID; repeat to select a subset")
    parser.add_argument("--power-condition", required=True, help="operator description of power/CPU conditions")
    options = parser.parse_args()
    if options.iterations < 1 or options.timeout <= 0 or not math.isfinite(options.timeout):
        parser.error("iterations and timeout must be positive")
    cases_path = ROOT / "tests/profile/scar_compile/cases.json"
    cases = json.loads(cases_path.read_text())
    ids = [case["id"] for case in cases]
    if len(ids) != len(set(ids)) or any(not re.fullmatch(r"[A-Za-z0-9_-]+", name) for name in ids):
        parser.error("duplicate or unsafe case IDs")
    if options.selected:
        unknown = set(options.selected) - set(ids)
        if unknown:
            parser.error(f"unknown case IDs: {sorted(unknown)}")
        cases = [case for case in cases if case["id"] in options.selected]
    binaries = {variant: getattr(options, variant).resolve(strict=True)
                for variant in ("before", "after")}
    provenance = {}
    for variant in binaries:
        path = getattr(options, f"{variant}_manifest").resolve(strict=True)
        provenance[variant] = {"path": str(path), **digest(path), "content": json.loads(path.read_text())}
    hashes = source_hashes(cases)
    binary_hashes = {variant: digest(path) for variant, path in binaries.items()}
    output = options.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items()
           if not (key.startswith("SURTR_") and ("PROFILE" in key or "COUNTER" in key))}
    manifest = {"started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "cwd": str(ROOT), "cases": cases, "cases_manifest": digest(cases_path),
                "iterations": options.iterations, "timeout_seconds": options.timeout,
                "order": "iteration outermost; each case alternates before/after order by iteration; cold then disk_hit",
                "binaries": {variant: {"path": str(path), **binary_hashes[variant]}
                             for variant, path in binaries.items()},
                "build_provenance": provenance, **hashes,
                "python": sys.version, "platform": platform.platform(),
                "machine": platform.machine(), "processor": platform.processor(),
                "power_condition": options.power_condition,
                "rustc": probe(["rustc", "-Vv"]), "git_head": probe(["git", "rev-parse", "HEAD"]),
                "git_status": probe(["git", "status", "--short"])}
    if sys.platform == "darwin":
        manifest["power_probe"] = probe(["pmset", "-g", "batt"])
        manifest["cpu_probe"] = probe(["sysctl", "-n", "machdep.cpu.brand_string"])
    else:
        manifest["cpu_probe"] = probe(["lscpu"])
    save(output / "manifest.json", manifest)
    rows = []
    with (output / "runs.jsonl").open("w") as raw:
        for iteration in range(1, options.iterations + 1):
            variants = ("before", "after") if iteration % 2 else ("after", "before")
            for case in cases:
                for variant in variants:
                    cache = output / case["id"] / f"{iteration:02d}" / variant / "cache"
                    cache.mkdir(parents=True, exist_ok=False)
                    for condition in ("cold", "disk_hit"):
                        row = run_one(binaries[variant], case, variant, iteration, cache,
                                      condition, output, options.timeout, env)
                        raw.write(json.dumps(row, ensure_ascii=False) + "\n")
                        raw.flush()
                        if "error" in row:
                            save(output / "failure.json", row)
                            print(f"measurement aborted: {row['error']} ({case['id']}, {variant}, {condition}); {output}", file=sys.stderr)
                            return 1
                        rows.append(row)
                print(f"iteration {iteration}/{options.iterations}: {case['id']}", flush=True)
    if source_hashes(cases) != hashes or any(digest(path) != binary_hashes[variant]
                                            for variant, path in binaries.items()):
        save(output / "failure.json", {"error": "binary/input/lib hashes changed during measurement"})
        print("measurement aborted: binary/input/lib changed", file=sys.stderr)
        return 1
    save(output / "summary.json", summarize(rows))
    manifest["completed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    manifest["completed_runs"] = len(rows)
    manifest["final_hash_verification"] = "unchanged"
    save(output / "manifest.json", manifest)
    print(f"saved {len(rows)} runs: {output}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"measurement error: {error}", file=sys.stderr)
        sys.exit(1)
