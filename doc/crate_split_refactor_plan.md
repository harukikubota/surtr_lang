# Crate Split Refactor Plan

## Summary

This document is the implementation anchor for Surtr's large-file split and dependency cleanup work.
The refactor keeps the current pipeline boundaries intact:

- `spire -> sigil -> scar -> forge -> eldr`
- `xldr` and `rune` remain orchestration layers
- `sindr` and `diagnostics` remain shared support crates

The default rule is internal modularization plus crate-root re-exports, so downstream crates keep the same public API while implementation files become smaller and easier to own per crate.

## Goals And Non-Goals

Goals:

- Split oversized files along behavioral seams rather than raw line count.
- Make crate-local ownership clearer so work can proceed crate-by-crate.
- Reduce test-helper coupling and keep fixture execution behavior stable.
- Keep behavior identical unless a focused follow-up task explicitly changes semantics.

Non-goals:

- No pipeline boundary change.
- No new workspace test-support crate in this pass.
- No split of `crates/sindr/src/builtin.rs` in this pass.
- No mass assertion rewrites while reorganizing tests.

## Workspace Dependency Rules

- Preserve the current crate dependency direction.
- Prefer crate-internal modules plus `pub use` re-exports over moving types across crates.
- Keep phase-specific error ownership unchanged:
  - parser: `ParseError`
  - resolver: `ResolveError`
  - typechecker: `TypeError`
  - codegen: `CodegenError`
  - runtime: `RuntimeError`
- Keep test helpers inside the existing integration test target unless a later performance task justifies a dedicated crate.
- When a file is large but semantically canonical, keep it unified and split only the adjacent helpers.

## Priority Order

1. `diagnostics`
2. `sindr`
3. `tests/integration/support.rs`
4. `xldr`
5. `rune`
6. `spire`
7. `sigil`
8. `scar`
9. `forge`
10. `eldr`

This ordering front-loads shared infrastructure and test support so later crate work has smaller diffs and better helper boundaries.

## Crate-By-Crate Tasks

### `diagnostics`

- Split `heuristics.rs` by concern:
  - span and line helpers
  - operator/type mismatch heuristics
  - runtime heuristics
  - trait and extractor heuristics
- Split `tests.rs` into thematic modules that mirror the helper layout.
- Keep `render.rs`, `parse.rs`, `resolve.rs`, `runtime.rs`, and `typecheck.rs` as entry modules with narrow imports.

### `sindr`

- Split `ir.rs` into:
  - opcode definitions
  - bytecode structures and format types
  - `.eldr` encode/decode
  - derived metadata helpers
- Split `runtime.rs` into:
  - runtime values and handles
  - collection helpers
  - rich error formatting and display
- Keep `builtin.rs` unified.
- Keep `viewer.rs` unified unless opcode-view growth forces an extraction.
- Review `serde_json` reachability and keep any narrowing behavior-neutral.

### `spire`

- Split `parser/tests.rs` into feature-family modules:
  - declarations
  - expressions
  - patterns
  - strings and docs
  - modules and imports
  - control flow
- Delay `decl.rs` and `expr.rs` logic splits until test extraction is complete.
- Keep `lexer.rs` unified unless token families grow further.

### `sigil`

- Split `resolver/tests.rs` into:
  - staged modules
  - imports
  - traits and impls
  - special forms
  - captures and pipe-slot lowering
  - pattern resolution
- Split `resolver/expr.rs` into call resolution, special-form lowering, capture lowering, and match-pattern helpers.
- Split `resolver/declarations.rs` into index construction, trait/impl registration, and duplicate/conflict validation.

### `scar`

- Split `typecheck_surface_tests.rs` into:
  - lens and privacy
  - special forms
  - traits and operators
  - generics and forward references
  - pattern and match coverage
- Split `checker/expr.rs` first, then follow with `types.rs`, `definitions.rs`, `predeclare.rs`, and `specialize.rs`.
- Keep `checker/mod.rs` as orchestration plus re-exports.

### `forge`

- Split `codegen.rs` into:
  - session and checkpoint state
  - chunk composition and relocation
  - metadata and doc collection
  - opcode emission
- Move bootstrap-heavy tests out of `src/lib.rs` into dedicated test modules.
- Preserve the current public surface of `opcode.rs`, `registry.rs`, and `bytecode.rs`.

### `eldr`

- Split `builtin.rs` by builtin family:
  - formatting and inspection
  - arithmetic and bit operations
  - collections and maps
  - regex
  - IO and random
  - test-only helpers
- Split `vm.rs` into:
  - execution loop
  - push/checkpoint handling
  - observation and stats
  - runtime error helpers
  - tests
- Keep `error.rs` and `value.rs` small and public-facing.

### `xldr`

- Split `lib.rs` into:
  - doc collection and signature formatting
  - module-stage parsing and lowering
  - runtime-policy helpers
  - stdlib snapshot/cache logic
- Split `repl/logic/core.rs` into bootstrap/load flow, command execution, doc/symbol queries, rollback logic, and parser helpers.
- Keep UI concerns out of REPL core.
- Revisit `loader.rs` only after `lib.rs` is smaller.

### `rune`

- Split `compile.rs` into include/module discovery, diagnostic/source mapping, entrypoint resolution, stdlib snapshot loading, and compile-plan assembly.
- Split `commands/test.rs` into CLI option parsing, suite discovery, formatting, and cache/reuse helpers.
- Keep `commands/run.rs` unified unless adjacent behavior grows enough to justify another pass.

### `tests/`

- Split `tests/integration/support.rs` into:
  - fixture and source loading
  - compile cache
  - phase checks
  - compile wrappers
  - run helpers
- Keep `tests/integration/all.rs` as the single entrypoint.
- Keep `run_srt.rs` mostly unchanged because it is fixture-driven rather than structurally mixed.
- Keep `run_eldr.rs` unified for now, but organize follow-up work by cache, diagnostics, include handling, and entrypoint behavior.
- Preserve the bucketed `language_features` runner while splitting oversized underlying feature files by topic.

## Test Migration And Acceptance Checks

Per crate pass:

1. Run the crate-local unit tests first.
2. Run the workspace gate:
   - `cargo nextest run --workspace`
   - `cargo nextest run -p rune --test run_srt`
3. Add tests only when the refactor creates a new seam that was previously untested.

Recommended seam-preservation checks:

- re-export smoke tests for moved public items
- `sindr::ir` encode/decode parity checks
- `eldr` builtin dispatch parity checks
- `xldr` and `rune` cache and stdlib snapshot coverage
- resolver/typechecker feature-family modules that verify behavior did not drift during extraction

## Current Implementation Status

Completed in this pass:

- Added this master doc as the shared implementation guide.
- Split `tests/integration/support.rs` into smaller modules while preserving the existing helper surface.

Next implementation targets:

1. `diagnostics` internal module split
2. `sindr` internal module split
3. `xldr` and `rune` orchestration file splits
