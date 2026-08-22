# Final integration fix report — TypeIdentity OwnerRegistry

## Result

All four Important findings and the Minor documentation finding from
`final-review.md` are fixed. The correction stays in compile-time ownership,
semantic metadata, analysis ordering, and diagnostic provenance; runtime type and
VM semantics are unchanged.

## Corrections

- **I1 — constructor-trait fixed point:** constructor seeds and inheritance edges
  are collected before owner registration. `OwnerRegistry::register` classifies
  trait candidates from the fixed point before constructing a collision diagnostic,
  and registry merges are transactional. Direct and inherited collisions now report
  `TypeConstructor` in both declaration orders.
- **I2 — registry-first semantic identity:** declaration semantic lookup now prefers
  `OwnerRegistry` and uses builtin metadata only as a fallback. Source-owner
  capabilities are derived from the owner kind, and Xldr restores registry-aware
  identity/capabilities after its completion-symbol projection. The shipped
  `Duration` defstruct is covered as `Struct` with type-root capabilities.
- **I3 — canonical source order:** staged modules carry an explicit stage-local
  `source_index`. Owner precollection sorts by source, then source-local span, then
  encounter order; independently parsed analysis files and Xldr module stages assign
  that index explicitly. The regression makes the earlier source start at a later
  local offset and asserts the primary, first, and conflicting spans/labels.
- **I4 — durable diagnostic provenance:** session owner precollection can persist
  source-rebased spans while resolution continues with local AST spans. Xldr keeps an
  immutable source entry per completed REPL chunk, seeds preloaded script owners with
  their source id, decodes owner spans into source-aware labels, and renders
  multi-source diagnostics. Regressions cover prior-chunk/live and preload/live
  collisions and assert text from both sources.
- **M1 — taxonomy wording:** `doc/要件定義v9.md` now states that canonical builtin
  names use the shared metadata identity taxonomy (`Type`, `TypeConstructor`, or
  `Enum`) rather than describing every builtin owner as `Type`.

## TDD evidence

The focused regressions were introduced before the corresponding production fixes:

- Direct and inherited constructor-trait collision-label tests initially reported
  provisional `Trait`; both orders now pass with final identities.
- The cross-file ordering test initially selected the later file at local offset
  zero as the first owner; it now preserves file order.
- The `Duration` regressions initially observed `Type` and then missing type-root
  capabilities; both integration and snapshot paths now report the registry's
  `Struct` metadata.
- The REPL/preload rendered-source tests initially required source-aware session and
  diagnostic APIs that did not exist; both now render the stored first source and the
  live conflicting source.

## Verification

- `cargo nextest run -p sigil`: **223 passed**.
- `cargo nextest run -p surtr-analysis`: **132 passed**.
- `cargo nextest run -p diagnostics`: **70 passed**.
- `cargo nextest run -p xldr --status-level fail --final-status-level fail`:
  **200 passed, 74 skipped**.
- Focused Xldr owner-rendering tests: **2 passed**.
- Focused `Duration` unit/integration regressions: **passed**.
- `cargo check --workspace`: **passed**.
- `cargo fmt --check`: **passed**.
- `cargo nextest run --workspace --status-level fail --final-status-level fail`:
  **1,658 passed, 202 skipped** in 134.054 seconds.
- `git diff --check`: **passed**.

An earlier cold Xldr package run hit the existing 15-second timeout in
`core_reload_and_clear_commands_preserve_only_requested_state`; the exact test then
passed under `cargo test` in 3.93 seconds, the repeated Xldr package suite passed, and
the final full-workspace gate passed.

## Concerns

No unresolved correctness concern remains from the review. The nextest timeout noted
above appears transient because the exact rerun, package rerun, and final workspace
run are all green.
