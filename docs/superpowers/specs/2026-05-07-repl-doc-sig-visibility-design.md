# REPL `@doc` / `:doc` / `:sig` Visibility Design

Date: 2026-05-07
Status: Draft for review

## Goal

Align documentation annotations and REPL lookup behavior with Surtr's public/private function boundary.

This design fixes the current mismatch where:

- private functions can still carry `@doc`
- REPL `:doc` already tends to hide private functions
- REPL `:sig` behavior is broader than the intended public surface

The desired end state is:

- `@doc` is only allowed on public declarations
- REPL evaluation is limited to callable public functions
- `:doc` is limited to public declarations that actually registered `@doc`
- `:sig` is limited to public declarations, even when the queried symbol is not callable from the REPL runtime

## Non-Goals

- Redesigning REPL binding inspection such as `:type $name`
- Changing the standard-library strategy of defining broad surface declarations for REPL help
- Reworking hidden builtin rules unless required as a narrow compatibility fix
- Expanding private helper visibility for tooling

## User-Facing Rules

### `@doc`

- `@doc` may annotate public declarations only.
- `@doc` on `defp` is a compile error.
- `@doc` on process helper functions that become private because they have no process annotation is a compile error.
- Existing restrictions remain unchanged:
  - `@doc` may appear only once before a declaration
  - `@doc` requires a triple-quoted doc string
  - `@doc` does not allow interpolation

### REPL evaluation

- REPL runtime evaluation may call only public callable functions.
- This rule is unchanged in spirit, but should remain explicitly separate from `:doc` and `:sig` lookup rules.

### `:doc`

- `:doc` resolves only public declarations.
- A result is shown only when a matching declaration has an `@doc` entry registered in the doc table.
- If a public declaration exists but has no `@doc`, the current "undocumented" guidance remains valid.
- If the queried symbol resolves to a private declaration, `:doc` should not treat it as merely undocumented. It should report that the symbol is private and outside `:doc` visibility.

Recommended message shape:

```text
`MyMod::helper` is private and cannot be queried with `:doc`.
Add `@doc` only to public declarations.
```

This keeps the distinction clear between:

- public but undocumented
- private and intentionally hidden
- not found

### `:sig`

- `:sig` resolves only public declarations.
- `:sig` may still show signatures for public declarations that are not directly executable from the current REPL runtime path.
- `:sig` should not expose private declarations.
- For private declarations, return the same style of explicit private-surface guidance as `:doc`, rather than falling through to a generic "No signature found" when we can positively identify a private target.

Recommended message shape:

```text
`MyMod::helper` is private and cannot be queried with `:sig`.
Only public declarations are visible to REPL signature lookup.
```

## Implementation Strategy

### 1. Enforce `@doc` visibility at validation time

Preferred behavior is to reject invalid `@doc` usage before doc collection.

Implementation intent:

- after a declaration's effective visibility is known, validate `attrs.doc`
- if `attrs.doc.is_some()` and the declaration is private, emit a compile error

This must cover:

- top-level `defp`
- impl member `defp`
- lowered process helper defs whose visibility becomes private during parser lowering

The key point is that validation must happen after the effective visibility is final, not only at raw annotation parse time.

### 2. Separate REPL query predicates by command purpose

The current REPL logic shares a broad "queryable" concept across commands. This design replaces that with narrower predicates.

Suggested conceptual split:

- `is_doc_visible(entry)`: public only
- `is_sig_visible(entry)`: public only
- `is_runtime_callable(entry)`: public and user-callable

This avoids overloading one predicate for three different concerns:

- name lookup for docs
- name lookup for signatures
- runtime execution eligibility

### 3. Distinguish private hits from missing hits

For `:doc` and `:sig`, the REPL should first determine whether:

1. a public declaration matches
2. a private declaration matches
3. no declaration matches

That allows better output:

- public + doc entry => show docs
- public + no doc entry => undocumented guidance
- private => private-surface guidance
- none => missing symbol guidance

This requires a lookup path that can detect private declarations without making them generally visible.

### 4. Keep doc storage simple

Doc collection may remain structurally unchanged as long as invalid private `@doc` never survives validation.

That means we do not need to encode visibility into `DocEntry` just to support this rule set.

## Affected Areas

- `crates/spire/src/parser/decl.rs`
  - declaration annotation parsing and process helper lowering
- `crates/xldr/src/lib.rs`
  - doc entry collection assumptions
- `crates/xldr/src/repl/logic/core.rs`
  - `:doc` / `:sig` lookup and messaging
- `crates/xldr/tests/repl_core.rs`
  - REPL visibility behavior coverage
- `tests/compile_errors/**`
  - new invalid `@doc` cases
- possibly `doc/要件定義v9.md`
  - if the canonical wording should explicitly mention that private declarations cannot carry `@doc`

## Error Handling

Compile-time invalid `@doc` should be reported in the declaration phase that owns the rule.

Recommended error wording:

```text
`@doc` is only allowed on public declarations
```

For REPL commands:

- do not reuse the undocumented path for private declarations
- do not silently degrade private declarations into not-found if they can be recognized

## Testing Plan

### Compile error tests

Add coverage for:

- `@doc` before top-level `defp`
- `@doc` before private impl helper
- `@doc` before process helper with no handler annotation

Each test should assert the relevant phase and a stable message fragment.

### REPL tests

Add or update coverage for:

- `:doc` returns docs for public documented functions
- `:doc` reports undocumented for public undocumented functions
- `:doc` reports private guidance for private functions
- `:sig` returns signatures for public functions
- `:sig` reports private guidance for private functions
- `:sig` still works for public non-runtime-callable declarations when that behavior is intended

### Regression checks

Retain coverage for:

- special forms with docs
- standard definition source docs
- hidden builtin exceptions that are intentionally supported

## Open Questions

1. Should `:sig` private guidance be explicit, or should it remain generic to avoid confirming symbol existence?

Current recommendation:
Use explicit private guidance because the same thread already chose public-only visibility as the surface rule, and explicit feedback is easier to understand while debugging declarations.

2. Do we want the canonical spec text to say "`@doc` is only allowed on public declarations" in one sentence, or enumerate private cases like `defp` and process helpers separately?

Current recommendation:
State the general rule once and let examples cover `defp` and process helpers.

## Recommendation

Implement option 1:

- reject `@doc` on private declarations
- make `:doc` public-only plus doc-entry existence
- make `:sig` public-only
- keep runtime callability as a separate, stricter rule

This produces one clear model:

- public surface is discoverable
- private surface is not discoverable through REPL docs/signatures
- documentation annotations belong only to the public surface
