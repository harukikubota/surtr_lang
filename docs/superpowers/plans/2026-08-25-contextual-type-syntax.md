# Contextual Type Syntax Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make type-related syntax context-sensitive: `where` trait bounds are bare family capabilities, constructor-shape and slot-map forms stay distinct, and constructor applications have the specified signature-only witness lifecycle.

**Architecture:** Update the normative surface contract before code. Then move the `where` trait RHS from `Spire` to `Sigil` to `Scar` without losing full expression-dispatch arguments: declaration bounds hold only a trait family capability, while trait calls continue to carry `(trait_id, trait_args, receiver)` structurally. Scar owns declaration well-formedness, witness resolution, unused-capability tracking, and the no-pending-dispatch boundary; Forge and Eldr remain unchanged concrete-dispatch consumers.

**Tech Stack:** Rust workspace (`spire`, `sigil`, `scar`, `forge`, `eldr`, `rune`, `surtr-analysis`, `xldr`), Pure Surtr standard definitions, `cargo nextest`.

**Spec:** `doc/contextual_type_syntax_impact_analysis.md`; `doc/where_trait_parameter_separation_proposal.md`; `doc/要件定義v9.md`; `docs/dev/Trait_system_spec.md`; `docs/dev/diagnostics.md`; `docs/dev/テスト方針.md`.

## Global Constraints

- The three `where` RHS forms are `Type<Ty, ...>`, bare `Trait`, and `TypeConstructorTrait.$Slot`; a parameterized `Trait<...>` RHS is always a parse error.
- `Type<...>` is valid only as `Self: Type<...>` in a `deftrait` where clause. It is not a normal trait bound; a TypeConstructor trait has no head parameters, including after inherited-shape closure.
- Keep trait-head arguments, trait-impl-head arguments, and trait-call expression arguments. Only declaration-level `where` trait bounds lose arguments.
- `Applicative<$A>`-style constructor applications are valid only as a direct parameter or return type of a normal function or trait method. Parameter witnesses are independent per position; every return use gets a fresh witness and must resolve to one concrete constructor before Forge.
- `Self<$A>` is a type-position substitution marker for the known declaration target. `Self` and `Type` are never value-level qualified-call owners; `impl Result<$T>` remains invalid.
- A bare capability does not prove an impl by itself. Only an expression may create a full structured obligation `(trait_id, trait_args, receiver)`; unused body/block capabilities are `UnusedTraitConstraint` errors.
- Do not add runtime dictionaries, runtime tags, or Forge/Eldr dispatch behavior. Forge receives only concrete dispatches.
- Preserve user-authored dirty files, including both `doc/*proposal*.md` inputs and the Either draft. Stage only files belonging to each task.

---

### Task 1: Reconcile the normative context matrix and test contract

**Files:**
- Modify: `doc/要件定義v9.md`
- Modify: `docs/dev/Trait_system_spec.md`, `docs/dev/テスト方針.md`, `docs/dev/diagnostics.md`
- Modify: `docs/site/trait-system.md`, `docs/site/trait-impls.md`, `docs/site/type-annotations.md`, `docs/site/language-reference.md`, `docs/site/language-guide.md`

**Interfaces:**
- Consumes: the two user-provided impact/proposal documents.
- Produces: the source-of-truth acceptance matrix used by parser, resolver, checker, diagnostics, and fixture work.

- [ ] **Step 1: Replace parameterized declaration-bound examples.** State that a normal bound is `$T: Trait`; replace `Marker<Int>`, `Encode<String>`, and parent-bound examples with bare capability forms. Retain `deftrait TryFrom<$To>`, `impl TryFrom<Int> for String`, and `try_from::<Int>(value)` as their distinct valid surfaces.
- [ ] **Step 2: Specify the contextual matrix.** Document `Self: Type<...>` restriction, `Trait.$Slot` semantics, direct function/trait-method parameter-or-return restriction for constructor applications, `Self<$...>` substitution, and value-level `Self::`/`Type::` rejection.
- [ ] **Step 3: Specify typechecker lifecycle.** Define bare-capability proof environments, structured full expression obligations, position-keyed parameter witnesses, fresh concrete return witnesses, unused-capability scope, and the Scar-before-Forge pending-dispatch audit.
- [ ] **Step 4: Specify diagnostics and coverage.** Add parser/typechecker diagnostic responsibilities with position-rule notes and rewrite helps; replace old parameterized-bound and root-shared witness matrix entries with the required positive/negative cases.
- [ ] **Step 5: Review and commit.** Search the authoritative docs for statements accepting `$T: Trait<...>`; preserve only impl-head/call examples. Commit only the changed normative documents.

### Task 2: Make Spire parse the three contextual RHS forms

**Files:**
- Modify: `crates/spire/src/ast.rs`, `crates/spire/src/parser/{decl.rs,ty.rs,mod.rs,tolerant.rs}`
- Modify: `crates/spire/src/parser/tests.rs`

**Interfaces:**
- Consumes: Task 1 matrix.
- Produces: `WhereConstraintRhs::Trait` with a name and span only; parser type/declaration context sufficient to reject source-determinable misuse.

- [ ] **Step 1: Write failing parser tests.** Cover every declaration boundary for `Self: Type<$A>`, non-`Self`/wrong-block `Type`, every `Trait<$A>` RHS, `Self::f()`/`Type::f()`, and allowed versus forbidden direct/nested constructor applications.
- [ ] **Step 2: Run focused Spire tests and confirm each new case fails for the intended missing behavior.**
- [ ] **Step 3: Change the AST and parser.** Remove trait-RHS arguments; introduce declaration-block and type-position parser context; retain separate `TypeConstructor` and `TraitSlot` variants; update source rewriting, span shifting, and tolerant parsing exhaustively.
- [ ] **Step 4: Run `cargo nextest run -p spire`; commit the AST/parser/test slice.**

### Task 3: Propagate bare bounds through Sigil without weakening expression dispatch

**Files:**
- Modify: `crates/sigil/src/{resolved.rs,semantic_metadata.rs}`
- Modify: `crates/sigil/src/resolver/{declarations.rs,expr.rs,mod.rs}`
- Modify: relevant Sigil resolver tests

**Interfaces:**
- Consumes: bare Spire `Trait` bounds and slot-map AST.
- Produces: resolved bare trait capabilities and validated slot-map identities; full trait-call arguments remain structural expression data.

- [ ] **Step 1: Add failing resolver tests** for canonical bare bounds, bare parent edges, valid constructor-trait slot maps, and slot maps targeting a non-constructor trait.
- [ ] **Step 2: Run the focused tests and observe expected failures.**
- [ ] **Step 3: Remove bound-argument resolution from all resolved/rebase/metadata paths.** Resolve only trait identity for a normal bound; retain trait-head and expression-call argument resolution.
- [ ] **Step 4: Run `cargo nextest run -p sigil --lib`; commit the Sigil slice.**

### Task 4: Enforce declaration well-formedness and `Self` substitution in Scar

**Files:**
- Modify: `crates/scar/src/{typed.rs,checker/{predeclare.rs,definitions.rs,mod.rs}}`
- Modify: `crates/scar/tests/typecheck_surface.rs`

**Interfaces:**
- Consumes: Task 3 resolved bare capabilities and slot-map identities.
- Produces: declaration-bound well-formedness, inherited constructor-trait head validation, and target-aware `Self<$...>` contract expansion.

- [ ] **Step 1: Add failing Scar tests** for rejected `Type` LHS/block forms, direct and inherited constructor-trait head parameters, slot-map target/completeness, plain-inherent `Self<$A>`, and captured generic parameters.
- [ ] **Step 2: Confirm the focused tests fail before the checker change.**
- [ ] **Step 3: Update Typed `where` data and predeclaration checks.** Remove bare-bound args, retain full impl-head args, run post-closure constructor-head checks, and preserve `Self<$...>` substitution in trait contract comparison.
- [ ] **Step 4: Run `cargo nextest run -p scar`; commit the declaration slice.**

### Task 5: Implement signature witnesses, full obligations, and unused capabilities

**Files:**
- Modify: `crates/scar/src/checker/{types.rs,expr.rs,specialize.rs,definitions.rs,mod.rs}` and `crates/scar/src/typed.rs`
- Modify: Scar unit tests and focused Rune fixtures

**Interfaces:**
- Consumes: Task 4 declaration contracts.
- Produces: position-scoped parameter witnesses, fresh return witnesses resolved to concrete constructors, capability consumption tracking, and structural pending dispatches with a pre-Forge audit.

- [ ] **Step 1: Add failing tests.** Verify same-root different-parameter witnesses, different roots, fresh result witness distinctness, concrete result success, unresolved/mixed-result failure, forbidden local/field/closure/nested applications, bare `TryFrom` capability vs `TryFrom<$To>` call obligation, and each unused-capability scope.
- [ ] **Step 2: Run focused tests to establish RED.**
- [ ] **Step 3: Implement the minimal lifecycle.** Key parameter witnesses by direct signature position; allocate return witnesses freshly; resolve and compare body returns; mark capabilities consumed only by emitted full obligations or forwarded proof; fail unconsumed capabilities and pending dispatch before Forge.
- [ ] **Step 4: Run `cargo nextest run -p scar` and focused Rune fixtures; commit the solver slice.**

### Task 6: Update standard definitions, fixtures, LSP/REPL consumers, and final verification

**Files:**
- Modify only as required: `lib/traits/operator/{functor,bifunctor,applicative,alternative,monad}.srt`, `lib/types/{list,option,result,either}.srt`, `lib/traits/{from,try_from}.srt`
- Modify/create: affected `tests/fixtures/**`, `crates/surtr-analysis/**`, `crates/xldr/**`
- Verify: `crates/forge/src/codegen.rs`, `crates/eldr/src/vm.rs`

**Interfaces:**
- Consumes: completed parser/resolver/checker contracts.
- Produces: user-facing standard-library and tooling behavior consistent with the matrix, with no runtime dispatch change.

- [ ] **Step 1: Add/update failing fixture and tooling tests** for declaration boundaries, standard definitions, tolerant parse category, completion/signature display, and query concrete-type boundaries.
- [ ] **Step 2: Run focused tests to establish RED.**
- [ ] **Step 3: Update only sources required by the new surface.** Keep `Functor.$A` slot maps and `Self<$...>` signatures valid; do not rewrite valid trait-head or call-site arguments into bare forms.
- [ ] **Step 4: Run focused crate and fixture checks; commit the consumer slice.**
- [ ] **Step 5: Run final verification:** `cargo fmt --check`, `cargo check --workspace`, `cargo nextest run --workspace` twice, and the affected Rune fixture suites. Audit the diff to prove Forge/Eldr/runtime contracts were not changed.
