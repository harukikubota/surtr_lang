# Nonempty Future table / operand stack

Manual release probes for OI-036. The unit probe constructs a List from repeated
LoadLocal opcodes with an empty, unrelated Running/Ready, or stack-referenced
Running/Ready Future. Preparation is outside VM::run timing. Values are Int(7).
The same test function and counters run in baseline and candidate binaries.

Build and save each source state with independent target directories. Read the
Cargo JSON compiler-artifact executable and retain the source hashes. The unit
binary must include `vm::future_stack_tests::nonempty_future_stack_profile`.
The build records consumed by run.py contain `source_sha256` plus `records`
(command, artifact, saved binary path, binary_sha256). Both unit and Rune CLI
artifacts are recorded in the OI-036 report.

```sh
cargo test --release --locked -p eldr --lib --no-run --message-format=json --target-dir /absolute/before-target
cargo build --release --locked -p rune --bin surtr --message-format=json --target-dir /absolute/before-target
python3 tests/profile/future_stack_scan/run.py --before /absolute/before_builds.json --after /absolute/after_builds.json --before-layout count --after-layout indexed --output doc/optimize/runtime/value/future_stack_new_run --samples 6
python3 tests/profile/future_stack_scan/batch.py --before /absolute/before-surtr --after /absolute/after-surtr --output doc/optimize/runtime/value/future_stack_batch_new_run --samples 6
```

run.py uses 1,000 / 10,000 elements, five Future conditions, six AB/BA balanced
pairs per condition, and fresh processes. It validates exact read/write visit
counts. Full List content verification uses separate PROBE_VERIFY processes;
those results omit elapsed time and RSS. Select `--before-layout full|count|indexed` and `--after-layout full|count|indexed`
to check the actual source variants (defaults: full/count). A live PendingFuture
still triggers readiness checks; the indexed variant visits only that slot on each
opcode. This is not a fairness or allocation benchmark.

batch.py generates ordinary Task::async / List literal / List::len / Task::await
programs, plus controls without a Task. Compiler artifacts are warmed for both
executables before measurement; every measured process must report cache hit.
CLI execute includes runtime setup, run, background drain, and output. CLI total
also includes cache loading; parent process wall includes startup/final cleanup.
These are distinct from the unit probe's VM::run interval. Outputs are validated.
Source text and binary/source/runner hashes are retained in the summaries.

Darwin process peak RSS includes preparation, validation, JSON and test harness;
it is not a phase's retained memory. Allocation requests/live bytes and physical
copy bytes are unmeasured. Logical scan counters are enabled in unit time series.
No time or RSS thresholds are correctness tests. Runners refuse existing raw
output files. Stop builds/tests before performance samples.

See [implementation and measurements](../../../doc/optimize/runtime/value/future_stack_nonempty_optimization_2026_10_13.md).

The indexed follow-up compares count/indexed layouts, and a second series compares
indexed/indexed binaries with and without stack-operation inline hints. Preserve
both build records and treat binary code layout as part of the measured variant.
See [follow-up plan](../../../doc/optimize/runtime/value/future_stack_index_plan_2026_10_13.md).
