# Scar / stdlib optimization handoff

## Summary

The run cache added for `surtr run <file.srt>` improves repeated script runs, but first-run latency is still dominated by standard-library compilation. The next optimization target should be the Scar / stdlib snapshot path, not VM execution.

Local measurements from `examples/guess.srt`:

- `target/debug/surtr run examples/guess.srt`: about `1.1s` before cache, about `1.76s` on a cold cache in one later run
- cached second run: about `0.10s`
- direct `.eldr` run: about `0.03s`
- `SURTR_SCAR_PROFILE=1 target/debug/surtr check examples/guess.srt`: stdlib typecheck about `740ms`, user program typecheck about `12ms`
- `SURTR_SCAR_PROFILE=1 target/release/surtr check examples/guess.srt`: stdlib typecheck about `330ms`, user program typecheck about `6ms`

## Current bottleneck

`xldr::default_stdlib_semantic_snapshot()` builds the standard-library semantic baseline on process startup. The expensive section is Scar checking the stdlib statements, especially the `check_stmt_loop`.

Observed Scar profile for debug stdlib:

- total: about `740ms`
- `check_stmt_loop`: about `711ms`
- statement count: `485`
- slow examples: `TraitImplDef Ord`, `_fg_code`, `_bg_code`, `width_bits`, `_single_ascii_codepoint`, `_max_by_go`, `_min_by_go`

The user script itself is not the main issue. For `guess.srt`, user-side typecheck was about `12ms` in debug.

## Code entry points

- `crates/xldr/src/lib.rs`
  - `default_stdlib_semantic_snapshot`
  - `build_default_stdlib_snapshot`
- `crates/scar/src/checker/mod.rs`
  - `ScarSession::typecheck_with_context`
  - `Checker::check_program`
  - existing `SURTR_SCAR_PROFILE` instrumentation
- `crates/rune/src/compile.rs`
  - `parse_program_with_module_sources`
  - `compile_source`
  - both currently rely on the stdlib snapshot

## Recommended next steps

1. Keep the run cache as the repeated-run fast path, but optimize first-run latency separately.
2. Add a focused benchmark or timing test around `xldr::default_stdlib_semantic_snapshot()` so stdlib-only changes can be measured without VM execution noise.
3. Investigate why stdlib `Def` and `TraitImplDef` checking dominates. Start from the slow statements shown by `SURTR_SCAR_PROFILE=1`.
4. Check whether Scar repeatedly clones or re-walks large type environments during stdlib checking. `ScarSession::typecheck_with_context` currently clones session state into `Checker`.
5. Consider a lower-risk serialized stdlib snapshot only after understanding the Scar cost. Persisting `ScarCheckpoint` would require adding serialization to Scar/Sigil internal types, so it is larger and riskier than local Scar improvements.

## Verification baseline

Useful commands:

```bash
SURTR_SCAR_PROFILE=1 target/debug/surtr check examples/guess.srt
SURTR_SCAR_PROFILE=1 target/release/surtr check examples/guess.srt
cargo nextest run -p rune
```

Acceptance target for the next task:

- reduce first-run `target/debug/surtr run examples/guess.srt` meaningfully without relying on the run cache
- keep `cargo nextest run -p rune` green
- preserve the cached second-run behavior from this change
