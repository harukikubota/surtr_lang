# Compare-Only Public Comparison Model Implementation Plan

## Goal

Make `Compare` the only public ordered-comparison capability in Surtr, remove `Ord` / `Lt` / `Lte` / `Gt` / `Gte` and helper functions `lt/lte/gt/gte` from the public surface, and keep `< <= > >=` as syntax derived from `Compare`.

## Public API Changes

- `Compare` remains public and is the only ordered-comparison trait.
- `Ord`, `Lt`, `Lte`, `Gt`, `Gte` are removed from the public stdlib and std-module preload list.
- Public helper functions `lt`, `lte`, `gt`, `gte` and compat aliases `le`, `ge` are removed completely.
- `<`, `<=`, `>`, `>=` remain valid language syntax and require `Compare`.
- `compare(...)` and `Compare::compare(...)` remain the canonical callable/query surface.
- `Eq` / `Neq` are unchanged.

## Implementation Tasks

### 1. Lock the normative contract first

- Rewrite the ordered-comparison contract so only `Compare` is named as the public capability.
- Remove normative mentions of `Ord`, `Lt`, `Lte`, `Gt`, `Gte`, `lt/lte/gt/gte`, and aliases `le/ge`.
- Change the operator rule text to `A < A -> Boolean (where A: Compare)` and the analogous rule for `<=`, `>`, `>=`.
- Remove `Ord`, `Lt`, `Lte`, `Gt`, `Gte` from every fixed std-module load-order list.
- Update REPL/query examples so ordered-comparison call examples use `compare(Int, Int)` or operator symbols.

### 2. Remove the public stdlib surface for the old model

- Delete the trait declaration files for `Ord`, `Lt`, `Lte`, `Gt`, `Gte`.
- Update `compare.srt` and `ordering.srt` docs so they no longer describe compatibility with the removed traits.
- Remove `impl Ord`, `impl Lt`, `impl Lte`, `impl Gt`, `impl Gte` from `Int`, `Float`, and `Duration`.
- Rewrite `Int::compare` and `Float::compare` so they do not depend on removed traits.
- Rewrite `Duration::compare` and `Range<Int>::compare` to be purely `Compare`-based.

### 3. Remove helper-name and old-surface discovery paths

- Remove parser classification that treats `lt/lte/gt/gte` and `le/ge` as comparison helper names.
- Remove compiler special-name recognition for `lt/lte/gt/gte`.
- Remove `Ord`, `Lt`, `Lte`, `Gt`, `Gte` from stdlib preload wiring in runtime/compiler/test harnesses.
- Make REPL help, alias tables, and query examples advertise `compare` and operators only.
- Keep operator symbol inspection user-visible, but back it with `Compare`-centric wording.

### 4. Rebase operator semantics on `Compare`

- Change typechecking for `< <= > >=` so all four operators require `Compare`.
- Emit ordered-operator expressions as `Compare::compare(lhs, rhs)`-based typed calls.
- Add a dedicated typed-call origin for ordered operators so codegen still knows the original operator.
- Keep existing primitive boolean compare opcodes as a private fast path for operator expressions.
- Add private primitive compare backing for explicit `Compare` on `Int` and `Float`, returning `Ordering`.
- Rewrite diagnostics and type-error templates from `where A: Lt/Lte/Gt/Gte` to `where A: Compare`.

### 5. Replace or delete verification that hardcodes the old public API

- Update end-to-end tests to cover `compare(...)`, `Compare::compare(...)`, and symbolic operators only.
- Replace REPL query/doc assertions that expect `gt` / `Gt` with `compare` / `Compare` or operator-symbol output.
- Delete old-helper-only tests instead of replacing them with compatibility checks.
- Update diagnostics tests to assert `Compare`-based wording.

## Verification Plan

- `cargo nextest run -p scar --tests`
- `cargo nextest run -p xldr --tests`
- `cargo nextest run -p diagnostics --tests`
- `cargo nextest run -p rune --test integration run_srt`
- `cargo nextest run --workspace`
