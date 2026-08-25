# Task 6 Report — Standard definitions, fixtures, and tooling consumers

## Status

Consumer migration and final verification are complete. All focused suites and both required workspace-wide test runs pass.

## Changes

### Standard definition

- `lib/types/json.srt`: changed the illegal declaration-level bound `$T: Encode<JsonValue>` to the bare capability `$T: Encode`.
- Preserved the full expression obligation `Encode::encode::<JsonValue>(value)`.
- Audited the listed operator/type definitions. Valid trait and impl heads, call-site trait arguments, `Functor.$A` slot maps, and `Self<$...>` contextual target signatures were retained.

### Fixture matrix

- Added parser compile-fail fixtures for parameterized `where` RHS at four declaration boundaries: function, trait, trait method, and impl.
- Added `bare_where_full_obligation.srt`, proving `$A: TryFrom` can discharge the explicit `TryFrom::try_from::<Int>` obligation.
- Rewrote the exact and mismatched `Marker` fixtures from parameterized declaration bounds to a bare `Marker` capability plus explicit `Marker::mark::<Int>` dispatch. The mismatch now asserts the full specialization error.
- Migrated three existing stale capability fixtures after focused RED evidence:
  - `trait_impl_hidden_rigid_bound.srt` now consumes `Marker` through `Marker::mark`;
  - `trait_impl_transitive_where_obligation.srt` now consumes `Eq` through `Eq::eq` and forwards `Marker` through `Marker::mark`;
  - `trait_method_where_alpha_equivalent.srt` now consumes both alpha-renamed `Eq` and `Show` capabilities.

### Scar consumer tests

Per explicit authorization, only these stale cases in `crates/scar/tests/typecheck_surface.rs` were migrated:

1. `parameterized_trait_bound_controls_rigid_generic_dispatch`
   -> `bare_trait_capability_controls_full_parameterized_dispatch`
2. `parameterized_trait_bounds_validate_arity_and_generic_scope`
   -> `trait_heads_and_expression_obligations_retain_nested_arguments`
3. `impl_where_obligations_match_parameterized_trait_arguments`
   -> `impl_bare_capabilities_emit_full_parameterized_obligations`

The replacements keep full arguments in trait/impl heads and explicit dispatch expressions; only normal declaration `where` RHS is bare.

After the Task 5 solver fix, three additional authorized stale cases in the same file were migrated to consume their declared capabilities while retaining their original purpose:

4. `child_impl_where_assumptions_cover_parent_impl_requirements`
   - emits `FixtureEq::eq` and `FixtureShow::show` operations and still proves stronger child coverage;
5. `deferred_trait_obligation_is_checked_when_closure_argument_is_bound`
   - emits `Marker::mark` from the impl body and asserts the nested unresolved dispatch after binding to `Int`;
6. `trait_dispatch_rejects_impl_with_unsatisfied_where_obligation`
   - emits `Marker::mark` from the impl body and asserts that the unsatisfied nested obligation rejects dispatch.

### Analysis and REPL consumers

- Command-query types reject `Self` and `Type`, including nested occurrences, as contextual markers rather than concrete owners.
- Tolerant parameterized-bound diagnostics remain parser-category diagnostics and do not leak into resolve/typecheck categories.
- Completion metadata retains `Functor::fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>` and `FunctionCall` identity.
- xldr `:sig` coverage checks `Self`, `Type`, and `List<Self>` retain `ReplQueryParseError` and the concrete-type boundary message.

### Runtime audit

- No Forge or Eldr file was changed.
- No runtime dispatch, type tag, dictionary, builtin, or opcode contract was changed.

## TDD evidence

### RED

- Initial `cargo nextest run -p scar`: 56 passed, 78 failed because `lib/types/json.srt` stopped stdlib bootstrap at the illegal parameterized bound.
- The new query-marker test failed because `Self` was reported only as a generic unsupported query argument instead of a contextual/concrete-type boundary.
- The new xldr marker test initially stopped at the same JSON bootstrap error.
- Full Rune fixture RED after the Task 5 proof-forwarding fixes exposed three stale fixtures as `UnusedTraitConstraint`; those were migrated to make actual matching trait calls.

### GREEN

- `cargo fmt --check`: passed.
- `cargo check --workspace`: passed.
- `cargo nextest run -p surtr-analysis`: 135 passed.
- `cargo nextest run -p scar`: 145 passed.
- Both focused Scar migration groups: 3/3 passed and 3/3 passed.
- `cargo nextest run -p rune --test integration module_import_fixtures`: 10 passed.
- `cargo nextest run -p rune --test integration run_srt`: 27 passed.
- `cargo nextest run -p xldr`: 201 passed.
- First `cargo nextest run --workspace`: 1,712 passed, 202 skipped by the configured profile.
- Second `cargo nextest run --workspace`: 1,712 passed, 202 skipped by the configured profile.

## Resolved integration blockers

The shared Task 5 commits `849851ef`, `80b8ff73`, and `ecfd897e` resolved the default-method, builtin proof-forwarding, candidate-applicability, checked-signature, and deferred-obligation regressions exposed by this consumer migration.

The consumer migrations retain real trait operations. In particular, the transitive Rune fixture remains a compile failure and now asserts the stable nested `Eq::eq could not be specialized to a concrete dispatch target` diagnostic rather than an outer candidate message.

## Final verification checklist

- [x] Focused consumer tests.
- [x] Full `surtr-analysis` suite.
- [x] Rune module fixture suite.
- [x] `cargo fmt --check`.
- [x] `cargo check --workspace`.
- [x] Forge/Eldr diff audit.
- [x] Full Rune script fixtures after Task 5 fix.
- [x] Full xldr suite after Task 5 fix.
- [x] `cargo nextest run --workspace` twice after Task 5 fix.
- [x] Consumer-only staged diff audit.
- [x] Independent Task 6 consumer commit.

## Commit

Independent Task 6 consumer commit containing this report; see the Task 6 handoff or `git log -1` for its immutable hash.
