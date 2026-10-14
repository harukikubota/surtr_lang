# Empty Future metadata clone

現行private sourceのready mapは`HashMap<u64, &Value>`を受け取るため、probeのready fixtureも参照を渡す。古いowned版sourceを再現するときは、そのbuild記録のprobe hashと対応するGit版を使う（`c76a0980`以前のprobe）。現行sourceの内容確認は[記録](../../../doc/optimize/runtime/value/future_ready_snapshot_borrow_2026_10_14.md)を参照する。

This standalone release probe imports the exact private `OperandStack` source
and links the Cargo-built `sindr` library. It measures this one owner, including
clone and destruction, without a VM, scheduler or test harness. It is not a
whole-checkpoint or REPL benchmark.

Build `sindr` and capture its Cargo JSON artifact, then retain before/after source
files and build both probes. The builder checks the artifact manifest, records
source/library/binary hashes, and uses the artifact dependency directory.

```sh
cargo build --release --locked -p sindr --message-format=json --target-dir /absolute/profile-target > /absolute/sindr-build.jsonl
python3 tests/profile/future_stack_clone/build.py --before-source /absolute/before_operand_stack.rs --after-source crates/eldr/src/vm/operand_stack.rs --sindr-build /absolute/sindr-build.jsonl --output-dir /absolute/probes
python3 tests/profile/future_stack_clone/run.py --builds /absolute/probes/builds.json --output doc/optimize/runtime/value/future_stack_empty_clone_new_run --samples 6 --clones 1000
```

The before state is derive Clone; after omits an empty boxed position buffer.
The cases never / cleared / popped / truncated / resolved / pending cover
reservation absence, four ways to retire positions, and required metadata.
Lengths are 0 / 16 / 256 ordinary Unit values. The pending control appends two
identical Future IDs. Each process retains 1,000 clones in preallocated root
storage, then clears them. Setup and root storage are outside the intervals.

Allocation mode counts actual successful Rust GlobalAlloc requests delegated to
System: allocation/reallocation requests, requested/freed bytes, and logical
live/peak bytes within the clone/drop scope. Allocator-internal overhead, RSS,
and preparation are excluded. The custom allocator exists only in this probe;
it does not instrument Eldr or the normal test gate.

Timing mode disables counters but retains the allocator wrapper's disabled
branch, and measures clone and drop separately. The runner removes unmeasured
placeholder fields emitted by the shared probe format. Full content/equality
checks run in separate verification processes. Six AB/BA balanced pairs and
fresh processes are used for each case/mode. Binary hashes are checked before
and after the series. Runners refuse existing raw files.

See [scope and results](../../../doc/optimize/runtime/value/future_stack_empty_clone_2026_10_14.md).


For owned-to-shared nonempty metadata, run with `--before-metadata owned
--after-metadata shared`. The default legacy/owned modes preserve the earlier
empty-metadata comparison. Storage modes describe source states, not product
configuration switches.

`resume.rs` adds separate metadata construction, clone, first operation and drop
intervals. Build it with `build.py --probe tests/profile/future_stack_clone/resume.rs`.
`resume.py --builds /absolute/resume/builds.json --output /absolute/results` uses
0/16 ordinary values, 1/256/4096 pending positions, 32 clones and none/append/resolve
operations. It uses separate allocation/time processes and separate content
verification. The original owner stays alive during each branch mutation.

Value storage and clone root storage are preallocated outside all intervals.
Construction counts only position metadata and position-vector growth; its live
storage stays with the original and is excluded from clone/mutation/drop live
counters. Do not combine phase peaks. Clone and mutation allocations must all be
freed in the drop interval. Sum request totals to assess shifted copy costs.
See [sharing and resume results](../../../doc/optimize/runtime/value/future_stack_position_sharing_2026_10_14.md).
