#!/usr/bin/env python3
"""Record process-warm and incremental Scar baselines (no VM execution)."""

import argparse
import json
import math
import os
from pathlib import Path
import re
import statistics
import subprocess
import time

from measure import ROOT, digest, probe, save

TIMING_FIELDS = {"parse", "resolve", "scar", "restore", "checkpoint", "rollback",
                 "resolver_restore", "resolver_checkpoint", "resolver_rollback",
                 "resolved_clone", "frontend_total"}


def source_state():
    paths = set(ROOT.glob("crates/**/*.rs"))
    paths.update(ROOT.glob("crates/**/Cargo.toml"))
    paths.update(ROOT.glob("lib/**/*.srt"))
    paths.update(ROOT.glob("tests/profile/scar_compile/**/*.py"))
    paths.update(ROOT.glob("tests/profile/scar_compile/**/*.srt"))
    paths.update(ROOT / name for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"))
    return {str(path.relative_to(ROOT)): digest(path)
            for path in sorted(paths) if path.is_file()}


def validate_invocation(rows, mode, api, size, iterations):
    if mode not in ("warm", "incremental") or api not in ("transactional", "in_place"):
        raise ValueError("invalid mode/API")
    if mode == "incremental" and api != "transactional":
        raise ValueError("incremental requires transactional API")
    if type(size) is not int or size < 1 or type(iterations) is not int or iterations < 1:
        raise ValueError("size/iterations must be positive integers")
    if any(type(row) is not dict for row in rows):
        raise ValueError("rows must be objects")
    setup = [row for row in rows if row.get("event") == "setup"]
    if len(setup) != 1 or rows[0].get("event") != "setup":
        raise ValueError("expected exactly one initial setup event")
    if any(row.get("event") not in ("setup", "sample") for row in rows):
        raise ValueError("unexpected event")
    for key, value in (("mode", mode), ("api", api), ("size", size), ("iterations", iterations)):
        if setup[0].get(key) != value:
            raise ValueError(f"setup {key} mismatch")
    expected = {(iteration, chunk): ("expected_failure" if mode == "incremental" and chunk == size + 1
                                    else "success")
                for iteration in range(iterations)
                for chunk in range(1 if mode == "warm" else size + 3)}
    seen = set()
    timing_keys = {}
    semantics = {}
    for row in rows[1:]:
        if any(row.get(key) != value for key, value in (("mode", mode), ("api", api), ("size", size))):
            raise ValueError("sample mode/API/size mismatch")
        if type(row.get("iteration")) is not int or type(row.get("chunk")) is not int:
            raise ValueError("sample iteration/chunk must be integers")
        key = (row["iteration"], row["chunk"])
        if key in seen or key not in expected or row.get("status") != expected[key]:
            raise ValueError("duplicate, unexpected, or incorrectly classified sample")
        seen.add(key)
        timings = row.get("timings_us")
        if type(timings) is not dict or not TIMING_FIELDS <= timings.keys():
            raise ValueError("missing required session timing")
        for name, value in timings.items():
            if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
                raise ValueError(f"invalid timing: {name}={value}")
        chunk = row["chunk"]
        if chunk in timing_keys and timing_keys[chunk] != timings.keys():
            raise ValueError("timing fields differ between repetitions")
        timing_keys[chunk] = timings.keys()
        for field in ("source_sha256", "semantic_signature"):
            value = row.get(field)
            if field == "semantic_signature" and row["status"] == "expected_failure":
                if field not in row or value is not None:
                    raise ValueError("rejected sample has a typed signature")
            elif type(value) is not str or not re.fullmatch("[0-9a-f]{64}", value):
                raise ValueError(f"invalid {field}")
        for field, minimum in (("input_bytes", 1), ("retained_user_definitions_before", 0),
                               ("retained_user_definitions_after", 0)):
            if type(row.get(field)) is not int or row[field] < minimum:
                raise ValueError(f"invalid {field}")
        for field, value in (("resolved_clone_in_scar", False), ("resolved_clone_performed", False),
                             ("typed_metadata_in_scar", True), ("session_restore_in_scar", False)):
            if row.get(field) is not value:
                raise ValueError(f"unexpected timing boundary: {field}")
        before, after = row["retained_user_definitions_before"], row["retained_user_definitions_after"]
        if mode == "warm":
            if before != 0:
                raise ValueError("warm sample retained prior user declarations")
        else:
            expected_counts = ((chunk, chunk + 1) if chunk < size else
                               (size, size + 3) if chunk == size else
                               (size + 3, size + 3 + (chunk == size + 2)))
            if (before, after) != expected_counts:
                raise ValueError("incremental retained declaration counts mismatch")
        if row["status"] == "expected_failure":
            diagnostic = row.get("diagnostic")
            if (type(diagnostic) is not dict
                    or diagnostic.get("reason") != "Some(ReturnTypeMismatch)"
                    or type(diagnostic.get("span_start")) is not int or diagnostic["span_start"] <= 0
                    or type(diagnostic.get("span_end")) is not int
                    or diagnostic["span_end"] <= diagnostic["span_start"]
                    or type(diagnostic.get("message")) is not str or not diagnostic["message"]
                    or row.get("resolver_rollback_verified") is not True
                    or "typed_top_level_nodes" not in row or row["typed_top_level_nodes"] is not None):
                raise ValueError("unexpected rejection or unverified rollback")
        elif (type(row.get("typed_top_level_nodes")) is not int or row["typed_top_level_nodes"] < 0
              or "diagnostic" not in row or row["diagnostic"] is not None):
            raise ValueError("invalid successful typed output")
        if mode == "incremental" and chunk == size + 2 and row.get("reused_failed_binding") is not True:
            raise ValueError("recovery did not reuse the rejected name")
        stable = {field: row[field] for field in ("source_sha256", "input_bytes", "semantic_signature",
                  "typed_top_level_nodes", "retained_user_definitions_before", "retained_user_definitions_after",
                  "diagnostic")}
        if chunk in semantics and semantics[chunk] != stable:
            raise ValueError("source or typed semantics changed between repetitions")
        semantics[chunk] = stable
    if seen != expected.keys():
        raise ValueError("missing samples")


def summarize(rows, iterations):
    grouped = {}
    for row in rows:
        if row.get("event") != "sample":
            continue
        key = tuple(row[name] for name in ("case", "mode", "api", "size", "chunk", "status"))
        group = grouped.setdefault(key, {"iterations": set(), "timings": {}})
        iteration = row["iteration"]
        if type(iteration) is not int or not 0 <= iteration < iterations:
            raise ValueError(f"invalid iteration: {iteration}")
        if iteration in group["iterations"]:
            raise ValueError(f"duplicate sample: {key}, iteration {iteration}")
        group["iterations"].add(iteration)
        timings = row["timings_us"]
        if "scar" not in timings or "frontend_total" not in timings:
            raise ValueError("missing Scar/frontend timing")
        for name, value in timings.items():
            if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
                raise ValueError(f"invalid timing: {name}={value}")
            group["timings"].setdefault(name, []).append(value)
    if not grouped:
        raise ValueError("no samples")
    result = []
    for key, group in grouped.items():
        if group["iterations"] != set(range(iterations)):
            raise ValueError(f"incomplete repetitions: {key}")
        record = dict(zip(("case", "mode", "api", "size", "chunk", "status"), key))
        record["timings_us"] = {
            name: {"count": len(values), "median": statistics.median(values),
                   "min": min(values), "max": max(values)}
            for name, values in group["timings"].items()
        }
        result.append(record)
    return {"duration_unit": "us", "results": result}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--build-manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--iterations", type=int, default=10)
    parser.add_argument("--power-condition", required=True)
    parser.add_argument("--timeout", type=float, default=300)
    options = parser.parse_args()
    if options.iterations < 1 or options.timeout <= 0:
        parser.error("iterations and timeout must be positive")
    binary = options.binary.resolve(strict=True)
    build = json.loads(options.build_manifest.read_text())
    before = {"sources": source_state(), "binary": digest(binary)}
    if build["sources"] != before["sources"] or build["binary"] != before["binary"]:
        parser.error("build manifest does not match current sources/binary")
    output = options.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    environment = dict(os.environ, SURTR_STDLIB_CACHE_DIR=str(output / "cache"))
    removed = sorted(key for key in environment
                     if key == "RUST_MIN_STACK" or (key.startswith("SURTR_")
                     and ("PROFILE" in key or "COUNTER" in key)))
    for key in removed:
        del environment[key]
    inputs = [("stdlib_prewarm", "tests/profile/stdlib_prewarm.srt"),
              ("patterns", "tests/profile/scar_compile/features/patterns.srt"),
              ("monad_carriers", "tests/profile/scar_compile/features/monad_carriers.srt")]
    for series, sizes in (("generic_chain", (16, 32, 64)),
                          ("error_definitions", (64, 128, 256)),
                          ("trait_impls", (16, 32, 64))):
        inputs.extend((f"{series}_{size:03}",
                       f"tests/profile/scar_compile/generated/{series}_{size:03}.srt")
                      for size in sizes)
    cases = [(name, "warm", 0, path) for name, path in inputs]
    cases += [(f"incremental_{size:03}", "incremental", size, None) for size in (16, 32, 64)]
    manifest = {"build": build, "cwd": str(ROOT), "iterations": options.iterations,
                "power_condition": options.power_condition, "removed_environment": removed,
                "environment": {key: value for key, value in environment.items()
                                if key.startswith(("SURTR_", "CARGO_", "RUST"))},
                "host": probe(["uname", "-a"]), "cpu": probe(["sysctl", "-n", "machdep.cpu.brand_string"]),
                "power": probe(["pmset", "-g", "batt"]), "commands": []}
    # Include control inputs outside scar_compile in the immutability check.
    input_hashes = {path: digest(ROOT / path) for _, path in inputs}
    manifest["inputs"] = input_hashes
    save(output / "manifest.json", manifest)
    rows = []
    try:
        for name, mode, size, path in cases:
            for api in (("transactional", "in_place") if mode == "warm" else ("transactional",)):
                command = [str(binary), "--mode", mode, "--api", api,
                           "--iterations", str(options.iterations), "--size", str(size or 16)]
                if path:
                    command += ["--input", path]
                stem = output / f"{name}-{api}"
                start = time.monotonic()
                try:
                    process = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True,
                                             text=True, timeout=options.timeout)
                except subprocess.TimeoutExpired as error:
                    stem.with_suffix(".stdout").write_bytes(error.stdout or b"")
                    stem.with_suffix(".stderr").write_bytes(error.stderr or b"")
                    manifest["commands"].append({"case": name, "api": api, "args": command,
                                                 "exit_code": None, "error": "timeout",
                                                 "wall_seconds": time.monotonic() - start})
                    save(output / "manifest.json", manifest)
                    raise
                stem.with_suffix(".stdout").write_text(process.stdout)
                stem.with_suffix(".stderr").write_text(process.stderr)
                invocation = {"case": name, "api": api, "args": command,
                              "exit_code": process.returncode, "wall_seconds": time.monotonic() - start,
                              "stdout": stem.name + ".stdout", "stderr": stem.name + ".stderr"}
                manifest["commands"].append(invocation)
                save(output / "manifest.json", manifest)
                if process.returncode:
                    raise ValueError(f"{name}/{api}: exit {process.returncode}; see {stem.name}.stderr")
                samples = [json.loads(line) for line in process.stdout.splitlines()]
                validate_invocation(samples, mode, api, size or 16, options.iterations)
                if path and any(row["source_sha256"] != input_hashes[path]["sha256"]
                                or row["input_bytes"] != input_hashes[path]["bytes"]
                                for row in samples[1:]):
                    raise ValueError("sample does not match the requested input file")
                for row in samples:
                    row["sample_case"] = row.get("case")
                    row["case"] = name
                    if row.get("event") == "sample" and (row["mode"] != mode or row["api"] != api):
                        raise ValueError("sample mode/API mismatch")
                summarize(samples, options.iterations)
                rows.extend(samples)
                with (output / "runs.jsonl").open("a") as stream:
                    for row in samples:
                        stream.write(json.dumps(row, ensure_ascii=False) + "\n")
        after = {"sources": source_state(), "binary": digest(binary)}
        if before != after or input_hashes != {path: digest(ROOT / path) for _, path in inputs}:
            raise ValueError("sources, inputs, or binary changed during measurement")
        manifest["final_hash_verification"] = "unchanged"
        save(output / "manifest.json", manifest)
        save(output / "summary.json", summarize(rows, options.iterations))
    except Exception as error:
        save(output / "failure.json", {"error": str(error)})
        raise


if __name__ == "__main__":
    main()
