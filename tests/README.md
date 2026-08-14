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
- `tests/unit/{spire,sigil,scar,forge,eldr}/`
  - Unit-test viewpoints and crate-local notes

## Partial Commands

- Full gate: `cargo nextest run --workspace`
- Hot crate check: `cargo nextest run -p scar`
- Warm script fixtures: `cargo nextest run -p rune --test integration run_srt`
- One script bucket: `cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0`
- One compile-error bucket: `cargo nextest run -p rune --test integration run_srt::compile_error_fixtures_bucket_0`
- Warm module fixtures: `cargo nextest run -p rune --test integration module_import_fixtures`
- Cold run/build/dump boundary: `cargo nextest run -p rune --test integration run_eldr build_roundtrip`
- Cold REPL boundary: `cargo nextest run -p rune --test integration repl`
- `surtr test` command boundary: `cargo nextest run -p rune --test integration test_command`

## Timing And Cache

Useful env vars:

- `SURTR_TEST_TIMING=1`
  - Print fixture count, elapsed time, cache counters, and slowest fixtures for `tests/integration/run_srt.rs`
- `SURTR_TEST_CACHE=1`
  - Opt in to the integration final `.eldr` fixture cache under `target/test-fixture-cache/eldr`
  - Does not gate the shared semantic prefix cache

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
