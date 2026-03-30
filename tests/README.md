# Tests Layout (Restructured)

This repository uses the following test fixture layout:

- `tests/spec/**.srt` + `.expected`
  - End-to-end behavior fixtures
  - Runner: `crates/rune/tests/spec_fixture_tests.rs`
- `tests/compile_errors/**.srt` + `.error`
  - Compile error fixtures (`phase` and `contains` expectations)
  - Runner: `crates/rune/tests/spec_fixture_tests.rs`
- `tests/e2e/**`
  - Legacy fixture set kept temporarily for migration safety

`.error` format:

```txt
phase: typecheck
contains: expected Int, got String
```

