# Profile Fixtures

`tests/profile/**` is for manual measurement. These inputs are not part of the normal nextest gate.

## Heavy Compile

`heavy_compile.srt` is a compile-heavy script fixture for checking compile/cache behavior outside the fixture runner.

Build once before measuring:

```bash
cargo build -p rune
```

Cold run:

```bash
rm -rf target/run-cache /tmp/surtr-heavy.eldr
/usr/bin/time -p ./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/surtr-heavy.eldr
```

Hot run:

```bash
/usr/bin/time -p ./target/debug/surtr build tests/profile/heavy_compile.srt /tmp/surtr-heavy.eldr
```

For fixture-runner cold/hot reports, prefer the opt-in integration report:

```bash
rm -rf target/test-fixture-cache
RUST_TEST_THREADS=1 SURTR_TEST_TIMING=1 SURTR_TEST_CACHE=1 cargo nextest run -p rune --test integration run_srt --no-capture
RUST_TEST_THREADS=1 SURTR_TEST_TIMING=1 SURTR_TEST_CACHE=1 cargo nextest run -p rune --test integration run_srt --no-capture
```

`legacy_orphans/**` keeps historical `.srt` inputs that were present without a
matching `.expected` or `.error` file before the fixture relocation. They are
not part of the normal nextest gate until an explicit expectation is added.

## Regex Input Ownership

[`regex_input_cost/`](regex_input_cost/README.md) measures whole VM Regex::find_all work,
result handle clones, and retained input lifetime. Use independent baseline/after
target directories and keep allocation and time series separate. The recorded
results are in [`doc/optimize/runtime/regex/`](../../doc/optimize/runtime/regex/).

## Checkpoint Builder

[`checkpoint_builder/`](checkpoint_builder/README.md) measures suspended Task
checkpoint, resume, shared completion, destruction, and completion without a
checkpoint. [`checkpoint_builder_short/`](checkpoint_builder_short/README.md)
uses outputs of 0 / 1 / 8 / 32 / 256 elements. Both use crate-local Eldr test
probes, logical copy counters, fresh processes, and separate content verification.
Results are in [`doc/optimize/runtime/checkpoint/`](../../doc/optimize/runtime/checkpoint/).
