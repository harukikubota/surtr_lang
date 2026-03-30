# Tests Layout (Restructured)

This repository uses a spec-first test layout.

- `tests/spec/**.srt` + `.expected`
  - End-to-end behavior fixtures (stdout match)
  - Runner: `tests/integration/run_srt.rs`
- `tests/compile_errors/**.srt` + `.error`
  - Compile error fixtures (`phase` and `contains` expectations)
  - Runner: `tests/integration/run_srt.rs`
- `tests/integration/*.rs`
  - CLI contract and pipeline integration tests
  - Registered from `crates/rune/Cargo.toml` via `[[test]]`
- `tests/unit/{spire,sigil,scar,forge,eldr}/`
  - Unit-test viewpoints and crate-local links

`.error` format:

```txt
phase: typecheck
contains: expected Int, got String
```
