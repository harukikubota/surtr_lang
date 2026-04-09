# Quality Audit (2026-04-09)

## Scope

- In scope: `spire`, `sigil`, `scar`, `forge`, `eldr`, `rune`, `sindr`, `diagnostics`, `xldr` CLI/REPL
- Out of scope for this pass: `xldr` TUI and `surtr tui`
  - Keep them in the version-bump verification checklist.

## Commands Run

```bash
cargo test --workspace
cargo nextest run
cargo clippy --workspace --all-targets -- -D warnings
cargo cov-json
```

## Result Snapshot

- `cargo test --workspace`: passed
- `cargo nextest run`: 144 passed, 5 skipped
- `cargo clippy --workspace --all-targets -- -D warnings`: the `sindr::ir` `type_complexity` issue fixed in this pass, but the workspace still has a pre-existing lint backlog concentrated in `eldr` (`result_large_err`, `map_identity`, `option_as_ref_deref`) and `spire` (`while_let_loop`, `approx_constant`)
- `cargo cov-json`: total line coverage `75.66%`, function coverage `77.97%`

## Coverage Hotspots

Non-TUI files with the largest remaining gaps:

- `forge/src/error.rs`: line `0.00%`
- `xldr/src/repl/logic/core.rs`: line `39.88%`
- `xldr/src/repl/ui/cli.rs`: line `48.50%`
- `diagnostics/src/lib.rs`: line `53.94%`
- `rune/src/error.rs`: line `57.95%`
- `rune/src/commands/dump.rs`: line `71.53%`
- `scar/src/checker.rs`: line `71.67%`
- `rune/src/commands/test.rs`: line `77.10%`

Out-of-scope TUI files remain at `0%` and were intentionally excluded from this audit.

## Missing Test Cases To Add Next

### diagnostics

- Add focused tests for hint-template inference on `if` branch mismatch and `match` arm mismatch formatting.
- Add tests for `report_error_by_id` fallback paths with missing source entries.

### sindr

- Add direct tests for docs chunk decode failure and malformed docs payload handling.
- Add a negative test for mixed valid `Code` payload plus invalid `Docs` payload.

### spire

- Add parser/lexer cases for unterminated `@@doc` triple strings and malformed interpolation near EOF.
- Add more explicit parse-error span tests for multi-line incomplete input.

### sigil

- Add tests for REPL/session-style import mutations after checkpoint rollback.
- Add more stage-boundary tests where one member is visible and another is blocked by stage ordering.

### scar

- Add cases around nested `match` exhaustiveness, function-type mismatch combinations, and generic `Result` annotations in fields/closures.
- Add more tests for diagnostic hints, not only success/failure classification.

### forge

- `forge/src/error.rs` is still effectively untested; add direct constructor/formatting tests.
- Add more chunk-codegen tests for closure capture ordering and partial-application combinations.

### eldr

- Add explicit tests for builtin arity/type mismatches outside the currently covered `safe_mod`/`eprint` paths.
- Add more REPL-chunk rollback cases with mixed function table and error-template updates.

### rune

- Add failure-path tests for `dump --format`, invalid `.eldr` decode, and write failures on `build`.
- Add more `test` command cases for bad selectors and `@@test` expressions that do not evaluate to `Boolean`.

### xldr CLI / REPL

- Add direct tests for `:doc`, `:save`, unknown command reporting, and `.eldr` restore behavior.
- Add fail-fast initialization tests for broken stdlib/bootstrap state.

## Review Findings

### [High] REPL initialization can return a partially bootstrapped engine

- File: `crates/xldr/src/repl/logic/core.rs`
- `ReplEngine::new()` and `ReplEngine::from_eldr()` call `bootstrap_std_modules*()` and then always return `Ok(engine)`.
- Those bootstrap helpers print diagnostics and `return` on parse/resolve/type/codegen/runtime failures instead of surfacing an error to the caller.
- Result: the CLI can start a REPL session even when stdlib bootstrap failed, leaving the engine in a partially initialized state.

### [Medium] Integration support still duplicates the production compile path

- File: `tests/integration/support.rs`
- This helper mirrors loader/parse/resolve/typecheck/codegen logic instead of reusing the `rune` compile path or invoking the CLI.
- The pass in this PR aligns `populate_error_template_lines`, but the helper can still drift from production behavior when compile orchestration changes.

### [Low] REPL command completion can drift from the actual command parser

- Files: `crates/xldr/src/repl/ui/cli.rs`, `crates/xldr/src/repl/logic/command.rs`
- The command parser and completer are maintained separately.
- This pass re-added the missing `:doc` and `:save` completions, but the duplication remains a maintenance trap.
