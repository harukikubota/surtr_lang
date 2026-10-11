# TypeIdentity Owner Registry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand `TypeIdentity` to the approved owner taxonomy and reject every duplicate top-level owner name—including `defrecord Hoge` and `defmod Hoge`—during Sigil declaration precollection.

**Architecture:** Keep the existing `DeclarationIndex` responsible for value/type bindings, imports, and deterministic UIDs. Add an adjacent, canonical `OwnerRegistry` that records only identity-bearing owners and is the sole collision authority; this avoids treating a module root as a callable binding. Lowering retains each module/process declaration span and owner kind so precollection can register module roots, type heads, traits, signature aliases, and constants before body resolution. `DeclarationEntry` and semantic-analysis queries read the registry for an owner's identity; members, enum variants, and `impl` blocks resolve back to that owner rather than receiving a new identity.

**Tech Stack:** Rust workspace (`spire`, `sigil`, `scar`, `sindr`, `surtr-analysis`, `rune`); `cargo nextest`; existing module/script fixture harness.

**Spec:** `doc/要件定義v9.md` §2.2.1 `TypeIdentity` (rewritten by Task 1); `docs/dev/Trait_system_spec.md`; `docs/dev/diagnostics.md`; `docs/dev/テスト方針.md`.

## Global Constraints

- `TypeIdentity` is compile-space metadata. Do not change Xldr's runtime `RuntimeTypeDisplay` / REPL value-identity categories.
- Classification follows the declaration form, not the error concept: abstract builtin `Error` comes from `type` and is `TypeIdentity::Type`; a concrete `deferror` comes from `deferror` and is the dedicated `TypeIdentity::Error`. `TypeKind::ConcreteError` may remain as Scar's internal runtime/type-definition classification and does not name the compile-space identity.
- `TypeConstructor` takes precedence for standard generic type-level owners (`List`, `Option`, `Result`, and every other standard owner explicitly marked as such in shared metadata) and for promoted traits. An ordinary user `defenum` remains `Enum`, even when generic.
- A type-constructor trait remains `DeclarationKind::Trait`; only its `TypeIdentity` is promoted. Existing trait lookup, coherence, and constructor-slot metadata remain intact.
- `defagent` and `defgenserver` register as `Mod`; `defsupervisor` and `defdynamicSupervisor` register as `Supervisor`. Their existing `ProcessSpec` remains the source of runtime process behavior.
- The owner namespace is flat for type-like owners exactly as today. Module/process owners use their canonical declared path: bare `Hoge` conflicts with bare type-like `Hoge`; `A::Hoge` is distinct from root `Hoge`. A public `const` uses its existing public surface name; a private const uses its existing `__const__`-qualified key and remains private to that key.
- `impl Type`, trait methods, enum variants, module members, result-constructor contracts, imports, namespaces, and `SupervisorInit` do not create owner records. Their semantic identity is their enclosing/target owner where one exists.
- New duplicate-owner failures are resolver diagnostics with the second declaration as primary span and the first declaration as a related label. The stable fixture fragment is `Duplicate top-level owner: <name>`.
- Do not merge `OwnerRegistry` entries into `Scope` or UID allocation. Modules are paths/owners, not values; the existing callable/type binding semantics must not change.
- Increment `SYMBOL_CAPABILITY_SCHEMA_VERSION` because serialized compile-space identity metadata changes.

## File Structure

- Modify: `doc/要件定義v9.md` — normative taxonomy, namespace, and promotion rules.
- Modify: `docs/dev/テスト方針.md` — resolver and warm-fixture coverage contract for owner collisions.
- Modify: `crates/sindr/src/names.rs` — expanded enum, shared identity/capability constructors, builtin identity lookup, schema version.
- Modify: `crates/sindr/src/builtin.rs` — declare standard builtin-owner identities in metadata instead of inferring them from arity or hard-coding names in Sigil.
- Modify: `crates/sigil/src/resolver/declarations.rs` — owner data types, lowered/staged owner provenance, registry construction, collision diagnostic, trait promotion fixed point.
- Modify: `crates/sigil/src/resolver/mod.rs` and `crates/sigil/src/resolver/session.rs` — export/store registry and use it when resolving owner/member metadata; preserve it across checkpoints.
- Modify: `crates/sigil/src/resolver/imports.rs` — consume declaration binding entries only; add regression assertions that owner registration does not create module-value bindings.
- Modify: `crates/sigil/src/resolver/expr.rs` and `crates/sigil/src/resolved.rs` — attach registry-backed identity to resolved owner IDs and preserve owner references for declarations that already carry them; leave member identity owner-derived.
- Modify: `crates/scar/src/env.rs` and `crates/scar/src/checker/predeclare.rs` — rename `ConcreteError` mapping to `Error`, remove duplicate-head checks that become unreachable, retain type-only registration/type-cycle behavior.
- Modify: `crates/surtr-analysis/src/semantic.rs` — obtain module, trait, signature-alias, const, and type identity from `OwnerRegistry`, and distinguish `Mod` from `Supervisor` in semantic metadata without changing completion kinds unnecessarily.
- Modify: `crates/sigil/src/resolver/tests.rs`, `crates/sindr/src/names.rs` tests, `crates/scar/tests/typecheck_surface.rs`, and `crates/surtr-analysis/tests/completion.rs` — focused unit coverage.
- Create: `tests/fixtures/modules/fail/type_identity_record_mod/{entry.srt,RecordThenMod.srt,ModThenRecord.srt,entry.error}`, `type_identity_trait_type/{entry.srt,Owners.srt,entry.error}`, `type_identity_sig_type/{entry.srt,Owners.srt,entry.error}`, and `type_identity_process_owner/{entry.srt,Owners.srt,entry.error}` — isolated cross-source resolve fixture coverage.
- Verify: `docs/dev/diagnostics.md` — the existing `ResolveError` renderer already supports the required primary and related labels; no diagnostics API change is planned.

---

### Task 1: Make the TypeIdentity contract normative before code changes

**Files:**
- Modify: `doc/要件定義v9.md:44-79`
- Modify: `docs/dev/テスト方針.md:264-289, 472-478`
- Verify: `docs/dev/Trait_system_spec.md:113-129`
- Verify: `docs/dev/diagnostics.md:16-34, 68-90`

**Interfaces:**
- Consumes: The user-approved taxonomy in this task request and the existing flat type namespace contract.
- Produces: A source-of-truth owner classification and collision rule consumed by `OwnerRegistry::register` in Task 3.

- [ ] **Step 1: Replace the TypeIdentity subsection with the final 11-value taxonomy**

  Write the table below in `doc/要件定義v9.md` and remove every use of the old `ConcreteError` name from that subsection.

  | Source owner | Identity | Canonical owner key |
  |---|---|---|
  | abstract builtin types declared with `type` (`Int`, `String`, `Error`) | `Type` | builtin surface name |
  | standard generic type-level owners (`List`, `Option`, `Result`, etc.) | `TypeConstructor` | builtin/standard surface name |
  | `defstruct`, `defrecord`, `defenum`, concrete `deferror` | `Struct`, `Record`, `Enum`, `Error` | flat type head |
  | `defmod`, `defagent`, `defgenserver` | `Mod` | canonical module path |
  | `defsupervisor`, `defdynamicSupervisor` | `Supervisor` | canonical module path |
  | function-signature `type Alias = (...)` | `Sig` | flat alias head |
  | `const` | `Const` | public surface name or private `__const__` key |
  | ordinary `deftrait` | `Trait` | flat trait head |

- [ ] **Step 2: State the uniqueness and ownership rules precisely**

  Add this normative rule immediately after the table:

  ```text
  Every identity-bearing owner is inserted exactly once into the compile-unit
  OwnerRegistry under its canonical owner key. Registering a second owner with
  the same key is a resolve error, regardless of declaration order or identity.
  `defrecord Hoge` and `defmod Hoge` therefore conflict. Member declarations,
  enum variants, trait methods, and `impl` blocks do not register a second
  TypeIdentity; they refer to their enclosing or target owner.
  ```

  Also record the deliberate scope boundary: `defmod A::Hoge` does not conflict with root `Hoge`, while bare type-like heads remain flat exactly as in the current specification.

- [ ] **Step 3: Specify TypeConstructor promotion as an identity fixed point**

  Add the following rules, using the existing `Self: Type<...>` syntax from `docs/dev/Trait_system_spec.md`:

  ```text
  A trait is TypeConstructor when its own where clause contains
  `Self: Type<...>`, or when any parent trait is TypeConstructor. Promotion is
  transitive and order-independent. The trait declaration kind remains Trait;
  only its TypeIdentity changes. `impl Functor for List` never changes List's
  identity.
  ```

  Declare that conflicting inherited constructor-slot shapes remain the existing Scar trait-contract error; identity promotion itself does not introduce another slot-consistency rule.

- [ ] **Step 4: Extend the test policy with the mandatory regression matrix**

  Add requirements for: both declaration orders of record/module collision; cross-file/stage collisions; all process identity mappings; type-alias and trait collisions; direct and inherited trait promotion; first-definition related diagnostic label; and a regression proving that module owner registration does not make `Hoge` a value binding.

- [ ] **Step 5: Review the wording against the diagnostics contract**

  Confirm the planned diagnostic roles are: headline `Duplicate top-level owner: Hoge`; primary label `conflicting <Identity> declaration`; related label `first <Identity> declaration`; note explaining the shared owner namespace; and help directing the user to rename one owner. Do not put the namespace rule in a source label.

- [ ] **Step 6: Commit the specification change**

  ```bash
  git add doc/要件定義v9.md docs/dev/テスト方針.md
  git commit -m "docs: define unified TypeIdentity owner namespace"
  ```

### Task 2: Establish one shared identity and capability source in Sindr

**Files:**
- Modify: `crates/sindr/src/names.rs:113-175, 327-328, 487-526, 572-640`
- Modify: `crates/sindr/src/builtin.rs:20-25, 1059-1164`
- Test: `crates/sindr/src/names.rs:572-640`

**Interfaces:**
- Consumes: Task 1's identity table.
- Produces: `TypeIdentity::{Type, TypeConstructor, Struct, Record, Enum, Error, Mod, Supervisor, Sig, Const, Trait}` and `SymbolIdentityInfo` constructors used by Sigil and analysis.

- [ ] **Step 1: Write the failing metadata tests**

  Add a table-driven test that queries `builtin_symbol_identity_info` and asserts the intended distinction:

  ```rust
  let cases = [
      ("Int", TypeIdentity::Type),
      ("Error", TypeIdentity::Type),
      ("List", TypeIdentity::TypeConstructor),
      ("HashMap", TypeIdentity::TypeConstructor),
      ("Result", TypeIdentity::TypeConstructor),
  ];
  for (name, identity) in cases {
      assert_eq!(builtin_symbol_identity_info(name).unwrap().identity, identity);
  }
  ```

  Add capability assertions that a `TypeConstructor` still has the same type-annotation, inherent-impl, and Facet-root capabilities that its current builtin surface owns; identity classification must not alter type checking.

- [ ] **Step 2: Run the focused test before implementation**

  Run: `cargo nextest run -p sindr names::tests::builtin_symbol_identity_info_marks_core_type_capabilities`

  Expected: FAIL because `List`, `HashMap`, and `Result` still report `Type`.

- [ ] **Step 3: Expand the enum and centralize standard-owner identity metadata**

  Replace `ConcreteError` with `Error`, add the five missing variants, and bump:

  ```rust
  pub const SYMBOL_CAPABILITY_SCHEMA_VERSION: u32 = 2;

  pub enum TypeIdentity {
      Type,
      TypeConstructor,
      Struct,
      Record,
      Enum,
      Error,
      Mod,
      Supervisor,
      Sig,
      Const,
      Trait,
  }
  ```

  Add `identity: TypeIdentity` to `BuiltinTypeMeta`; assign every entry explicitly. Normal generic public builtins get `TypeConstructor`; compiler-special generic heads (`MatchArms`, `CondClauses`, `BulkUpdateEntries`) retain `Type`. Route `builtin_symbol_surface_meta` through this metadata rather than returning `TypeName::identity()` unconditionally. Keep `Tuple` and `Function` as `Type`.

- [ ] **Step 4: Add shared owner capability constructors**

  Add named constructors such as `SymbolCapabilities::type_owner()`, `module_owner()`, `supervisor_owner()`, `signature_owner()`, `const_owner()`, and `trait_owner()`. Set module/supervisor to `type_annotation: false`, `impl_target: false`, and no Facet root. Do not use the existing `module_owner` boolean to mean both “can own functions” and “is a module declaration”; retain its current behavior until a consumer needs a separate semantic capability.

- [ ] **Step 5: Run Sindr tests**

  Run: `cargo nextest run -p sindr`

  Expected: PASS, including derive applicability remaining restricted to `Struct | Record | Enum`.

- [ ] **Step 6: Commit the shared model**

  ```bash
  git add crates/sindr/src/names.rs crates/sindr/src/builtin.rs
  git commit -m "feat: expand TypeIdentity metadata"
  ```

### Task 3: Add a dedicated OwnerRegistry and collect every identity-bearing declaration

**Files:**
- Modify: `crates/sigil/src/resolver/declarations.rs:105-145, 114-143, 469-580, 1340-1810`
- Modify: `crates/sigil/src/resolver/mod.rs:25-35, 70-102, 325-459, 1025-1045`
- Modify: `crates/sigil/src/resolver/session.rs:7-31, 86-115, 122-166`
- Test: `crates/sigil/src/resolver/tests.rs:542-595, 4505-4574`

**Interfaces:**
- Consumes: `TypeIdentity` and shared capability constructors from Task 2; lowerer input `Ast` declarations.
- Produces: `OwnerRegistry`, `OwnerEntry`, `OwnerKind`, and `OwnerRef` lookup APIs; resolve-phase duplicate-owner errors with two source spans.

- [ ] **Step 1: Add failing owner-registry unit tests**

  Construct staged modules with the two exact permutations below and assert that precollection fails before any body resolution:

  ```rust
  defrecord Hoge(a: String)
  defmod Hoge { def test() { () } }
  ```

  ```rust
  defmod Hoge { def test() { () } }
  defrecord Hoge(a: String)
  ```

  For both errors assert `message == "Duplicate top-level owner: Hoge"`, primary span is the second `Hoge`, and `related_labels` contains the first `Hoge` span with `first ... declaration` text.

- [ ] **Step 2: Preserve module/process declaration provenance through lowering**

  Extend `StagedModuleAst` and `LoweredModuleAst` with an owner descriptor carrying canonical path, declaring span, and source form. Populate it for `Ast::Defmod`, `Defagent`, `Defgenserver`, `Defsupervisor`, and `DefdynamicSupervisor`; leave fallback/impl/trait-impl staging modules without an owner descriptor. Do not infer module roots from member names or file names.

- [ ] **Step 3: Define the registry separately from binding declarations**

  Add data structures equivalent to:

  ```rust
  pub struct OwnerEntry {
      pub canonical_key: String,
      pub identity: TypeIdentity,
      pub kind: OwnerKind,
      pub span: Span,
      pub stage_index: usize,
      pub module_path: Option<String>,
  }

  pub struct OwnerRegistry {
      entries: BTreeMap<String, OwnerEntry>,
  }
  ```

  Give it `register(entry) -> Result<(), ResolveError>`, `get(key)`, and `identity_for_owner(key)`. `register` must create the Task 1 diagnostic when a key exists; it must retain the first span in the related label. Keep `DeclarationIndex = BTreeMap<String, DeclarationEntry>` unchanged for UID/scope/import consumers.

- [ ] **Step 4: Register all direct owners in deterministic source order**

  During `precollect_declaration_index`, build `OwnerRegistry` in the same stage/module/AST order used for declaration UID ordering. Register:

  ```rust
  Ast::BuiltinTypeDecl       => builtin metadata identity
  Ast::StructDef             => Struct
  Ast::RecordDef             => Record
  Ast::EnumDef               => Enum
  Ast::DeferrorDef           => Error
  Ast::TraitDef              => Trait provisionally
  Ast::TypeAlias             => Sig
  Ast::ConstDef              => Const, using existing public/private key rules
  staged defmod/agent/server => Mod
  staged supervisor forms    => Supervisor
  ```

  Reuse `global_surface_name`, `canonical_type_key`, and current public/private const key construction so type heads do not silently gain a module namespace. Do not register `Def`, extractors, enum variants, trait methods, `ImplDef`, `TraitImplDef`, `ResultCtorDecl`, `Namespace`, or `SupervisorInit`.

- [ ] **Step 5: Promote trait identity after all trait heads are known**

  Reuse the existing trait-parent and `Self: Type<...>` collection logic. Seed a set with direct constructor traits, then repeatedly add traits whose parent key is already in the set until no identity changes. Update the corresponding `OwnerEntry.identity` to `TypeConstructor`; leave its `OwnerKind::Trait` and `DeclarationKind::Trait` unchanged. This makes declaration order irrelevant and preserves the existing slot map/error behavior.

- [ ] **Step 6: Thread the registry through resolver lifecycle state**

  Return the registry beside the declaration index from precollection, store it in `Resolver`, `SigilSession`, and `SigilCheckpoint`, and clone it into staged parallel resolvers. Add `owner_identity_for_declaration(name, kind, enclosing_owner)` that returns direct owner identity when present, otherwise the enclosing/target owner identity for a member. No registry entry may reserve a scope UID or define a value binding.

- [ ] **Step 7: Run focused Sigil tests**

  Run: `cargo nextest run -p sigil resolver::tests::test_precollect_declaration_index_rejects_duplicate_fully_qualified_name`

  Run: `cargo nextest run -p sigil resolver::tests::test_precollect_namespaced_duplicate_type_is_rejected`

  Run: `cargo nextest run -p sigil --lib`

  Expected: PASS; existing duplicate callable/type behavior is unchanged and the two new record/module permutations fail at precollection.

- [ ] **Step 8: Commit the registry boundary**

  ```bash
  git add crates/sigil/src/resolver/declarations.rs crates/sigil/src/resolver/mod.rs crates/sigil/src/resolver/session.rs crates/sigil/src/resolver/tests.rs
  git commit -m "feat: register TypeIdentity owners during precollection"
  ```

### Task 4: Make resolver identity propagation owner-aware without changing lookup semantics

**Files:**
- Modify: `crates/sigil/src/resolver/expr.rs:1704-1711, 2583-2765, 2826-2855, 2897-2995, 3288-3300, 3418-3555`
- Modify: `crates/sigil/src/resolved.rs:296-300` only if a signature alias needs an explicit owner reference for downstream metadata.
- Modify: `crates/sigil/src/resolver/imports.rs:68-119, 563-569`
- Test: `crates/sigil/src/resolver/tests.rs:1359-1412, 4505-4574`

**Interfaces:**
- Consumes: `OwnerRegistry::identity_for_owner` from Task 3.
- Produces: Consistent `SymbolIdentityInfo` for type heads and owner-derived semantic metadata; unchanged `Scope` lookup/import behavior.

- [ ] **Step 1: Add failing resolver tests for direct and inherited trait identity**

  Use a direct trait and a child trait:

  ```surtr
  deftrait Functor
  where
    Self: Type<$A>
  {
    def fmap(self: Self<$A>) -> Self<$A>
  }

  deftrait Applicative
  where
    Self: Functor
  {
    def pure(value: $A) -> Self<$A>
  }

  deftrait Show {
    def show(self: Self) -> String
  }
  ```

  Assert `Functor` and `Applicative` resolve with `TypeConstructor`, while `Show` resolves with `Trait`; also assert the existing constructor-slot data remains available for `Functor` and `Applicative`.

- [ ] **Step 2: Replace the narrow DeclarationKind switch with registry-backed metadata**

  Change `user_type_symbol_identity_info` / `declaration_symbol_identity_info` to consult the registry for `Struct`, `Record`, `Enum`, `Deferror`, `Trait`, `TypeAlias`, `Const`, module/process owners, and standard builtins. Retain kind-specific capabilities for actual type construction, and use Task 2's owner capability constructors for non-type owners.

- [ ] **Step 3: Preserve identity ownership for non-owner declarations**

  When creating `ResolvedId` for enum variants, trait methods, module members, and inherent/trait-impl members, resolve an `OwnerRef` to the enum/trait/module/impl target instead of assigning a new `TypeIdentity`. Do not modify their qualified callable names, visibility, or UID rules. A bare top-level `def` with no explicit owner continues to have no `TypeIdentity` rather than inventing an implicit module owner.

- [ ] **Step 4: Keep imports and scope binding planes separate**

  Add regression assertions around `build_global_scope` that an `OwnerRegistry` entry for `defmod Hoge` does not make `scope.lookup("Hoge")` succeed solely because of the module root. The type constructor binding of `defrecord Hoge` must remain unchanged when no collision exists. Do not make `import Hoge` legal merely because a module owner now has metadata.

- [ ] **Step 5: Run resolver tests**

  Run: `cargo nextest run -p sigil --lib`

  Expected: PASS, including the existing trait slot inheritance and all import-policy tests.

- [ ] **Step 6: Commit resolver propagation**

  ```bash
  git add crates/sigil/src/resolver/expr.rs crates/sigil/src/resolved.rs crates/sigil/src/resolver/imports.rs crates/sigil/src/resolver/tests.rs
  git commit -m "feat: resolve symbols through TypeIdentity owners"
  ```

### Task 5: Align Scar's type-only model and eliminate duplicate enforcement drift

**Files:**
- Modify: `crates/scar/src/env.rs:44-62`
- Modify: `crates/scar/src/checker/predeclare.rs:11-52, 451-529`
- Test: `crates/scar/tests/typecheck_surface.rs:4122-4279`

**Interfaces:**
- Consumes: Resolve phase has already guaranteed unique owner keys.
- Produces: `TypeKind::ConcreteError -> TypeIdentity::Error`; Scar continues to own type signatures, tags, aliases, and trait coherence but not cross-owner name collision policy.

- [ ] **Step 1: Write the failing rename regression**

  Update/add a `TypeKind::ConcreteError` test that expects `TypeIdentity::Error`, alongside existing struct/record/enum identity assertions.

- [ ] **Step 2: Change the compile-space mapping**

  Replace only the former compile-space `TypeIdentity::ConcreteError` mapping with `TypeIdentity::Error`. Keep abstract `Error` on `TypeIdentity::Type`. Do not rename runtime `RichError`, `Ty::Error`, error constructor behavior, tags, or the `TypeKind::ConcreteError` internal classification in this task.

- [ ] **Step 3: Narrow Scar duplicate checks to defensive invariants**

  Keep `predeclare_signature_aliases` and `predeclare_type_signatures` checks for malformed manually-constructed `Resolved` test inputs, but make their duplicate message an internal-consistency assertion path rather than the normal user-facing collision path. In normal compilation, duplicate owner diagnostics must originate in Sigil, before Scar. Preserve alias expansion, cycle detection, deterministic tags, and all field-signature passes.

- [ ] **Step 4: Run focused type-checker coverage**

  Run: `cargo nextest run -p scar typecheck_surface::trait_constructor`

  Run: `cargo nextest run -p scar`

  Expected: PASS; direct/inherited trait constructor semantics and type-alias behavior remain unchanged except for earlier resolve-phase duplicate rejection.

- [ ] **Step 5: Commit Scar alignment**

  ```bash
  git add crates/scar/src/env.rs crates/scar/src/checker/predeclare.rs crates/scar/tests/typecheck_surface.rs
  git commit -m "refactor: align Scar identities with owner registry"
  ```

### Task 6: Expose the complete owner identity set to semantic analysis and tooling

**Files:**
- Modify: `crates/surtr-analysis/src/semantic.rs:623-650, 666-735, 3073-3090`
- Modify: call sites that construct semantic indexes from compile metadata (found with `rg -n "symbol_semantic_infos_from_compile_metadata|symbol_semantic_infos_from_declaration_index" crates`)
- Test: `crates/surtr-analysis/tests/completion.rs:1749-1860, 2037-2060`
- Verify unchanged: `docs/dev/Xldr_spec.md:183`

**Interfaces:**
- Consumes: `OwnerRegistry` and registry-backed `DeclarationEntry`/owner references from Tasks 3–4.
- Produces: semantic `TypeIdentity` for direct owners and owner-derived members, without altering runtime `:type` rendering.

- [ ] **Step 1: Write semantic-index tests for newly visible identities**

  Build a minimal registry with entries for `Hoge` (`Mod`), `Worker` (`Mod`), `RootSup` (`Supervisor`), `Alias` (`Sig`), `Flag` (`Const`), `Show` (`Trait`), and `Functor` (`TypeConstructor`). Assert that `symbol_identity_for_declaration_entry` or its registry-aware replacement returns each exact identity. Assert a member `Hoge::test` reports the `Mod` owner identity but retains `CompletionKind::FunctionCall`.

- [ ] **Step 2: Make semantic identity queries registry-aware**

  Replace the current ad hoc `Const => TypeIdentity::Const` fallback and synthetic `TypeIdentity::Mod` insertion for every non-empty module path with owner-registry lookup. Preserve existing completion kinds: module/supervisor owners remain `TypePath`; traits remain `TypePath`; aliases/types remain `TypeConstructor` only when they are callable constructors; constants remain `Variable`.

- [ ] **Step 3: Preserve Xldr's runtime identity separation**

  Do not edit `crates/xldr/src/repl/logic/core.rs` runtime identity enum or `:type` golden output. Add/retain an assertion that compile-space semantic identity is consumed by completion/info metadata only; `:type value` still reports `RuntimeTypeDisplay`.

- [ ] **Step 4: Run analysis and REPL-focused tests**

  Run: `cargo nextest run -p surtr-analysis`

  Run: `cargo nextest run -p xldr repl_core`

  Expected: PASS. Any Xldr failure that expects the separate runtime `TypeIdentity::...` text is a regression and must not be updated merely to match compile-space variants.

- [ ] **Step 5: Commit semantic propagation**

  ```bash
  git add crates/surtr-analysis/src/semantic.rs crates/surtr-analysis/tests/completion.rs
  git commit -m "feat: expose owner identities to semantic analysis"
  ```

### Task 7: Add resolver fixtures and diagnostics coverage for the cross-kind collision matrix

**Files:**
- Create: `tests/fixtures/modules/fail/type_identity_record_mod/{entry.srt,RecordThenMod.srt,ModThenRecord.srt,entry.error}`
- Create: `tests/fixtures/modules/fail/type_identity_trait_type/{entry.srt,Owners.srt,entry.error}`
- Create: `tests/fixtures/modules/fail/type_identity_sig_type/{entry.srt,Owners.srt,entry.error}`
- Create: `tests/fixtures/modules/fail/type_identity_process_owner/{entry.srt,Owners.srt,entry.error}`
- Modify: `crates/sigil/src/resolver/tests.rs`
- Verify: `docs/dev/diagnostics.md`

**Interfaces:**
- Consumes: Task 3's resolver diagnostic.
- Produces: warm end-to-end regression coverage that reaches resolver precollection before Scar/Forge/Eldr.

- [ ] **Step 1: Create the minimal fixture case for the reported bug**

  Put this in `type_identity_record_mod/RecordThenMod.srt`:

  ```surtr
  defrecord Hoge(a: String)
  defmod Hoge {
      def test() { () }
  }
  ```

  Put the inverse declaration order in `type_identity_record_mod/ModThenRecord.srt`. Keep `entry.srt` a valid module fixture entry that imports neither conflicting owner, so the test proves precollection—not use-site lookup—rejects the input.

- [ ] **Step 2: Add representative identity-pair collisions**

  Put one collision in each isolated directory: `deftrait Same` + `defenum Same`; `type Same = ($A -> $A)` + `defstruct Same`; and `defagent ProcessName` + `defsupervisor ProcessName`. Each directory has its own `entry.srt` and `.error`, so the fixture harness observes its intended conflict rather than stopping at an unrelated earlier one. Establish the reverse declaration orders in Sigil unit tests.

- [ ] **Step 3: Pin only stable diagnostic fragments**

  Write `type_identity_record_mod/entry.error` as:

  ```text
  phase: resolve
  contains: Duplicate top-level owner: Hoge
  ```

  For the remaining three fixture `.error` files, use `phase: resolve` and the matching stable fragment (`Same` or `ProcessName`). Add direct unit assertions for the related label/span; do not make fixture tests depend on ANSI formatting, complete label text, or column layout.

- [ ] **Step 4: Run hot and warm resolver checks**

  Run: `cargo nextest run -p sigil --lib`

  Run: `cargo nextest run -p rune --test integration module_compile_error_fixtures_bucket_0`

  Run: `cargo nextest run -p rune --test integration module_import_fixtures`

  Expected: PASS. If bucket naming differs, select the bucket reported by `cargo nextest list -p rune --test integration | rg module_compile_error_fixtures` rather than running a non-existent filter.

- [ ] **Step 5: Commit the regression suite**

  ```bash
  git add tests/fixtures/modules/fail/type_identity_record_mod tests/fixtures/modules/fail/type_identity_trait_type tests/fixtures/modules/fail/type_identity_sig_type tests/fixtures/modules/fail/type_identity_process_owner crates/sigil/src/resolver/tests.rs
  git commit -m "test: cover TypeIdentity owner collisions"
  ```

### Task 8: Validate pipeline boundaries, full workspace behavior, and documentation consistency

**Files:**
- Verify: `crates/forge/src/codegen.rs:7248-7308`
- Verify: `crates/sindr/src/runtime.rs:171-236`
- Verify: `crates/eldr/src/vm.rs:3910-3941, 5209-5212`
- Verify: `crates/rune/src/compile.rs:324-417, 556-636`
- Modify only if verification proves needed: relevant tests or stale comments/rustdoc in the files above.

**Interfaces:**
- Consumes: the complete resolver/type-checker/analysis implementation.
- Produces: evidence that the compile-space registry does not perturb runtime type tags, bytecode format, process boot metadata, or CLI phase reporting.

- [ ] **Step 1: Run formatting and static checks**

  Run:

  ```bash
  cargo fmt --check
  cargo check --workspace
  ```

  Expected: both PASS. If formatting fails, run `cargo fmt`, inspect the diff, and repeat `cargo fmt --check`.

- [ ] **Step 2: Run the affected pipeline tests**

  Run:

  ```bash
  cargo nextest run -p sindr
  cargo nextest run -p sigil
  cargo nextest run -p scar
  cargo nextest run -p surtr-analysis
  cargo nextest run -p rune --test integration module_import_fixtures
  cargo nextest run -p rune --test integration run_srt
  ```

  Expected: PASS. Record any pre-existing failure separately with its exact command and output; do not weaken collision tests to accommodate it.

- [ ] **Step 3: Run the required full gate**

  Run: `cargo nextest run --workspace`

  Expected: PASS. This checks that Forge/Eldr continue receiving the same typed type definitions and `TypeRegistry` tags, because `OwnerRegistry` stays compile-space only.

- [ ] **Step 4: Perform the final spec/implementation audit**

  Run:

  ```bash
  rg -n "ConcreteError|TypeIdentity::(Type|Struct|Record|Enum|Error|Mod|Supervisor|Sig|Const|Trait|TypeConstructor)" \
    crates doc docs tests
  git diff --check
  git status --short
  ```

  Confirm no compile-space `ConcreteError` spelling remains; runtime/internal `TypeKind::ConcreteError` is allowed. Confirm every direct owner declaration has exactly one registry insertion path and that no `Defmod`/process root has been inserted into `Scope` merely for identity metadata.

- [ ] **Step 5: Commit final verification-only fixes if any were necessary**

  ```bash
  git add -A
  git commit -m "test: verify TypeIdentity owner registry integration"
  ```

## Plan Self-Review

- **Spec coverage:** Task 1 covers all 11 identities, uniqueness scope, the concrete `defrecord Hoge` / `defmod Hoge` bug, trait promotion, and owner-derived members. Tasks 2–6 map the policy through Sindr, Sigil, Scar, and semantic tooling. Task 7 supplies resolver and fixture regressions; Task 8 confirms unchanged Forge/Eldr/Rune behavior.
- **Namespace safety:** The plan uses an adjacent `OwnerRegistry`, not `DeclarationIndex` keys or `Scope`, so module ownership becomes unique without turning module paths into value bindings or destabilizing UIDs/imports.
- **Type consistency:** `OwnerEntry.identity` is the only source for non-builtin owner identity. `DeclarationKind::Trait` remains distinct from `TypeIdentity::TypeConstructor`, and `TypeKind::ConcreteError` maps to `TypeIdentity::Error` without changing runtime error representations.
- **No deferred work:** Each identity mapping, collision direction, diagnostic fragment, source span expectation, and test command is specified above.
