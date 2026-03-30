# Tests Layout (Restructured)

This repository uses a spec-first test layout.

- `tests/spec/**.srt` + `.expected`
  - End-to-end behavior fixtures (`stdout` match)
  - Runner: `tests/integration/run_srt.rs` (`spec_fixtures_match_expected_stdout`)
- `tests/compile_errors/**.srt` + `.error`
  - Compile error fixtures (`phase` and `contains` expectations)
  - Runner: `tests/integration/run_srt.rs` (`compile_error_fixtures_match_expectations`)
- `tests/integration/*.rs`
  - CLI contract and pipeline integration tests
- `tests/unit/{spire,sigil,scar,forge,eldr}/`
  - Unit-test viewpoints and crate-local notes

`.error` format:

```txt
phase: typecheck
contains: expected Int, got String
```
