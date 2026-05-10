# `_.path` Inferred Facet Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `_.path` as language-level syntax for inferred unary field/facet capture.

**Architecture:** Sigil recognizes field-access chains rooted at `_` before normal variable lookup and represents them as inferred captures. Scar typechecks them only under an expected unary function type, then lowers them through the existing closure/field-access path so Facet privacy, tuple, record, struct, and variant rules stay centralized.

**Tech Stack:** Rust compiler crates (`spire`, `sigil`, `scar`) and Surtr spec fixtures.

---

### Task 1: Pin the language behavior

**Files:**
- Create: `tests/spec/functions/inferred_facet_capture.srt`
- Create: `tests/spec/functions/inferred_facet_capture.expected`
- Create: `tests/compile_errors/functions/inferred_facet_capture_standalone.srt`
- Create: `tests/compile_errors/functions/inferred_facet_capture_standalone.error`
- Create: `tests/compile_errors/functions/inferred_facet_capture_unknown_field.srt`
- Create: `tests/compile_errors/functions/inferred_facet_capture_unknown_field.error`

- [x] Add success coverage for `users |*> _.name`, nested `_.profile.name`, tuple `pairs |*> _._0`, explicit `&User.age`, and inline `Function::on`.
- [x] Add compile-error coverage for standalone `_.name` and inferred unknown field access.
- [x] Verify red: before implementation, fixtures fail in resolve/typecheck because `_` is not yet an inferred capture.

### Task 2: Resolve `_.path` as an inferred capture

**Files:**
- Modify: `crates/sigil/src/resolved.rs`
- Modify: `crates/sigil/src/resolver/expr.rs`
- Modify: `crates/sigil/src/resolver/mod.rs`
- Modify: `crates/sigil/src/resolver/captures.rs`
- Modify: `crates/scar/src/checker/predeclare.rs`

- [x] Add `Resolved::InferredFacetCapture(Span, Vec<Symbol>)`.
- [x] In Sigil expression resolution, detect `Ast::FieldAccess` chains rooted at `Ast::Var("_")` and emit the inferred capture node before resolving `_`.
- [x] Treat inferred captures as capture-free and non-const-surface values in support walkers.

### Task 3: Typecheck inferred captures from expected unary function context

**Files:**
- Modify: `crates/scar/src/checker/expr.rs`

- [x] Reject bare `_.path` with `_.path requires expected unary function context`.
- [x] In `check_node_with_expected`, accept inferred capture only for `Ty::Func([source], ret)`.
- [x] Lower to a synthetic closure whose parameter has the expected source type and whose body is the normal field-access chain.
- [x] Reuse existing field/Facet resolution by typechecking that synthetic closure instead of duplicating field policy.

### Task 4: Feed expected types from operators

**Files:**
- Modify: `crates/scar/src/checker/expr.rs`
- Modify: `crates/spire/src/parser/expr.rs`

- [x] In `|*>`, typecheck the LHS first when it reveals `Result<A>`, `List<A>`, or `Option<A>`, then check RHS against `(A -> _)`.
- [x] In expected `Function::on` calls, check the key function first against `(source -> _)`, then check the comparator against the inferred key type.
- [x] Allow explicit captures like `&User.age` by parsing post-capture field chains into the capture target and allowing inline Facet path captures, not only bound Facet variables.

### Task 5: Document and verify

**Files:**
- Modify: `docs/site/capture-operator.md`
- Modify: `docs/site/function-operators.md`
- Modify: `docs/site/facet.md`

- [x] Document `_.path` as inferred field/facet capture.
- [x] Document `&Type.path` as explicit canonical capture.
- [x] Run focused fixture buckets and phase tests.
