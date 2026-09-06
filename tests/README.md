# Tests Layout

Surtr tests are organized by execution temperature.

- Hot: crate-local unit tests in `crates/**/src/**` and `crates/*/tests/**`
- Warm: disk fixtures under `tests/fixtures/**`
- Cold: CLI/process integration tests under `tests/integration/**`
- Profile: manual measurement inputs under `tests/profile/**`

Preferred runner: `cargo nextest run`

Coverage runner:

- Install once: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`
- Summary run: `cargo cov`
- HTML report: `cargo cov-html`
- JSON summary: `cargo cov-json`

## Fixture Suites

- `lib/tests/spec.srt`
  - Canonical aggregate PureSurtr success suite
  - Runner: `./target/debug/surtr test spec`
- `tests/fixtures/script/pass/**.srt` + `.expected`
  - Script-mode success fixtures for file boundary, stdmod, JSON, string, process-runtime, and usecase behavior
  - Runner: `tests/integration/run_srt.rs` (`run_srt::spec_fixtures_bucket_0..7`)
- `tests/fixtures/script/fail/**.srt` + `.error`
  - Script-mode compile error fixtures (`phase` and `contains` expectations)
  - Runner: `tests/integration/run_srt.rs` (`run_srt::compile_error_fixtures_bucket_0..15`)
- `tests/fixtures/modules/pass/**/entry.srt` + `entry.expected`
  - Multi-source module behavior fixtures
  - Runner: `tests/integration/module_import_fixtures.rs` (`module_spec_fixtures_bucket_0..3`)
- `tests/fixtures/modules/fail/**/entry.srt` + `entry.error`
  - Multi-source module compile-error fixtures
  - Runner: `tests/integration/module_import_fixtures.rs` (`module_compile_error_fixtures_bucket_0..3`)
- `tests/integration/*.rs`
  - CLI contract and pipeline integration tests
  - `language_features.rs` is organized into topic modules under `tests/integration/language_features/`
- `crates/xldr/tests/repl_core.rs`
  - 137 REPL core semantic cases registered once in a stable source-order table
  - Runner: 8 round-robin bucket tests (`index % 8`) plus one duplicate-name/function inventory test
- `tests/unit/{spire,sigil,scar,forge,eldr}/`
  - Unit-test viewpoints and crate-local notes
- `tests/profile/stdlib_prewarm.srt`
  - Minimal manual-profile input used by the `ci` nextest setup to prepare the default stdlib semantic snapshot

## Partial Commands

- Full gate: `cargo nextest run --workspace`
- CI/full gate: `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`
- Hot crate check: `cargo nextest run -p scar`
- Warm script fixtures: `cargo nextest run -p rune --test integration run_srt`
- One script bucket: `cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0`
- One compile-error bucket: `cargo nextest run -p rune --test integration run_srt::compile_error_fixtures_bucket_0`
- Warm module fixtures: `cargo nextest run -p rune --test integration module_import_fixtures`
- Cold run/build/dump boundary: `cargo nextest run -p rune --test integration run_eldr build_roundtrip`
- Cold REPL boundary: `cargo nextest run -p rune --test integration repl`
- Xldr REPL core: `rtk cargo nextest run --profile ci -p xldr --test repl_core`
- `surtr test` command boundary: `cargo nextest run -p rune --test integration test_command`

## Timing And Cache

Useful env vars:

- `SURTR_TEST_TIMING=1`
  - Print fixture count, elapsed time, cache counters, and slowest fixtures for `tests/integration/run_srt.rs`
- `SURTR_TEST_CACHE=1`
  - Opt in to the integration final `.eldr` fixture cache under `target/test-fixture-cache/eldr`
  - Does not gate the shared semantic prefix cache

The `ci` profile builds and lists test binaries, then runs `scripts/ci-stdlib-prewarm.sh` once from the workspace root before starting test processes. The script checks `tests/profile/stdlib_prewarm.srt` and prepares only the default-variant `std.semantic`; it does not prepare the test-enabled `std.test.semantic` or enable the final `.eldr` cache. Keep `SURTR_TEST_CACHE=1` explicit on the CI/full-gate command when that final cache is wanted.

The stdlib semantic cache is content-addressed from the stdlib sources and semantic schema/metadata. A missing, stale, or corrupt entry is ignored and rebuilt through the normal stdlib compile path, so a cache hit is an optimization rather than a correctness condition. Within each Xldr process, the default REPL bootstrap state restores source/scope/checkpoint/bytecode metadata from that semantic snapshot once. Each `ReplEngine` receives a fresh VM, runs the runtime boot plan independently, and clones session state; mutable runtime state is not shared. `.eldr` restore also validates the compiled stdlib function/type/callable/process prefix before applying that compile-time state.

Before snapshot-backed `ReplEngine` construction, the smallest stable Xldr layout used 64 buckets and took 55.888s for 65/65 tests in a current checkout. After the default bootstrap state became process-local and cloneable, the same 137 semantic cases use 8 buckets plus one inventory test: 9/9 tests passed in 7.180s under `profile ci`, below the unchanged 15s per-test timeout.

Cold run:

```bash
rm -rf target/test-fixture-cache
RUST_TEST_THREADS=1 SURTR_TEST_TIMING=1 SURTR_TEST_CACHE=1 cargo nextest run -p rune --test integration run_srt --no-capture
```

Hot run:

```bash
RUST_TEST_THREADS=1 SURTR_TEST_TIMING=1 SURTR_TEST_CACHE=1 cargo nextest run -p rune --test integration run_srt --no-capture
```

Test-related compilation caches are layered:

- Shared semantic prefix cache on top of the stdlib snapshot
- Final `.eldr` fixture cache as the top-layer artifact cache
- Integration support stores prefix entries under `target/test-fixture-cache/prefix`
- `surtr test` stores prefix entries under `target/surtr-test-cache/prefix`

`.error` format:

```txt
phase: typecheck
contains: expected Int, got String
```
