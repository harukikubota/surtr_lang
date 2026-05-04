# Tests Layout (Restructured)

This repository uses a spec-first test layout.

Preferred runner: `cargo nextest run`

Coverage runner:

- Install once: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`
- Summary run: `cargo cov`
- HTML report: `cargo cov-html`
- JSON summary: `cargo cov-json`

- `lib/tests/spec.srt`
  - Canonical aggregate PureSurtr success suite
  - Runner: `rune test spec`
- `tests/spec/**.srt` + `.expected`
  - Success fixtures kept on disk when file boundaries, include/import resolution, or other non-PureSurtr behavior is the thing under test
  - Runner: `tests/integration/run_srt.rs` (`spec_fixtures_bucket_0..3`)
- `tests/compile_errors/**.srt` + `.error`
  - Compile error fixtures (`phase` and `contains` expectations)
  - Runner: `tests/integration/run_srt.rs` (`compile_error_fixtures_bucket_0..3`)
- `tests/spec/modules/**/entry.srt` + `entry.expected`
  - Multi-source module behavior fixtures
  - Runner: `tests/integration/module_import_fixtures.rs` (`module_spec_fixtures_bucket_0..3`)
- `tests/compile_errors/modules/**/entry.srt` + `entry.error`
  - Multi-source module compile-error fixtures
  - Runner: `tests/integration/module_import_fixtures.rs` (`module_compile_error_fixtures_bucket_0..3`)
- `tests/integration/*.rs`
  - CLI contract and pipeline integration tests
  - `language_features.rs` is organized into topic modules under `tests/integration/language_features/`
- `tests/unit/{spire,sigil,scar,forge,eldr}/`
  - Unit-test viewpoints and crate-local notes

Useful env vars:

- `SURTR_TEST_TIMING=1`
  - Print phase / slow-fixture breakdown for `tests/integration/run_srt.rs`
- `SURTR_TEST_CACHE=1`
  - Opt in to `.eldr` fixture cache under `target/test-fixture-cache/eldr`

`.error` format:

```txt
phase: typecheck
contains: expected Int, got String
```
