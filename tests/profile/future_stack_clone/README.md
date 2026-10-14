# Empty Future metadata clone

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
