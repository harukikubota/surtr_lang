# Constrained Builtins Design

**Goal:** Ensure every non-`@intrinsic` builtin expresses and enforces required trait constraints through the normal compiler obligation-checking route, including propagation to callers.

## Scope and invariants

- `@intrinsic` declarations remain outside this change. They are compiler special forms whose constraint checking is intentionally ignored.
- A constrained `@builtin` must carry its constraints from source syntax through Spire, Sigil, and Scar.
- A builtin call must be checked like a constrained generic function call; matching `List<$A>` or another shape is not sufficient.
- A generic caller that invokes a constrained builtin must expose or satisfy the resulting obligation. A concrete caller must be rejected when the concrete type has no matching trait implementation.
- Runtime builtin dispatch and builtin IDs remain unchanged. Runtime code is reached only after compile-time obligation checking succeeds.
- The standard `.srt` declaration is the source-level contract. Rust builtin metadata continues to describe runtime names, arity, and runtime signatures, but does not become a second source of trait requirements.

## Current failure boundary

`@builtin` parsing currently rejects `where`, `Resolved::BuiltinDecl` has no where-clause field, and Scar registers builtin declarations as an unconstrained `Ty::BuiltinFunc`. The builtin call branch then performs only positional argument type matching. `List::group_count` therefore accepts `List<User>` even when `User` has no `Eq` implementation. `dedup` inherits the same gap because it calls `group_count` without a bound.

## Proposed data flow

1. Spire accepts an optional function-style `where` clause on `@builtin def` declarations and stores it in the builtin AST node.
2. Sigil resolves that clause exactly as it resolves a normal function where clause and stores it on `Resolved::BuiltinDecl`.
3. Scar predeclaration records the resolved builtin signature and its typed where clause in the callable metadata used by ordinary call checking. The builtin remains runtime-backed, but its type-level contract is no longer only a bare `Ty::BuiltinFunc`.
4. The builtin call path invokes the same argument/obligation route used by constrained user functions. Trait calls with concrete arguments are solved immediately; unresolved generic obligations remain pending and are rechecked during specialization/concretization.
5. Existing dispatch lowering emits the same builtin target and runtime builtin ID after type checking. No Eldr-side trait lookup is introduced.

## Source contract changes

The standard declaration becomes:

```surtr
@builtin def group_count(values: List<$A>) -> List<($A, Int)>
where
  $A: Eq
```

The helper that delegates to it also declares the capability it requires:

```surtr
def dedup(values: List<$A>) -> List<$A>
where
  $A: Eq
```

Any other standard builtin found by the audit to require a trait capability receives the same source-level treatment. Builtins whose constraints are already concrete nominal types, special-form contracts, or runtime capabilities are not given artificial trait bounds.

## Compatibility and diagnostics

- Existing valid calls remain valid when their argument types implement the required trait.
- Calls using a type without the required trait fail in the `typecheck` phase with the existing trait-obligation diagnostic style.
- The error should identify the constrained builtin and the missing trait where the existing callable signature/hint machinery permits it.
- A malformed or unsupported builtin where clause remains a parse/resolve/type error at the declaration boundary, rather than silently dropping the constraint.

## Testing strategy

- Parser unit test: a constrained `@builtin def` preserves its where clause; `@intrinsic` behavior remains unchanged.
- Resolver/Scar unit coverage: builtin constraints are registered and checked through the normal obligation solver.
- Compile-fail fixture: a custom struct without `Eq` passed to `List::group_count` is rejected in `typecheck`.
- Compile-fail fixture: `List::dedup` rejects the same custom struct because the caller's bound is propagated.
- Success fixture: `Int` and another existing `Eq` implementation continue to work.
- Audit test or explicit fixture inventory: every standard builtin requiring a trait constraint is represented by a declaration with the constraint and a call-site check.
- Run the focused tests first, then the required workspace and Rune integration suites.

## Non-goals

- No changes to `@intrinsic` checking.
- No runtime trait dictionaries or dynamic trait dispatch.
- No change to builtin ID ordering or Eldr value representation.
- No unrelated refactoring of the trait solver.
