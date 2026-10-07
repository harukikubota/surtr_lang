# Constrained Builtins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make all non-`@intrinsic` builtins preserve and enforce required trait constraints through the normal Scar obligation-checking route.

**Architecture:** Extend the source declaration pipeline so constrained `@builtin def` declarations carry a normal `where` clause from Spire through Sigil into Scar. Keep runtime builtin dispatch and IDs unchanged, while storing the builtin's typed constraint metadata alongside its callable declaration and routing call-site validation through the existing generic obligation machinery. Audit the standard library and add only the bounds required by actual builtin semantics; `@intrinsic` remains excluded.

**Tech Stack:** Rust workspace, Spire parser, Sigil resolver, Scar type checker, Rune fixture tests, Surtr `.srt` standard definitions.

## Global Constraints

- `@intrinsic` declarations remain outside trait-constraint checking.
- Constrained builtin declarations must use the normal source-level `where` contract.
- Builtin runtime names, builtin IDs, and Eldr runtime dispatch must not change.
- A missing trait implementation must produce a `typecheck` failure, not a runtime fallback.
- `cargo nextest run --workspace` is the required final baseline.

---

### Task 1: Add failing parser and pipeline tests

**Files:**
- Modify: `crates/spire/src/parser/tests.rs`
- Modify: `crates/sigil/src` tests covering resolved declarations
- Modify: `crates/scar/src` tests covering builtin calls and obligations
- Create: `tests/fixtures/script/fail/typecheck/builtin_group_count_requires_eq.srt`
- Create: `tests/fixtures/script/fail/typecheck/builtin_group_count_requires_eq.error`
- Create: `tests/fixtures/script/fail/typecheck/builtin_dedup_requires_eq.srt`
- Create: `tests/fixtures/script/fail/typecheck/builtin_dedup_requires_eq.error`

**Interfaces:**
- The parser test will use `@builtin def constrained(values: List<$A>) -> List<$A> where $A: Eq` and assert the clause is retained.
- The compile-fail fixtures will define a custom struct without `Eq` and call `List::group_count` and `List::dedup`.
- Existing `Int` success coverage remains green.

- [ ] Write the parser test that expects a builtin where clause to be preserved.
- [ ] Add the two compile-fail fixtures and expected `phase = typecheck` diagnostics.
- [ ] Run the focused parser/integration tests and confirm they fail because builtin where clauses are currently rejected or ignored.

### Task 2: Preserve builtin where clauses through Spire and Sigil

**Files:**
- Modify: `crates/spire/src/ast.rs`
- Modify: `crates/spire/src/parser/decl.rs`
- Modify: `crates/sigil/src/resolved.rs`
- Modify: `crates/sigil/src/resolver/expr.rs`
- Modify: `crates/sigil/src/resolver/mod.rs`
- Modify: declaration pattern matches and serialization tests that construct `Ast::BuiltinDecl` or `Resolved::BuiltinDecl`

**Interfaces:**
- `Ast::BuiltinDecl` and `Resolved::BuiltinDecl` gain `Option<WhereClause>` / `Option<ResolvedWhereClause>` in the same position used by normal function declarations.
- The parser accepts an optional function-style where clause for `@builtin def` and continues rejecting bodies.
- `@intrinsic` parsing and resolution remain unchanged.

- [ ] Change the AST and resolved declaration shapes.
- [ ] Replace the builtin parser's unconditional `where` rejection with normal optional where-clause parsing.
- [ ] Resolve and rebase builtin where clauses using the existing resolver helpers.
- [ ] Update every exhaustive match and fixture constructor.
- [ ] Run Spire/Sigil unit tests and confirm the Task 1 parser test passes.

### Task 3: Register constrained builtin contracts in Scar

**Files:**
- Modify: `crates/scar/src/checker/predeclare.rs`
- Modify: `crates/scar/src/checker/definitions.rs`
- Modify: `crates/scar/src/checker/mod.rs`
- Modify: `crates/scar/src/typed.rs` only if the typed declaration representation requires a new builtin contract field
- Modify: `crates/scar/src/checker/specialize.rs` if specialization must carry builtin where metadata

**Interfaces:**
- Builtin predeclaration seeds signature type variables, resolves parameter/return types, applies the resolved where clause with the same helper used by `Resolved::Def`, and records the callable's typed obligations by declaration identity.
- The existing `Ty::BuiltinFunc` runtime shape remains compatible with Forge; constraint metadata is kept in checker callable metadata rather than encoded into runtime values.
- The metadata lookup is keyed by the builtin declaration identity/unique ID so same-named qualified builtins cannot share constraints accidentally.

- [ ] Add the constrained builtin metadata storage and checkpoint/clone propagation wherever checker state is copied.
- [ ] Make builtin predeclaration process type parameters and where clauses like normal generic definitions.
- [ ] Ensure builtin declaration checking validates the source signature against canonical builtin metadata without dropping its where clause.
- [ ] Add a unit test proving the obligation is present for a constrained builtin and absent for an unconstrained builtin.
- [ ] Run Scar unit tests and inspect the typed/checker state for the expected obligation.

### Task 4: Route builtin calls through ordinary obligation checking

**Files:**
- Modify: `crates/scar/src/checker/expr.rs`
- Modify: `crates/scar/src/checker/types.rs` or `crates/scar/src/checker/predeclare.rs` for shared obligation helpers if required
- Modify: `crates/scar/src/checker/specialize.rs` if pending builtin obligations need concretization support

**Interfaces:**
- The `Ty::BuiltinFunc` branch in `check_app` continues to perform builtin-specific argument and special policy checks, but invokes the same callable-obligation validation used by constrained user functions.
- Concrete `User` arguments fail when `Eq` is absent; concrete `Int` arguments succeed.
- Generic callers retain pending obligations and are rechecked when their type variable is concretized, rather than silently accepting an unconstrained call.

- [ ] Write the smallest Scar-level failing test for a constrained builtin call with a non-`Eq` concrete type.
- [ ] Implement the shared call-site obligation check with one obligation per constrained builtin call.
- [ ] Preserve existing special handling for `set_exit_code`, facets, lazy arguments, and builtin runtime calls.
- [ ] Add a generic caller test proving the obligation is propagated to the caller boundary.
- [ ] Run focused Scar tests and verify both failure and success cases.

### Task 5: Audit and update standard builtin declarations

**Files:**
- Modify: `lib/types/list.srt`
- Modify: any other `lib/**/*.srt` builtin declaration identified by the semantic audit
- Modify: corresponding standard-library tests/fixtures

**Interfaces:**
- `List::group_count` declares `$A: Eq`.
- `List::dedup` declares `$A: Eq` because it delegates to `group_count`.
- Any additional builtin receives a bound only when its runtime semantics require a Surtr trait; `@intrinsic` is excluded.

- [ ] Generate an inventory of all non-intrinsic builtin declarations and classify each as concrete-only, unconstrained generic, or trait-constrained.
- [ ] Add source `where` clauses for every trait-constrained declaration.
- [ ] Add caller bounds to ordinary standard helpers that delegate to constrained builtins.
- [ ] Run standard library compilation and success fixtures.

### Task 6: Add integration coverage and verify the full workspace

**Files:**
- Modify: `tests/fixtures/script/pass/**` only where a positive constrained-builtin case is missing
- Modify: `tests/fixtures/script/fail/typecheck/**` for final diagnostic coverage
- Modify: `docs/dev/テスト方針.md` only if the established fixture rule needs an explicit builtin-constraint entry

- [ ] Verify `group_count([1, 1])` and `dedup([1, 1])` still pass.
- [ ] Verify a custom type without `Eq` fails for both APIs with `phase = typecheck`.
- [ ] Verify `@intrinsic` declarations remain unaffected.
- [ ] Run `cargo nextest run -p spire`, `cargo nextest run -p sigil`, and focused Scar/Rune tests.
- [ ] Run `cargo nextest run -p rune --test integration run_srt` and the module fixture suite.
- [ ] Run `cargo nextest run --workspace`.
- [ ] Run `git diff --check` and review the final diff for unintended builtin ID/runtime changes.
