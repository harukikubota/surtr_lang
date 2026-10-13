# Regex input ownership profile

Manual release probe outside the workspace gate. The same source runs against baseline
main `b5229abd` (or another recorded baseline) and the immutable-input-sharing version. This compares whole VM work and
result handle ownership; it does not isolate regex search cost or measure scheduler fairness.

Build baseline and after from separate source roots. Put this package at the same relative
path in each root; keep an identical Cargo.lock (versions match the root lock). Save each
binary before a subsequent build overwrites it. Use the same target settings and toolchain.

```sh
cargo build --release --locked --manifest-path tests/profile/regex_input_cost/Cargo.toml
# Copy release/regex-input-cost-probe as before-count or after-count.
cargo build --release --locked --no-default-features --manifest-path tests/profile/regex_input_cost/Cargo.toml
# Copy as before-time or after-time.
python3 tests/profile/regex_input_cost/run.py --baseline-root /absolute/baseline/root --before-count /absolute/before-count --before-time /absolute/before-time --after-count /absolute/after-count --after-time /absolute/after-time --output doc/optimize/runtime/regex/regex_input_cost_new_run --samples 5
```

Cases: ASCII input 1,024 / 2,048 / 4,096 bytes, pattern `a` (dense), `a+` (single), `z` (none);
UTF-8 `éあ` repeated 128 times and empty pattern (257 zero-width matches over 640 bytes).
Every condition runs in a fresh process. Baseline/after alternate AB/BA by sample; counting
and non-counting series remain separate. Do not run builds/tests alongside measurements.

| Phase | Included work |
|---|---|
| `run` | `VM::run`: bytecode validation, compile builtin, pattern compilation at find_all, runtime input construction, matching, output List construction; VM setup is outside |
| `collect_handles` | `List::iter` and collection into Vec, which clone each RegexMatch Value |
| `clone_handles` | Additional Vec<Value> clone, including allocation for the outer Vec |
| `drop_vm` | Drop VM, bytecode, constants and original result while both handle vectors remain alive |
| `drop_handles` | Drop first handle vector while clone vector remains alive |
| `drop_clones` | Drop final handles |

All six metrics are buffered until workload cleanup completes; JSON formatting/output and
verification are outside measured regions. Sequential phase allocator state can affect later
phases; these are ownership scenarios, not independent allocator kernels.

`PROBE_VERIFY=1 binary N CASE` performs span, full-input length, zero-width UTF-8 boundaries
and unique input-buffer checks after VM destruction in a separate process. Verification may
allocate/scan and is never included in the performance series. Expect baseline buffers equal
match count after collection, and after-sharing one for nonempty result, zero for no result.
Performance rows' `verified_unique_input_buffers` is null. `logical_referenced_input_bytes`
is B×M, not distinct physical storage or live memory. The shared result still retains whole
input as long as any match remains; short-match retained-full-input cost is not solved.

Count-enabled allocator metrics record requested layout sizes, cumulative allocation and
logical live deltas; realloc counts new requested size and old freed size. They do not record
physical bytes copied, allocator memory or RSS. Non-counting allocation fields are null.
`/usr/bin/time -l` records Darwin maximum resident bytes for the entire process, including
VM preparation and JSON output; the same high-water mark is attached to each phase and
must not be summed. It is not the phase RSS or logical retained input size. No time/RSS
threshold is used as correctness criteria.

Summary metadata stores binary/product/probe/root-lock hashes, toolchain/platform,
working tree status and child-inherited allocator environment. Record build source separately
and build immediately before saving binaries. Runner source hashes identify source state at
execution; they cannot independently prove which source built an arbitrary binary.

## Public batch entry

`batch.py` generates temporary ordinary Surtr programs: Result `do`, Regex::compile,
Regex::find_all, List::map with RegexMatch::text, and Ok(result count) output.
Use independently built baseline/after Rune release binaries. A shared target directory
across different worktree source states can reuse stale library artifacts; full binary SHA
alone is insufficient when only metadata changes. Save the executable named by Cargo's
JSON compiler-artifact and record the Sindr/Eldr/Rune manifests. Use separate target dirs.

```sh
python3 tests/profile/regex_input_cost/batch.py --before /absolute/before-surtr --after /absolute/after-surtr --output doc/regex_batch_new_prefix --samples 5
```

Six inputs (1,024/4,096 bytes × dense/single/no-match), both binaries warmed outside
measurements, five fresh processes per executable/input with AB/BA ordering. Each measured
row requires artifact cache `hit`; executables have independent fingerprint keys in the
same cache directory. This is a cached batch entry comparison, not cold compiler time.
Observer/trace are off. Output count is verified; whole text/span/lifetime verification
comes from the separate manual VM and regression tests. CLI phase `execute` includes VM
construction/run/background drain/output reporting, but precedes final VM destruction.
`total_us` includes cached source/compile-plan/bytecode loading plus execution; parent
`process_wall_ns` includes process startup/final drop and `/usr/bin/time` overhead. RSS is
the whole-process high-water mark. Allocation is unmeasured. Keep warmups separate from
measured raw samples. Use a new prefix for every run to preserve previous data.
