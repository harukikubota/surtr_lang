# Pattern Wildcard Bindings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure underscore-prefixed pattern names discard values and reject wildcard aliases in as-patterns.

**Architecture:** The Spire parser will canonicalize underscore-prefixed binding atoms into `AstPattern::Wildcard` and validate as-pattern aliases before AST construction. Downstream phases already recognize wildcard variants, so focused resolver and Xldr tests verify that no session binding can reappear.

**Tech Stack:** Rust workspace; Spire parser; Sigil resolver; Xldr REPL tests; cargo-nextest.

## Global Constraints

- Keep diagnostics compliant with `docs/dev/diagnostics.md`: headline names the cause; source span points to the alias; help gives replacement syntax.
- Preserve valid named as-pattern syntax and its existing type annotation support.
- Run focused tests before the workspace suite.

---

### Task 1: Define parser behavior and diagnostic coverage

**Files:**
- Modify: `crates/spire/src/parser/tests.rs`
- Modify: `crates/spire/src/parser/pattern.rs`

**Interfaces:**
- Consumes: `Token::Ident(String)` and `AstPattern`.
- Produces: `AstPattern::Wildcard` for any binding atom whose spelling begins with `_`; `ParseError` for underscore-prefixed as aliases.

- [ ] **Step 1: Write the failing tests**

```rust
let ast = parse("(_ignored, value) = (1, 2)").unwrap();
assert!(matches!(/* first item */, AstPattern::Wildcard(_)));

let err = parse("(left, right) @ _ = (1, 2)").unwrap_err();
assert!(err.message.contains("as-pattern alias must be a binding identifier"));
assert!(err.message.contains("@ value"));
```

- [ ] **Step 2: Run the focused test to verify it fails**

Run: `cargo nextest run -p spire --lib parser::tests::test_underscore_prefixed_pattern_is_wildcard parser::tests::test_as_pattern_rejects_wildcard_alias`

Expected: FAIL because `_ignored` is still `AstPattern::Var` and `@ _` is accepted.

- [ ] **Step 3: Implement the minimal parser change**

```rust
if name.starts_with('_') {
    return Ok(AstPattern::Wildcard(sp));
}

if alias.starts_with('_') {
    return Err(ParseError::syntax(
        "as-pattern alias must be a binding identifier; use `pattern @ name`, not `pattern @ _`",
        alias_span,
    ));
}
```

- [ ] **Step 4: Run the focused test to verify it passes**

Run: `cargo nextest run -p spire --lib parser::tests::test_underscore_prefixed_pattern_is_wildcard parser::tests::test_as_pattern_rejects_wildcard_alias`

Expected: PASS.

### Task 2: Prove wildcard values cannot be resolved or retained by REPL

**Files:**
- Modify: `crates/sigil/src/resolver/tests.rs`
- Modify: `crates/xldr/tests/repl_core.rs`

**Interfaces:**
- Consumes: parsed wildcard patterns from Spire.
- Produces: no resolver scope entry or REPL visible binding for wildcard names.

- [ ] **Step 1: Write the failing tests**

```rust
let err = parse_and_resolve("_ignored = 1\n_ignored").unwrap_err();
assert!(err.message.contains("Undefined variable: _ignored"));

let result = engine.submit_line("_ignored = 1");
assert!(result.rendered_lines().iter().all(|line| !line.contains("_ignored:")));
let lookup = engine.submit_line("_ignored");
assert!(lookup.diagnostic_text().contains("Undefined variable: _ignored"));
```

- [ ] **Step 2: Run focused tests to verify they fail**

Run: `cargo nextest run -p sigil --lib resolver::tests::test_underscore_prefixed_pattern_does_not_bind && cargo nextest run -p xldr --test repl_core underscore_prefixed_pattern`

Expected: FAIL because `_ignored` is currently a normal binding.

- [ ] **Step 3: Verify downstream behavior with the parser change**

No production change is expected outside Spire: `ResolvedPattern::Wildcard`, `TypedPattern::Wildcard`, Forge slot allocation, and Xldr binding collection already omit wildcard patterns.

- [ ] **Step 4: Run focused tests to verify they pass**

Run: `cargo nextest run -p sigil --lib resolver::tests::test_underscore_prefixed_pattern_does_not_bind && cargo nextest run -p xldr --test repl_core underscore_prefixed_pattern`

Expected: PASS.

### Task 3: Update language documentation and verify the workspace

**Files:**
- Modify: `doc/要件定義v9.md`

- [ ] **Step 1: Update the pattern specification**

Add explicit rules that `_` and underscore-prefixed names are wildcard patterns, never bind, and cannot appear after `@`.

- [ ] **Step 2: Run verification**

Run: `cargo nextest run -p spire --lib && cargo nextest run -p sigil --lib && cargo nextest run -p xldr --test repl_core && cargo nextest run --workspace`

Expected: all selected tests and the workspace pass.
