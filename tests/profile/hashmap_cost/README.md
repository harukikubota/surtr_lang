# RT-4 HashMap cost probe

Manual measurements outside the workspace correctness gate. Uses current public
Rust APIs and `VM::run`; no product paths, bytecode versions or thresholds change.

Six cases, n=256 / 512 / 1024, Int(index) or 256-byte String containing the index.
Keys are 8 ASCII bytes. `rust_same` / `vm_same` overwrite one key; other cases use
distinct keys. Each case runs in five fresh processes per allocator mode.

| case | prepare | run | drop |
|---|---|---|---|
| rust_distinct / rust_same | Vec of owned keys and values | `HashMapHandle::from_entries`, consuming the input Vec | output map |
| sort | directly collect entries into public HashMap storage | `sorted_entries`: clone all entries and sort | sorted Vec and input map |
| vm_distinct / vm_same | bytecode/constants, VM and observer | boot, validation, constant loads, Tuple/List creation, `map_from_entries` | entire VM including output, constants and observer |
| vm_map_values | same VM preparation | same distinct input construction, then `map_values` with identity callback | entire VM |

VM run is a whole-workload measurement. It does not isolate builtin preparation,
individual callbacks or resumes. Do not subtract other workloads to label a
measurement as `map_values` alone. Observer opcode aggregation remains enabled;
opcode/call trace is disabled. The observed Return count is a proxy only for this
identity workload, which has no other user functions or TCO.

`existing_entry_clones_expected` is derived from current construction code, not an
instrumented clone counter. It is zero after the owned-storage optimization.
The saved baseline used distinct construction: n(n−1)/2, same-key construction:
max(n−1,0), and VM map_values: n(n−1).
The field excludes argument/iterator/constant loads, callback cloning, sort clones,
and drop work. `sort` has zero *insert-related* existing-entry clones, but still
clones n entries. Strings and BigInt clone payloads; physical byte copying is not
counted.

Rust from_entries run frees its input Vec and consumes keys/values, so its signed
live delta is not output storage size. Sort keeps input and output alive together.
VM run keeps bytecode constants and output alive. Process peak RSS includes all
three intervals; it is not per-builtin RSS. Requested bytes are allocator layout
sizes, not RSS or physical memory. Realloc adds the full new layout to requested
and the old layout to freed. Peak extra is logical live above interval start;
allocator-internal realloc overlap/retained pages are not observed. Counter-disabled
allocation fields are null. Counting overhead is not subtracted from timing.

From the repository root:

```bash
cargo build --release --locked --manifest-path tests/profile/hashmap_cost/Cargo.toml --target-dir /tmp/surtr-rt4-after-count
cargo build --release --locked --no-default-features --manifest-path tests/profile/hashmap_cost/Cargo.toml --target-dir /tmp/surtr-rt4-after-time
python3 tests/profile/hashmap_cost/verify.py --binary /tmp/surtr-rt4-after-count/release/hashmap-cost-probe
python3 tests/profile/hashmap_cost/run.py --binary /tmp/surtr-rt4-after-count/release/hashmap-cost-probe --output doc/hashmap_cost_2026_10_11_after_count
python3 tests/profile/hashmap_cost/run.py --binary /tmp/surtr-rt4-after-time/release/hashmap-cost-probe --output doc/hashmap_cost_2026_10_11_after_time
python3 tests/profile/hashmap_cost/recheck.py --output doc/hashmap_cost_2026_10_11_recheck
```

The saved baseline files and binaries describe the pre-optimization product at
547c6a68. The commands above write separate after artifacts and leave them intact.
Only clone expectation/provenance reporting changed in the probe; workload
construction and measured intervals are unchanged.
`recheck.py` reads the saved time summaries, verifies their binary hashes, and
alternates before/after (reversing pair order on odd samples) for four 1,024-entry
String cases. It saves 80 fresh processes / 240 intervals separately by default;
run it after other builds and tests finish. It never updates the main series.

The lockfile uses root lock dependency versions, pruned by Cargo for this package.
`run.py` requires macOS `/usr/bin/time -l` for process peak RSS bytes and rejects
PROBE_VERIFY. `verify.py` checks full contents and overwrite results in 60 separate
processes (all measured configurations plus empty/single-entry boundaries), without
saving performance samples. JSON formatting/output happens after all measured
intervals. Compile/build, subprocess orchestration and `/usr/bin/time` reporting
are outside the measured intervals.

The summary records toolchain, platform, base source commit, binary/source/runner/lock,
changed product sources and the tracked product diff SHA-256. The after measurement
uses an uncommitted product diff on the base commit. Measurements use randomized
Rust hash seeds across fresh processes.
Results describe these inputs, not scheduler fairness, cancellation, checkpoint,
JSON object conversion or compiler/REPL/Task/process entry performance.
