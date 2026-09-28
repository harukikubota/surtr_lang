# Task 1 Report: Type Constructor Signature Unification

## Completed migration

Task 1 replaces the legacy `FunParams` / `FunParam` vocabulary across the
Spire, Sigil, Scar, Forge, Xldr, documentation, and focused test surfaces.

- Explicit `::<...>` declaration and call-site slots are represented as
  `ReturnTypeArgument` / `ResolvedReturnTypeArgument` /
  `TypedReturnTypeArgument`.
- Runtime declaration parameters are represented as `ValueParameter` /
  `ResolvedValueParameter` / `TypedValueParameter`, including their mode and
  source span.
- The AST and resolved explicit-application variants are now
  `ReturnTypeArgumentApply`.
- No compatibility aliases for the previous vocabulary were retained.

## Regression found and corrected

The existing `callable_alias_and_partial_tuple_expected.srt` integration
fixture initially failed at runtime with:

```text
Call arity mismatch for function 500: expected 8, got 1
```

The migration makes explicit declaration `::<...>` slots participate in Scar's
specialization set.  Static trait-call specialization previously inferred that
set only from runtime value arguments.  For receiverless trait methods such as
`Applicative::pure::<Option<$T>>(value: $A)`, the return-type argument is
instead determined by the trait receiver (`Option<Int>`).  The incomplete
mapping left the original function index in the typed trait call after the
unspecialized definition had been omitted, allowing an unrelated generated
closure to occupy that index.

`crates/scar/src/checker/specialize.rs` now augments the mapping from the
declaration's return-type arguments: it matches them against the trait receiver
for parameterless traits, or against the trait arguments for parameterized
traits.  This produces the required specialized function index and preserves
the fixture without changing its expectations or adding a workaround.

The incremental/cache path also now preserves the materialized bytecode
function-index floor before Scar reconciles delayed specializations. This
prevents a cached/generated function index from colliding with a later
specialization; the invariant is covered by a Scar unit test and the cache
integration path.

## Diff review

Reviewed the complete Task 1 diff, including the canonical migration, trait
specialization correction, and incremental/cache function-index floor fix.
The propagation is consistent through parser, resolver, type checker, code
generator, REPL pattern matches, docs, and focused tests. During review,
migration-local identifiers and test names that still spelled the old role
imprecisely were aligned with `ReturnTypeArgument` and `ValueParameter`; these
are naming-only changes.

## Verification

- `cargo fmt --check`
- `cargo nextest run -p spire` — 403 passed
- `cargo nextest run -p sigil` — 230 passed
- `cargo nextest run -p scar` — 164 passed
- `cargo nextest run -p forge` — 81 passed, including
  `format_function_signature_preserves_generic_surface_names`
- `cargo nextest run -p xldr` — 201 passed
- `cargo nextest run -p rune --test integration run_srt --no-capture` — 27
  passed (170 skipped by the test filter), including
  `callable_alias_and_partial_tuple_expected.srt`
- `cargo nextest run --workspace` — 1734 passed, 202 skipped
- `rg -n 'FunParams|fun_params|fun_param|FunParam' crates docs/dev docs/site
  doc/要件定義v9.md` — no matches
- A broader case-insensitive scan for `funparams`, `fun_param`, and `funparam`
  also returned no matches.
- `git diff --check`

## Concerns

None.
