# Type Constructor Signature Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ReturnTypeArgument、Trait method型リスト、TypeCtorTrait family、callable signature、診断を一つの構造化経路へ更改し、その修正完了をゲートとして`do` intrinsicを追加する。

**Architecture:** SpireからForgeまで、定義側／呼び出し側の型入力をrole付き構造データとして保持し、Scarの一つのconstraint・Trait applicability・callable instantiation経路で解く。診断はphase固有errorがreason、origin、typed dataを直接持ち、AriadneとJSONを同じ入力から生成する。修正フェーズで旧`FunParams`、名前・文字列・登録順依存を撤去した後だけ、compiler-owned `DoIntrinsicContract`を使う`do`実装へ進む。

**Tech Stack:** Rust workspace、Spire / Sigil / Scar / Forge / Eldr compiler pipeline、Sindr metadata、Ariadne diagnostics、Serde JSON、`cargo nextest`、Surtr fixture tests。

**Spec:**

- [`return_type_argument_rules.md`](return_type_argument_rules.md)
- [`trait_method_type_list_dispatch.md`](trait_method_type_list_dispatch.md)
- [`signature_diagnostics_unification.md`](signature_diagnostics_unification.md)
- [`do_intrinsic_spec.md`](do_intrinsic_spec.md)
- [`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)
- [`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)

## Global Constraints

- このファイルは実装順序、担当境界、検証ゲートの起点である。個別の意味規則は上記4仕様を正本とし、重複記述が衝突した場合は個別仕様を優先して本計画を修正する。
- `doc/signature_level_type_constructor_inference_draft.md`および未追跡draftを仕様根拠として読み戻さない。
- `2026-08-20-type-identity-owner-registry.md`の作業は完了済みであり、本計画へ再導入しない。
- 修正フェーズを完了するまで`do`のlexer、parser、AST、resolver、type checker、lowering、stdlib宣言を追加しない。
- `FunParams` / `fun_params` / `fun_param`を型入力とvalue parameterのどちらの内部語彙にも残さない。定義側とcall-siteの`::<...>`は`ReturnTypeArgument(s)`、`(...)`内は`ValueParameter(s)`へ置換し、compatibility aliasや二重fieldを作らない。
- Trait定義／implの型形状指定、TypeCtorTrait implのslot mapping、coherence / parent coverageの量化は変更しない。
- 未確定型、carrier、dispatchをimpl個数、builtin既定型、登録順、表示名から逆決定しない。
- non-intrinsic builtinの追加・変更起点は`crates/sindr/src/builtin.rs`のmetadataだけとし、source declarationは構造検証とdocs / provenanceを担う。
- phase固有の`ParseError` / `ResolveError` / `TypeError` / `CodegenError` / `RuntimeError`を維持する。
- user-facing診断はreason、origin、closed typed dataから生成し、message / label / hintやsource textを再解析しない。
- Forgeには具体化済みtype、callable、`TraitDispatchTarget`だけを渡す。runtime dictionary、runtime candidate lookup、do専用opcodeは追加しない。
- 各semantic taskは失敗テスト、最小実装、focused test、commitの順で行う。広範囲変更の最終ゲートは`cargo nextest run --workspace`とする。
- `crates/xldr/**`のREPL経路を変更または手動確認するtaskでは、iTerm2プロファイル`Codex`のtmux sessionを使い、開始時に`tmux attach -t surtr-repl`、終了時に`Ctrl-b`→`d`を利用者へ提示する。
- 実装開始時は`superpowers:using-git-worktrees`で隔離worktreeを作り、各taskは記載されたcommit単位を守る。

---

## Execution Order and Gates

| phase | tasks | exit gate |
|---|---|---|
| 0. 確定仕様 | completed prerequisite | commits `904130f5`, `e5e30679`, `9385cacb`, `7793c33d`, `e12ff5ce`, `2bc68a44`が存在する |
| A. 語彙・構造化基盤 | 1--3 | canonical signatureとstructured envelopeがadapterを通り、未移行payloadが明示分離される |
| B. 型入力・dispatch修正 | 4--8 | RTA、Trait type list、carrier、dispatchが正本どおりでForgeにpendingがない |
| C. 診断統合・修正完了 | 9--10 | SafeBind是正、heuristic撤去、旧語彙ゼロ、focused + workspace test成功 |
| D. `do`追加 | 11--15 | `do` surface、推論、SafeBind、診断、runtime fixture成功 |
| E. 最終監査 | 16 | 全受け入れ基準、全repo scan、workspace test成功 |

`do`開始ゲートはTask 10の最後に一度だけ判定する。Task 1--9の途中で`Ast::Do`、`Resolved::Do`、`TypedDo`、`DoBlock`、`IntrinsicId::Do`を追加してはならない。

## File Responsibility Map

| responsibility | canonical files |
|---|---|
| surface syntax / canonical vocabulary | `crates/spire/src/ast.rs`, `crates/spire/src/parser/decl.rs`, `crates/spire/src/parser/expr.rs`, `crates/spire/src/parser/ty.rs` |
| resolved signature and source origins | `crates/sigil/src/resolved.rs`, `crates/sigil/src/resolver/declarations.rs`, `crates/sigil/src/resolver/expr.rs`, `crates/sigil/src/resolver/derive.rs` |
| shared runtime / surface / intrinsic metadata | `crates/sindr/src/builtin.rs`, create `crates/sindr/src/signature.rs`, create `crates/sindr/src/intrinsic.rs` |
| callable signature / RTA validation | create `crates/scar/src/checker/signatures.rs`, `crates/scar/src/checker/predeclare.rs`, `crates/scar/src/checker/definitions.rs` |
| structural Trait lists / applicability / instantiation | create `crates/scar/src/checker/trait_selection.rs`, create `crates/scar/src/checker/carriers.rs`, `crates/scar/src/checker/specialize.rs`, `crates/scar/src/checker/types.rs` |
| call constraints / operators / SafeBind | `crates/scar/src/checker/expr.rs`, `crates/scar/src/checker/patterns.rs`, create `crates/scar/src/checker/do_intrinsic.rs`, `crates/scar/src/typed.rs` |
| structured diagnostics | create `crates/diagnostics/src/data.rs`, `crates/diagnostics/src/report.rs`, `crates/diagnostics/src/typecheck.rs`, `crates/diagnostics/src/render.rs`, `crates/scar/src/error.rs` |
| compiler adapters | `crates/rune/src/compile.rs`, `crates/rune/src/error.rs`, `crates/xldr/src/error_display.rs`, `crates/xldr/src/repl/logic/core.rs` |
| codegen boundary | `crates/forge/src/codegen.rs`, `crates/forge/src/lib.rs` |
| stdlib surface | `lib/bootstrap.srt`, `lib/types/special_types.srt`, affected `lib/types/*.srt` |
| focused regression tests | parser / resolver unit tests, create focused Scar integration tests under `crates/scar/tests/`, diagnostics tests, Rune fixtures under `tests/fixtures/` |

新規moduleは責任単位で作る。`checker/mod.rs`、`checker/predeclare.rs`、`checker/expr.rs`へ新しいsignature、selection、do実装を積み増さず、上表のmoduleへ移して公開範囲を`pub(super)`に限定する。

## Completed Specification Prerequisite

- [x] 完了済みtype identity owner registryをcommit `904130f5`で記録し、今回の対象外にした。
- [x] Trait用語集と網羅構文一覧をcommit `e5e30679`で`docs/dev/Trait_system_spec.md`へ追加した。
- [x] 置換済みの旧Task 6 / Task 7レビュー資料をcommit `9385cacb`で整理した。
- [x] ReturnTypeArgument仕様をcommit `7793c33d`で確定した。
- [x] Trait method型リスト／dispatch仕様をcommit `e12ff5ce`で確定した。
- [x] シグネチャ診断統一仕様と`do` intrinsic仕様をcommit `2bc68a44`で確定した。

---

### Task 1: Canonical ReturnTypeArgument and ValueParameter Vocabulary

**Files:**

- Modify: `crates/spire/src/ast.rs`
- Modify: `crates/spire/src/parser/decl.rs`
- Modify: `crates/spire/src/parser/expr.rs`
- Modify: `crates/spire/src/parser/mod.rs`
- Modify: `crates/spire/src/parser/tolerant.rs`
- Modify: `crates/spire/src/parser/tests.rs`
- Modify: `crates/sigil/src/resolved.rs`
- Modify: `crates/sigil/src/semantic_metadata.rs`
- Modify: `crates/sigil/src/resolver/{declarations,derive,expr,mod}.rs`
- Modify: `crates/sigil/src/resolver/tests.rs`
- Modify: `crates/scar/src/checker/{definitions,expr,mod,predeclare,specialize,types}.rs`
- Modify: `crates/scar/src/typed.rs`
- Modify: `crates/scar/tests/{typecheck_surface,warnings}.rs`
- Modify: `crates/forge/src/{codegen,lib}.rs`
- Modify: `crates/xldr/src/repl/logic/core.rs`
- Modify: `doc/要件定義v9.md`, `docs/dev/テスト方針.md`, `docs/site/{language-reference,trait-impls,trait-system}.md`

**Interfaces:**

- Produces: `ReturnTypeArgument`, `return_type_arguments`, `call_site_return_type_arguments`, `ReturnTypeArgumentApply`, `ValueParameter`, `ValueParameterMode`, `ResolvedValueParameter`, `TypedValueParameter`.
- Consumes: existing `AstTy`, `Span`, definition and call-site `::<...>` tokens.
- Invariant: no compatibility aliases or duplicate old/new fields; an AST node always records the syntactic role of its type list.

- [x] **Step 1: Add failing parser and resolver tests for canonical roles**

```rust
match parsed.as_slice() {
    [Ast::Def(_, _, return_type_arguments, value_parameters, ..)] => {
        assert_eq!(return_type_arguments.len(), 1);
        assert_eq!(value_parameters.len(), 1);
    }
    other => panic!("expected definition with canonical parameter roles: {other:?}"),
}

assert!(matches!(
    call,
    Ast::ReturnTypeArgumentApply(_, _, ref args) if args.len() == 1
));
```

- [x] **Step 2: Run the focused tests and record the expected failure**

Run:

```bash
cargo nextest run -p spire
cargo nextest run -p sigil
```

Expected: compilation or assertions fail because the canonical types, fields, and variant do not exist.

- [x] **Step 3: Replace AST and resolved vocabulary atomically**

Use these exact role-bearing fields as the target and update every constructor/pattern match atomically without retaining aliases:

```rust
pub struct ReturnTypeArgument {
    pub ordinal: u32,
    pub ty: AstTy,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueParameterMode {
    PositionalOrNamed,
    Variadic,
}

pub struct ValueParameter {
    pub name: Symbol,
    pub mode: ValueParameterMode,
    pub ty: AstTy,
    pub span: Span,
}

pub enum Ast {
    ReturnTypeArgumentApply(Span, Box<Ast>, Vec<ReturnTypeArgument>),
    Def(Span, Symbol, Vec<ReturnTypeArgument>, Vec<ValueParameter>, Option<AstTy>, Option<WhereClause>, Box<Ast>, DeclAttrs),
    BuiltinDecl(Span, Symbol, Vec<ReturnTypeArgument>, Vec<ValueParameter>, Option<AstTy>, Option<WhereClause>, DeclAttrs),
}

pub struct ResolvedReturnTypeArgument {
    pub ordinal: u32,
    pub ty: AstTy,
    pub span: Span,
}

pub struct ResolvedValueParameter {
    pub id: ResolvedId,
    pub mode: ValueParameterMode,
    pub ty: AstTy,
    pub span: Span,
}

pub struct TypedReturnTypeArgument {
    pub ordinal: u32,
    pub ty: Ty,
    pub span: Span,
}

pub struct TypedValueParameter {
    pub id: ResolvedId,
    pub mode: ValueParameterMode,
    pub ty: Ty,
    pub span: Span,
}
```

`TraitMethodSig` receives `return_type_arguments: Vec<ReturnTypeArgument>` and `value_parameters: Vec<ValueParameter>`. `Resolved::Def` / `BuiltinDecl` and `TypedInner::Def` carry the resolved/typed structures above in the same two positions. Rename the Trait method / impl metadata equivalents at the same time. `DeclAttrs.fun_params`, `ResolvedTraitMethodSig.fun_params`, `ResolvedTraitImplMethod.fun_params`, `TraitMethodInfo.fun_params`, and `TraitImplMethodInfo.fun_params` become `return_type_arguments`; only function/method/builtin parameter fields whose syntax is declaration `(...)` become `value_parameters`. Do not rename closure, extractor, process, or unrelated domain-specific `params` fields.

- [x] **Step 4: Update parser helpers and all exhaustive consumers**

`parse_trait_fun_params*` becomes a shared definition-side ReturnTypeArgument parser usable by ordinary defs, Trait declarations, impl methods, inherent methods, and builtin surface declarations. `parse_fun_param` and shift / rebase helpers become ValueParameter helpers. Call-site parsing replaces `Ast::TypeApply` / `Resolved::TypeApply` with`ReturnTypeArgumentApply` without changing ordinary nominal type application inside`AstTy`.

- [x] **Step 5: Remove old vocabulary from code and current canonical docs**

Run:

```bash
rg -n 'FunParams|fun_params|fun_param|FunParam' \
  crates docs/dev docs/site doc/要件定義v9.md
```

Expected: no matches. Do not scan the four historical implementation-input specs with a replacement script; update them deliberately only if a canonical term is wrong.

- [x] **Step 6: Run phase tests**

```bash
cargo nextest run -p spire
cargo nextest run -p sigil
cargo nextest run -p scar
```

Expected: all pass with only canonical vocabulary.

- [x] **Step 7: Commit**

```bash
git add crates/spire crates/sigil crates/scar crates/forge crates/xldr crates/surtr-analysis docs/dev docs/site doc/要件定義v9.md
git commit -m "refactor(types): replace legacy function parameter vocabulary"
```

---

### Task 2: Structured Diagnostic Contract

**Files:**

- Create: `crates/diagnostics/src/data.rs`
- Modify: `crates/diagnostics/src/lib.rs`
- Modify: `crates/diagnostics/src/report.rs`
- Modify: `crates/diagnostics/src/render.rs`
- Modify: `crates/diagnostics/src/typecheck.rs`
- Modify: `crates/diagnostics/src/tests/typecheck.rs`
- Modify: `crates/diagnostics/src/tests/render_and_source.rs`
- Modify: `docs/dev/diagnostics.md`
- Modify: `crates/scar/Cargo.toml`
- Modify: `crates/scar/src/error.rs`
- Modify: `crates/rune/src/compile.rs`
- Modify: `crates/rune/src/error.rs`
- Modify: `crates/xldr/src/error_display.rs`
- Modify: `crates/xldr/src/repl/logic/core.rs`
- Modify: `crates/xldr/tests/repl_core.rs`
- Modify: `tests/integration/common.rs`
- Modify: `tests/integration/support/phase.rs`

**Interfaces:**

- Produces: closed `TypeDiagnosticReason` and `DiagnosticData`, structured `DiagnosticOrigin`, `SourceFact`, additive JSON `reason` / `origin` / `data` / `related`.
- Consumes: phase-specific error reason code, source spans, canonical rendered type facts.
- Invariant: Ariadne and JSON consume the same object; existing JSON fields remain with their current meaning.

- [x] **Step 1: Write failing serialization and rendering tests**

```rust
let input = StructuredDiagnostic {
    reason: TypeDiagnosticReason::ReturnTypeArgumentMismatch,
    origin: DiagnosticOrigin::ReturnTypeArgument { ordinal: 0 },
    data: DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
        callable: "guard".into(),
        ordinal: 0,
        expected_type: "Option".into(),
        actual_type: "List".into(),
    }),
    primary: SourceFact::typed(SourceRole::ReturnTypeArgument, source_id, slot_span, "Option"),
    related: vec![SourceFact::typed(SourceRole::Value, source_id, value_span, "List<Int>")],
    remediation: None,
};
let spec = structured_type_error_spec(&input);
let json = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
assert_eq!(json.errors[0].reason.as_deref(), Some("ReturnTypeArgumentMismatch"));
assert_eq!(json.errors[0].related.len(), 1);
assert_eq!(json.errors[0].data["ordinal"], 0);
```

- [x] **Step 2: Run diagnostics tests and observe the missing structured fields**

```bash
cargo nextest run -p diagnostics
```

Expected: compilation fails because structured data and additive JSON fields are absent.

- [x] **Step 3: Add closed diagnostic data types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TypeDiagnosticReason {
    ArityMismatch,
    ArgumentModeMismatch,
    UnknownNamedArgument,
    DuplicateArgument,
    MissingArgument,
    ArgumentTypeMismatch,
    ReturnTypeMismatch,
    AnnotationTypeMismatch,
    NotCallable,
    CallableShapeMismatch,
    ReturnTypeArgumentArityMismatch,
    ReturnTypeArgumentMismatch,
    AmbiguousReturnTypeArgument,
    InvalidTraitConstraintSubject,
    MissingGenericBound,
    MissingTraitCapability,
    NoApplicableTraitImplementation,
    UnresolvedTraitMethodInstantiation,
    MissingTraitDispatchTarget,
    MissingTypeConstructorConstraint,
    TraitMethodTypeListMismatch,
    TraitMethodTypeListArityMismatch,
    TraitMethodConstraintMismatch,
    TypeConstructorFamilyMismatch,
    TypePayloadMismatch,
    MissingTypeConstructorCapability,
    DuplicateReturnTypeArgumentInput,
    MissingReturnTypeArgument,
    UnusedReturnTypeArgument,
    ConcreteReturnTypeArgumentInDefinition,
    InlineReturnTypeArgumentConstraint,
    IfBranchTypeMismatch,
    MatchArmTypeMismatch,
    CondBranchTypeMismatch,
}

pub enum DiagnosticData {
    ArgumentRelation(ArgumentRelationData),
    ReturnTypeArgument(ReturnTypeArgumentData),
    ConstraintSubject(ConstraintSubjectData),
    TraitObligation(TraitObligationData),
    TraitDispatch(TraitDispatchData),
    TypeConstructorCarrier(TypeConstructorCarrierData),
    BranchAssertion(BranchAssertionData),
    SafeBindRelation(SafeBindRelationData),
    Policy(PolicyData),
    Runtime(RuntimeData),
}

pub struct StructuredDiagnostic {
    pub reason: TypeDiagnosticReason,
    pub origin: DiagnosticOrigin,
    pub data: DiagnosticData,
    pub primary: SourceFact,
    pub related: Vec<SourceFact>,
    pub remediation: Option<Remediation>,
}

pub struct SourceFact {
    pub role: SourceRole,
    pub source_id: SourceId,
    pub span: Span,
    pub ty: Option<String>,
    pub declaration_identity: Option<DeclarationIdentity>,
}
```

Add the policy, resolve, parse, and runtime reason enums beside this type when their phase is migrated; do not flatten phase ownership into one cross-phase enum. Do not replace any reason or data enum with `String` or `HashMap<String, String>`.

- [x] **Step 4: Extend phase TypeError without changing phase ownership**

`scar::TypeError` remains the typecheck error type and gains reason, origin, typed data, and related facts. Add a `diagnostics` dependency to Scar for the shared envelope. Existing not-yet-migrated errors travel in a separate explicitly unstable legacy payload with no stable `reason`; do not add `UnmigratedMessage`, and do not infer a reason from legacy text. Tasks 4--10 eliminate that payload family by family.

- [x] **Step 5: Make Rune and Xldr pass structured input through**

Replace every `TypeErrorDiagnostic::new(e.message, e.span, e.hint)` construction in Rune and Xldr with a conversion that localizes and preserves reason/data/facts, including multi-source related spans. Render templates select message, labels, notes, and help from the reason and optional remediation overlay.

Because this task changes the Xldr REPL path, run it in the required iTerm2 profile `Codex` tmux session, present `tmux attach -t surtr-repl`, and present `Ctrl-b` then `d` for detach. Compare one type error in Rune and Xldr before committing.

- [x] **Step 6: Verify human and JSON output share facts**

```bash
cargo nextest run -p diagnostics
cargo nextest run -p rune --test integration run_srt
```

Expected: tests pass; JSON old fields remain and new fields serialize additively.

- [x] **Step 7: Commit**

```bash
git add crates/diagnostics crates/scar crates/rune/src crates/xldr tests/integration docs/dev/diagnostics.md
git commit -m "refactor(diagnostics): add structured diagnostic contract"
```

---

### Task 3: Canonical Callable Signatures and Builtin Surface Variants

**Files:**

- Create: `crates/sindr/src/signature.rs`
- Modify: `crates/sindr/src/lib.rs`
- Modify: `crates/sindr/src/builtin.rs`
- Modify: `crates/sigil/src/resolver/declarations.rs`
- Create: `crates/scar/src/checker/signatures.rs`
- Modify: `crates/scar/src/checker/mod.rs`
- Modify: `crates/scar/src/checker/definitions.rs`
- Modify: `crates/scar/src/checker/expr.rs`
- Modify: `crates/scar/src/checker/predeclare.rs`
- Modify: `crates/eldr/src/builtin.rs`
- Test: `crates/sindr/src/builtin.rs` unit tests, `crates/scar/tests/typecheck_surface.rs`

**Interfaces:**

- Produces: `CallableSignature`, `CanonicalValueParameter`, runtime builtin entry with one or more canonical surface variants.
- Consumes: resolved definitions, builtin metadata, canonical where constraints.
- Invariant: parameter name / mode / ordinal are never dropped; a shared`BuiltinId`does not collapse distinct surface callable identities.

- [ ] **Step 1: Add failing metadata parity tests**

```rust
let meta = builtin_meta_by_runtime_name("safe_div").unwrap();
let int = meta.surface_variant("Int", "safe_div").unwrap();
let float = meta.surface_variant("Float", "safe_div").unwrap();
assert_eq!(int.value_parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
assert_eq!(float.value_parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
assert_eq!(int.value_parameters.len(), meta.runtime_arity as usize);
assert_eq!(float.value_parameters.len(), meta.runtime_arity as usize);
assert_eq!(int.runtime_target, float.runtime_target);
```

In the Scar test, resolve one user function, one Trait helper, and one non-intrinsic builtin with the same RTA / value / return / where shape and assert that all three enter `check_callable_application` as `CallableSignature`; the only differing field is `runtime_target`.

- [ ] **Step 2: Run focused tests and capture the current failure**

```bash
cargo nextest run -p sindr
cargo nextest run -p scar
```

Expected: metadata cannot represent multiple complete surface variants and Scar still uses builtin-specific contracts.

- [ ] **Step 3: Introduce complete signature metadata**

```rust
pub struct CanonicalValueParameter<T> {
    pub ordinal: u32,
    pub name: String,
    pub mode: ValueParameterMode,
    pub ty: T,
    pub origin: SignatureOrigin,
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuiltinId(pub u16);

pub struct CallableSignature<T> {
    pub identity: CallableIdentity,
    pub return_type_arguments: Vec<CanonicalReturnTypeArgument<T>>,
    pub value_parameters: Vec<CanonicalValueParameter<T>>,
    pub return_type: CanonicalTypeOccurrence<T>,
    pub where_constraints: CanonicalConstraintSet<T>,
    pub runtime_target: RuntimeTarget,
    pub declaration_origins: Vec<SignatureOrigin>,
}
```

Sindr stores parseable surface metadata; Scar resolves it once into canonical types. Do not use raw`sig_str`as the runtime checker contract.

- [ ] **Step 4: Split runtime entries from surface variants**

Each runtime entry preserves definition-order`BuiltinId`and effect/runtime metadata. Its surface variants carry owner, name, RTA, parameter name/mode/type/ordinal, return, and where. Validate every`@builtin def`against a complete variant and attach the shared runtime ID only after a full match.

- [ ] **Step 5: Move all ordinary callables to one registry and call checker**

Remove `BuiltinContract`, `builtin_contracts`, `check_builtin_contract`, and `Ty::BuiltinFunc`-specific type-relation/call-obligation branches after user functions, Trait helpers, and non-intrinsic builtins build the same `CallableSignature`. A callable/runtime-target representation may retain a builtin variant, but it must not own an alternate type-inference route. Keep intrinsic policy and runtime target selection separate.

- [ ] **Step 6: Verify metadata, typecheck, and runtime order**

```bash
cargo nextest run -p sindr
cargo nextest run -p sigil
cargo nextest run -p scar
cargo nextest run -p eldr
cargo nextest run -p rune --test integration run_srt
```

Expected: complete surface parity passes and`BUILTIN_METAS`order still matches Eldr runtime implementations.

- [ ] **Step 7: Commit**

```bash
git add crates/sindr crates/sigil crates/scar crates/eldr
git commit -m "refactor(types): unify callable and builtin signatures"
```

---

### Task 4: ReturnTypeArgument Definition Rules

**Files:**

- Modify: `crates/spire/src/parser/decl.rs`
- Modify: `crates/spire/src/parser/validate.rs`
- Modify: `crates/sigil/src/resolver/declarations.rs`
- Modify: `crates/scar/src/checker/signatures.rs`
- Modify: `crates/scar/src/checker/predeclare.rs`
- Create: `crates/scar/tests/return_type_arguments.rs`
- Add parse fixtures: `tests/fixtures/script/fail/parse/return_type_argument_{empty,duplicate,concrete,inline_constraint}.{srt,error}`
- Add typecheck fixtures: `tests/fixtures/script/fail/typecheck/return_type_argument_{missing,duplicate_input,unused,missing_constructor_constraint,invalid_constraint_subject}.{srt,error}`

**Interfaces:**

- Produces: recursive occurrence classification and definition well-formedness reasons.
- Consumes: canonical callable signature from Task 3.
- Invariant: value-parameter inputs and return-only inputs are disjoint introduction channels;`where`never introduces an unknown type input.

- [x] **Step 1: Write failing definition tests**

Cover these exact cases:

```surtr
def make::<$A>() -> $A { panic("test") }
def missing(value: Int) -> $B { panic("test") }
def duplicate::<$F>(value: $F<$A>) -> $F<$A> where $F: Functor { value }
def unused::<$A>() -> Int { 0 }
def zeros::<List<$T>>() -> List<$T> { [] }
def invalid(value: $F<$A>) -> $F<$A> { value }
def stop::<$F: Monad>() -> $F<Unit> { panic("test") }
def invalid_subject::<$F>() -> $F<Unit> where Applicative: Add { panic("test") }
```

Assert distinct structured reasons: `MissingReturnTypeArgument`, `DuplicateReturnTypeArgumentInput`, `UnusedReturnTypeArgument`, `ConcreteReturnTypeArgumentInDefinition`, `MissingTypeConstructorConstraint`, `InlineReturnTypeArgumentConstraint`, and `InvalidTraitConstraintSubject`. Spire owns empty/duplicate/concrete/inline-list shape failures; Scar owns occurrence and constraint-subject failures after resolution.

- [x] **Step 2: Run the focused test**

```bash
cargo nextest run -p scar --test return_type_arguments
```

Expected: failures show ordinary defs cannot yet retain RTA and current code rejects return-only slots generically.

- [x] **Step 3: Implement recursive occurrence collection**

```rust
pub struct SignatureOccurrences {
    pub argument_inputs: BTreeMap<TypeInputId, Vec<SourceOrigin>>,
    pub return_inputs: BTreeMap<TypeInputId, Vec<SourceOrigin>>,
    pub declared_return_type_arguments: BTreeMap<TypeInputId, SourceOrigin>,
}
```

Walk tuples, functions, named applications, direct TypeCtorTrait applications, and nested constructor variables. Preserve both origins for duplicate-input diagnostics.

- [x] **Step 4: Implement context-sensitive definition validation**

Ordinary / abstract method definitions accept only abstract RTA inputs. Trait impl methods may contain the contract-substituted concrete structure and defer equality to the role-list task. Parse inline constraints into a rejected RTA item so `InlineReturnTypeArgumentConstraint` can point at the bound, rather than falling through to a generic token error. Reject invalid Trait-name subjects at their subject spans.

- [x] **Step 5: Normalize direct TypeCtorTrait syntax**

`def guard::<Alternative>(...) -> Alternative<Unit>`becomes one fresh constructor variable plus`where $F: Alternative`. `$F<$A>`is legal only when the same function`where`contains a TypeCtorTrait bound. Keep position-local capability views and same-family carrier identity separate.

- [x] **Step 6: Run focused and fixture tests**

```bash
cargo nextest run -p spire
cargo nextest run -p sigil
cargo nextest run -p scar --test return_type_arguments
cargo nextest run -p rune --test integration run_srt
```

- [x] **Step 7: Commit**

```bash
git add crates/spire crates/sigil crates/scar tests/fixtures
git commit -m "feat(types): validate return type argument declarations"
```

---

### Task 5: ReturnTypeArgument Call-Site Constraint Solving

**Completion repair plan (2026-09-06):**

- [x] Reproduce the bare `Convert` and nested `Marker` capability failures. In `expr.rs`, discharge canonical obligations with `ty_satisfies_bounds(&subject, std::slice::from_ref(trait_key))`; remove the duplicate `tyvar_has_bound` call. Bare families and full dispatch keys keep their existing distinct semantics.
- [x] Extend `forwards_return_only_where_obligation_through_outer_generic_bound` in `return_type_arguments.rs` to direct, `if`, and `match` tails. Propagate declared expected results through the relevant branch tails in `definitions.rs`.
- [x] Audit ordinary specialization to consume `call_substitution` without reconstructing value-pair mappings; retain trait-method inference until Task 7. Preserve predeclared input identities before checking ordinary definitions so recursive and forward calls retain valid substitution keys; reject missing keys instead of reconstructing them.
- [x] Cover generic capture forwarding (omitted, explicit, underscore), true ambiguity, and known closure return shapes in `return_type_argument_capture_forwarding.rs`. Propagate expected types to capture tails and closure bodies; accept enclosing rigid inputs as witnesses. Extend the capture script fixture to execute both returned callable paths.
- [x] Run Scar, Forge, Rune script/module fixtures and `language_features_bucket_0`, formatting and diff checks; run the CI workspace gate for the multi-phase Task 5 diff before committing.
- [x] Record results and commit the scoped Task 5 changes, including the existing implementation and fixtures.

**Completion verification (2026-09-06):**

- Reproduced both bare-capability failures before the fix; `cargo test -p scar --test typecheck_surface` then passed all 104 tests.
- `cargo test -p scar` passed. Added RED-to-GREEN coverage for branch proof forwarding, missing ordinary call substitution, generic capture forwarding, and known closure return shape.
- `cargo test -p rune --test integration run_srt`: 28 passed. `cargo test -p rune --test integration module_import_fixtures`: 10 passed.
- Final tree: `rtk cargo nextest run --profile ci --workspace --test-threads 4`: **2,004 passed**, 32 binaries, 477.537 seconds, no failures/timeouts. This includes Scar, Forge, all updated script fixtures, modules, and `language_features::language_features_bucket_0`, plus cold CLI and REPL coverage.
- `cargo fmt --all -- --check` and `git diff --cached --check`: passed. Independent review completed with its capture findings fixed and verified.
- Sigil's existing capture traversal already preserves `ReturnTypeArgumentApply`; no additional resolver edit was needed. Forge changes only update typed test data for `call_substitution`.

**Files:**

- Modify: `crates/scar/src/checker/signatures.rs`
- Modify: `crates/scar/src/checker/expr.rs`
- Modify: `crates/scar/src/checker/types.rs`
- Modify: `crates/scar/src/checker/specialize.rs`
- Modify: `crates/sigil/src/resolver/captures.rs`
- Modify: `lib/kernel.srt`
- Modify tests: `crates/scar/tests/return_type_arguments.rs`, `crates/scar/tests/typecheck_surface.rs`
- Add pass fixtures: `tests/fixtures/script/pass/functions/return_type_argument_{explicit,inferred_from_value,inferred_from_expected,underscore,capture}.{srt,expected}`
- Add fail fixtures: `tests/fixtures/script/fail/typecheck/return_type_argument_{arity,mismatch,ambiguous,captured_argument_ambiguous}.{srt,error}`

**Interfaces:**

- Produces: one constraint set joining call-site RTA, value arguments, expected return, closure shape, and callable obligations; `Solved` / `Deferred` / `Failed`.
- Consumes: Task 3 callable signatures and Task 4 definition validation.
- Invariant: omission equals an all-underscore list; an explicit list has exact arity; a failed probe rolls back all bindings.

- [x] **Step 1: Add failing inference tests**

```surtr
number = try_from::<Int>("42")
value: Option<Unit> = guard(True)
value = guard::<Option>(True)
partial = choose::<_, Int>()
ambiguous = guard(True)
```

Test every pair among explicit RTA, value argument, and expected return for agreement and conflict. Test callable capture, omitted list versus all`_`, arity underflow / overflow, constructor head with unresolved captured argument, and impl-order reversal.

- [x] **Step 2: Run focused tests**

```bash
cargo nextest run -p scar --test return_type_arguments
```

Expected: ordinary callable application is rejected or resolved through old special routes.

- [x] **Step 3: Build a single call constraint object**

```rust
pub(super) struct CallConstraintSet {
    pub signature: CallableSignature<CanonicalTy>,
    pub return_type_arguments: Vec<TypeConstraint>,
    pub value_arguments: Vec<ValueArgumentConstraint>,
    pub expected_return: Option<TypeConstraint>,
    pub obligations: Vec<TraitObligation>,
    pub origins: Vec<ConstraintOrigin>,
}

pub(super) enum SolveState<T> {
    Solved(T),
    Deferred(PendingConstraints),
    Failed(TypeError),
}
```

- [x] **Step 4: Replace`check_explicit_type_apply`special cases**

`ReturnTypeArgumentApply`wraps any non-intrinsic callable. Build the same constraints for explicit, underscore, and omitted forms. Do not partially zip an arity mismatch. Re-home every unresolved variable and pending obligation when another variable solves.

- [x] **Step 5: Feed the completed substitution into specialization**

Replace the ordinary-call portion of `infer_specialization_mapping` that reconstructs mappings from value argument pairs. The specialization input is the solved call substitution, including return-only inputs and expected return. Audit body-free variables at the callable-instantiation boundary. Keep the Trait-method portion until Task 7 supplies `TraitMethodInstantiation.substitution`, then delete it there.

Add the standard ordinary function to `Kernel`; `Bootstrap` is an earlier loader stage and cannot depend on `Alternative` / `Applicative`. Its direct Trait RTA is normalized by Task 4 and all internal/user calls use this task's common solver:

```surtr
def guard::<Alternative>(condition: Boolean) -> Alternative<Unit> {
  if(condition, Applicative::pure(()), Alternative::empty())
}
```

Add its final `@doc` in the same edit. Do not register `guard` as an intrinsic/builtin and do not branch on its name in Scar or the later do checker.

- [x] **Step 6: Verify inference and regression fixtures**

```bash
cargo nextest run -p scar --test return_type_arguments
cargo nextest run -p scar
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
```

- [x] **Step 7: Commit**

```bash
git add crates/scar crates/forge lib/kernel.srt tests/fixtures doc/type_constructor_signature_unification_implementation_plan.md
git commit -m "feat(types): solve return type arguments at call sites"
```

---

### Task 6: Role-Based Trait Method Contract Validation

**Files:**

- Create: `crates/scar/src/checker/trait_selection.rs`
- Modify: `crates/scar/src/checker/mod.rs`
- Modify: `crates/scar/src/checker/predeclare.rs`
- Modify: `crates/scar/src/checker/definitions.rs`
- Modify: `crates/scar/src/checker/types.rs`
- Modify: `crates/scar/src/typed.rs`
- Create: `crates/scar/tests/trait_method_type_lists.rs`
- Add fixtures: `tests/fixtures/script/fail/typecheck/trait_method_type_list_arity.{srt,error}`, `tests/fixtures/script/fail/typecheck/trait_method_type_list_nested_mismatch.{srt,error}`, `tests/fixtures/script/fail/typecheck/trait_method_constraint_mismatch.{srt,error}`

**Interfaces:**

- Produces: `TypeListRole`, `TypeListEntry`, `ImplHeadTypeList`, `MethodSignatureTypeList`, and structural contract-validation failures.
- Consumes: canonical types and RTA/value-parameter roles from Tasks 1--5.
- Invariant: Trait arguments, impl target, RTA, value parameters, and return type retain role and ordinal; nested types stay recursive rather than becoming extra list entries.

- [x] **Step 1: Add failing contract-equivalence tests**

Cover alpha-renamed generics, repeated-variable preservation, nested nominal/tuple/function types, `Self` and `Self<$A>`, RTA arity, value-parameter arity, and canonical `where` set equality. Include a mismatch whose only difference is the return type and assert:

```rust
assert_eq!(error.reason(), TypeErrorReason::TraitMethodTypeListMismatch);
assert_eq!(error.data().type_list_role(), TypeListRole::ReturnType);
assert_eq!(error.data().ordinal(), 0);
assert_eq!(error.data().nested_path(), &[0]);
assert_eq!(error.data().expected_type(), "Box<Int>");
assert_eq!(error.data().actual_type(), "Box<String>");
```

- [x] **Step 2: Run the focused test and preserve the failing output**

```bash
cargo nextest run -p scar --test trait_method_type_lists
```

Expected: current contract comparison either loses role information or compares source generic names / partial shapes.

- [x] **Step 3: Add role-preserving type-list structures**

```rust
pub enum TypeListRole {
    TraitArgument,
    ImplTarget,
    ReturnTypeArgument,
    ValueParameter,
    ReturnType,
}

pub struct TypeListEntry {
    pub role: TypeListRole,
    pub ordinal: u32,
    pub ty: CanonicalTy,
    pub origin: TypeOrigin,
}

pub struct MethodSignatureTypeList {
    pub entries: Vec<TypeListEntry>,
    pub where_constraints: CanonicalConstraintSet,
}

pub struct StructuralTypePath {
    pub role: TypeListRole,
    pub ordinal: u32,
    pub nested_arguments: Vec<u32>,
}
```

Build one list in the stable order `ReturnTypeArgument`, `ValueParameter`, `ReturnType`; build impl-head lists separately from `TraitArgument` followed by `ImplTarget`.

- [x] **Step 4: Validate contract and impl in one fresh environment**

Freshen all contract variables together, substitute Trait arguments and `Self`, then compare the impl method list in a second fresh namespace. Preserve repeated-variable relationships and perform an occurs check. Canonicalize method `where` constraints in that same environment and compare them as order-independent sets.

- [x] **Step 5: Replace old method-shape comparisons**

Remove comparisons based on rendered type strings, source generic identifiers, independently freshened parameter/return fragments, or value-parameter-only zips. Emit `TraitMethodTypeListArityMismatch`, `TraitMethodTypeListMismatch`, or `TraitMethodConstraintMismatch` with contract and impl origins.

- [x] **Step 6: Run focused and phase tests**

```bash
cargo nextest run -p scar --test trait_method_type_lists
cargo nextest run -p scar
cargo nextest run -p rune --test integration run_srt
```

- [x] **Step 7: Commit**

```bash
git add crates/scar tests/fixtures
git commit -m "refactor(types): validate trait methods with role type lists"
```

---

### Task 7: Canonical Trait Implementation Applicability

**Files:**

- Modify: `crates/scar/src/checker/trait_selection.rs`
- Modify: `crates/scar/src/checker/predeclare.rs`
- Modify: `crates/scar/src/checker/expr.rs`
- Modify: `crates/scar/src/checker/types.rs`
- Modify: `crates/scar/src/checker/specialize.rs`
- Modify: `crates/scar/src/typed.rs`
- Modify: `crates/sigil/src/resolved.rs`
- Modify: `crates/sigil/src/resolver/declarations.rs`
- Add tests: `crates/scar/tests/trait_impl_applicability.rs`
- Add fixtures: `tests/fixtures/script/pass/functions/trait_impl_generic_box.{srt,expected}`, `tests/fixtures/script/fail/typecheck/trait_impl_no_applicable_full_head.{srt,error}`, `tests/fixtures/script/fail/typecheck/trait_impl_where_unsatisfied.{srt,error}`

**Interfaces:**

- Produces: `CanonicalTraitImplPatternKey`, `TraitImplDeclarationKey`, `RequestedHeadTypeList`, checkpointed candidate probing, and `Applicable` / `Deferred` / `Rejected`.
- Consumes: canonical Trait identity, resolved impl head, role type lists, and proof obligations.
- Invariant: `TraitId` index narrows candidates only; it never decides applicability, priority, or a default implementation.

- [x] **Step 1: Add failing applicability tests**

Cover nested impl heads, Trait arguments plus impl target, impl `where` success/failure/deferred, reversed declaration order, generic overlap rejection, and a single visible impl with an unknown subject. Assert that the unknown case stays deferred:

```rust
assert!(matches!(
    selector.probe(&unknown_obligation),
    TraitSelection::Deferred { waiting_on, .. } if !waiting_on.is_empty()
));
```

Add a rollback regression where the first candidate binds two variables and then fails its `where`; the second candidate must observe neither binding.

- [x] **Step 2: Run the focused test**

```bash
cargo nextest run -p scar --test trait_impl_applicability
```

Expected: name/string keys, registration order, or leaked candidate bindings produce wrong selection.

- [x] **Step 3: Store canonical impl identity**

```rust
pub struct CanonicalTraitImplPatternKey {
    pub trait_ref: CanonicalTraitRef,
    pub target: CanonicalTy,
}

pub struct TraitImplDeclarationKey {
    pub pattern: CanonicalTraitImplPatternKey,
    pub declaration_id: DeclarationId,
}

pub enum CandidateApplicability {
    Applicable(MethodInstantiation),
    Deferred(PendingTraitCandidate),
    Rejected(CandidateRejection),
}
```

Keep declaration identity for provenance and body lookup, not as a substitute for structural identity. Store canonical impl heads as the source of truth and use a `TraitId` secondary index only for discovery.

- [x] **Step 4: Probe the complete candidate in one checkpoint**

In order, unify the impl head with the requested head, apply that substitution to impl `where`, unify the method signature list with the invocation list, then prove all obligations. Commit the checkpoint only for a fully applicable candidate; retain explicit waiting variables for deferred candidates; roll back every pattern, inference, carrier, and proof binding for rejected candidates.

- [x] **Step 5: Remove implicit selection routes**

Delete storage/lookups keyed by `(trait_name, rendered_target)`, parsing of rendered `Trait<...>` strings, exact-name dispatch shortcuts, single-candidate defaulting, and registration-order tie breaking. A concrete input with multiple applicable candidates is an internal coherence invariant; an unresolved input with candidates is deferred until its boundary.

- [x] **Step 6: Verify storage, selection, and modules**

```bash
cargo nextest run -p scar --test trait_impl_applicability
cargo nextest run -p scar
cargo nextest run -p rune --test integration module_import_fixtures
```

- [x] **Step 7: Commit**

```bash
git add crates/scar crates/sigil crates/xldr/src/lib.rs tests/fixtures doc/type_constructor_signature_unification_implementation_plan.md
git commit -m "refactor(types): select trait implementations structurally"
```

**Completion verification (2026-09-06):**

- Applicability, rejected-candidate isolation, rigid caller capability/impl boundaries, exact method substitution, and original declaration dispatch regressions passed. Independent review completed after fixing its rigid-variable findings.
- Final source tree with a warmed fresh stdlib cache: `rtk cargo nextest run --workspace --test-threads 4` ran **1,853 tests: 1,852 passed, 1 timed out, 202 skipped** in 304.396 seconds. There were no functional test failures; this workspace invocation did not exit successfully because of the timeout.
- The only timeout, `xldr::repl_core::core_reload_and_clear_commands_preserve_only_requested_state`, passed unchanged in an exact isolated rerun: **1 passed, 133 skipped**, 9.356 seconds, exit 0. Together these runs verified every selected test. The default profile excludes cold tests; those 202 skipped tests were not covered by this final gate.
- Fresh-cache Rune bucket 4 passed before the workspace run. `cargo fmt --all` and `git diff --check` passed after the final source change.
- Task 8's full invocation/carrier propagation and Task 9's complete diagnostic rendering remain separate follow-up tasks.

---

### Task 8: Unified Method Instantiation and TypeCtorTrait Carriers

**Files:**

- Create: `crates/scar/src/checker/carriers.rs`
- Modify: `crates/scar/src/checker/mod.rs`
- Modify: `crates/scar/src/checker/trait_selection.rs`
- Modify: `crates/scar/src/checker/specialize.rs`
- Modify: `crates/scar/src/checker/types.rs`
- Modify: `crates/scar/src/checker/expr.rs`
- Modify: `crates/scar/src/typed.rs`
- Modify: `crates/sigil/src/resolver/derive.rs`
- Modify: `crates/sindr/src/builtin.rs`
- Modify: `crates/forge/src/codegen.rs`
- Modify: `crates/forge/src/lib.rs`
- Create tests: `crates/scar/tests/trait_method_instantiation.rs`, `crates/scar/tests/type_constructor_carriers.rs`
- Add fixtures: `tests/fixtures/script/pass/functions/type_constructor_family_payload_change.{srt,expected}`, `tests/fixtures/script/fail/typecheck/type_constructor_family_carrier_mismatch.{srt,error}`, `tests/fixtures/script/fail/typecheck/type_constructor_family_captured_mismatch.{srt,error}`

**Interfaces:**

- Produces: `TypeCtorTraitFamilyId`, `CanonicalConstructorCarrier`, `TraitMethodInstantiation`, complete `CallableInstantiationKey`, and concrete `TraitDispatchTarget`.
- Consumes: the single substitution produced by Task 7 and TypeCtorTrait inheritance / mapped-slot metadata.
- Invariant: same family means same concrete carrier including captured arguments, while mapped payload variables remain ordinary independent type variables; capability is checked per signature position.

- [ ] **Step 1: Add failing instantiation and carrier tests**

Cover:

- `Box<$T>` impl instantiated as `Box<Int>` updates field, return, and body variables together;
- `Default::default` obtains `Self` from RTA or expected return without a runtime value argument;
- `TryFrom<Int>` keeps its Trait argument when dispatching the `String` subject;
- Functor and Monad occurrences share one family carrier because Monad's root is Functor;
- `Monad` and an unrelated `Monad2` family may use different carriers;
- `Either<String, Int>` and `Either<String, Boolean>` share a carrier, while `Either<Error, Boolean>` conflicts;
- multi-slot mappings compare every declared `slot_id` and position;
- reversing impl order and retrying a failed probe does not change the result.
- default method bodies receive the selected `Self`/Trait/RTA substitution;
- derived methods use a stable synthetic implementation identity without duplicating target inputs;
- builtin Trait methods obtain `BuiltinId` from canonical implementation metadata rather than names.

Assert that a typed call contains a concrete dispatch target and no pending inference variable before Forge receives it.

- [ ] **Step 2: Run both focused tests**

```bash
cargo nextest run -p scar --test trait_method_instantiation
cargo nextest run -p scar --test type_constructor_carriers
```

Expected: current specialization rebuilds a value-only mapping or represents constructor families too weakly.

- [ ] **Step 3: Add canonical family and carrier structures**

```rust
pub struct CanonicalConstructorCarrier {
    pub family_id: TypeCtorTraitFamilyId,
    pub constructor: ConstructorHead,
    pub arity: u32,
    pub mapped_slots: Vec<CanonicalMappedSlot>,
    pub captured_arguments: Vec<CanonicalCapturedArgument>,
}

pub struct TraitMethodInstantiation {
    pub implementation: TraitImplementationId,
    pub method: MethodDeclarationId,
    pub substitution: CanonicalSubstitution,
    pub callable_signature: CallableSignature<CanonicalTy>,
    pub dispatch_target: TraitDispatchTarget,
}
```

Derive `TypeCtorTraitFamilyId` from the canonical inheritance graph's connected component. Do not use the root display name, process-local insertion order, or implementation count as family identity.

- [ ] **Step 4: Unify carrier identity separately from capability**

Unify constructor head, arity, every mapped slot, and all captured/fixed arguments for occurrences in one family. Keep mapped payload types outside carrier identity. Record each occurrence's required Trait capability as an obligation on the shared carrier; a Functor position cannot call Monad methods merely because another position requires Monad.

- [ ] **Step 5: Carry the selection substitution through specialization**

Apply the exact Task 7 substitution to impl target, field types, method RTA/value/return, body, obligations, closure captures, and dispatch target. Build `CallableInstantiationKey` from canonical callable identity, concrete implementation identity, and all type inputs in stable role/ordinal order. Reject incomplete mappings as `UnresolvedTraitMethodInstantiation` at the instantiation boundary.

Use this same route for explicit impl bodies, default bodies, derived synthetic methods, and builtin Trait methods. Derive creates a stable `SyntheticMethodId` from derive site, target, and contract identity; builtin dispatch resolves `BuiltinId` through Task 3 metadata only.

- [ ] **Step 6: Enforce the Forge boundary**

Remove Forge-side Trait candidate lookup or type reconstruction. Add an assertion/error at codegen entry if a typed call lacks a concrete `TraitDispatchTarget` or retains a pending type/carrier; do not add a runtime dictionary.

- [ ] **Step 7: Verify the correction phase's type engine**

```bash
cargo nextest run -p scar --test trait_method_instantiation
cargo nextest run -p scar --test type_constructor_carriers
cargo nextest run -p scar
cargo nextest run -p forge
cargo nextest run -p rune --test integration run_srt
```

- [ ] **Step 8: Commit**

```bash
git add crates/scar crates/sigil/src/resolver/derive.rs crates/sindr/src/builtin.rs crates/forge tests/fixtures
git commit -m "refactor(types): unify method instantiation and constructor carriers"
```

---

### Task 9: Typed Diagnostic Reasons for Signatures, Traits, Operators, and Branches

**Files:**

- Modify: `crates/spire/src/ast.rs`
- Modify: `crates/spire/src/parser/expr.rs`
- Modify: `crates/sigil/src/resolved.rs`
- Modify: `crates/sigil/src/resolver/{expr,special_forms}.rs`
- Modify: `crates/scar/src/error.rs`
- Modify: `crates/scar/src/checker/signatures.rs`
- Modify: `crates/scar/src/checker/trait_selection.rs`
- Modify: `crates/scar/src/checker/carriers.rs`
- Modify: `crates/scar/src/checker/expr.rs`
- Modify: `crates/scar/src/checker/matching.rs`
- Modify: `crates/sindr/src/operator_diagnostics.rs`
- Modify: `crates/diagnostics/src/data.rs`
- Modify: `crates/diagnostics/src/typecheck.rs`
- Modify: `crates/diagnostics/src/render.rs`
- Modify: `crates/diagnostics/src/report.rs`
- Modify tests: `crates/diagnostics/src/tests/typecheck.rs`, `crates/diagnostics/src/tests/render_and_source.rs`
- Add fixtures: `tests/fixtures/script/fail/typecheck/structured_call_argument.{srt,error}`, `tests/fixtures/script/fail/typecheck/structured_trait_dispatch.{srt,error}`, `tests/fixtures/script/fail/typecheck/structured_carrier_relation.{srt,error}`, `tests/fixtures/script/fail/typecheck/structured_operator_relation.{srt,error}`, `tests/fixtures/script/fail/typecheck/structured_branch_relation.{srt,error}`

**Interfaces:**

- Produces: stable signature / Trait / carrier reason variants, `SourceRole`, complete typed facts, and common type-assertion results.
- Consumes: structured diagnostic envelope from Task 2 and semantic failures emitted by Tasks 4--8.
- Invariant: operator and helper calls share semantic reasons; `if`, `if_let`, `match`, and `cond` keep their context-specific final reasons while using the same inner assertion primitive.

- [ ] **Step 1: Add failing reason and parity tests**

Add table-driven tests for every reason in the common families: argument arity/mode/name, argument/return/annotation type relation, callable shape, RTA, invalid constraint subject, missing/deferred Trait capability, dispatch, constructor constraint/family/payload/capability, and Trait method contract. For `1 + "x"` and the equivalent `Add::add(1, "x")`, assert equal semantic reason and types but different origins:

```rust
assert_eq!(operator.reason(), helper.reason());
assert_eq!(operator.data().left_type(), Some("Int"));
assert_eq!(operator.data().right_type(), Some("String"));
assert!(matches!(operator.origin(), DiagnosticOrigin::Operator { .. }));
assert!(matches!(helper.origin(), DiagnosticOrigin::TraitCall { .. }));
```

Add branch regressions asserting `IfBranchTypeMismatch`, `MatchArmTypeMismatch`, and `CondBranchTypeMismatch`, with every branch/arm span and full type supplied by Scar.

- [ ] **Step 2: Run focused diagnostics and fixture tests**

```bash
cargo nextest run -p diagnostics typecheck
cargo nextest run -p rune --test integration run_srt
```

Expected: templates still infer reasons/types from messages or source text, and operator/helper parity fails.

- [ ] **Step 3: Centralize typed assertions in Scar**

Add one primitive that consumes two canonical types and two `SourceFact`s and returns either a committed substitution or a typed relation failure. Use it for callable arguments, expected return, annotations, operator operands, Trait invocation entries, carrier payloads, and branch bodies. The caller may wrap a relation failure in a contextual branch reason without discarding facts.

Preserve `cond` provenance, clause ordinals, and body spans through Spire and Sigil before its existing nested-`if` execution lowering. Without this metadata Scar cannot emit `CondBranchTypeMismatch` without searching source text.

- [ ] **Step 4: Emit the complete stable reason families**

Implement the names fixed by `signature_diagnostics_unification.md` sections 5.7 and 6.2. Keep `MissingGenericBound`, `MissingTraitCapability`, and `MissingTypeConstructorCapability` separate. Convert a deferred input only at its defined boundary to `AmbiguousReturnTypeArgument` or `UnresolvedTraitMethodInstantiation`; do not report a candidate-local rejection directly.

- [ ] **Step 5: Lower operators and helpers to the common invocation path**

Resolve an operator to its full Trait obligation and `CallableSignature`, then invoke the Task 5/7 solver. Preserve the token and left/right spans as `DiagnosticOrigin::Operator` plus `SourceRole::LeftValue` / `RightValue`. Delete arithmetic, concat, equality, comparison, context map/apply/bind, and compose diagnostic branches that re-check the same signature by callable or data-type name.

- [ ] **Step 6: Render only from reason and typed data**

For each migrated reason, create its `DiagnosticSpec` directly from closed `DiagnosticData`. Ensure call-site RTA labels may show the head alone, while relations between source values show both complete types. Serialize `reason`, `origin`, `data`, and `related` from the same failure and retain existing `kind`, `phase`, span, `expected`, `got`, and `hint` meanings.

- [ ] **Step 7: Verify focused families**

```bash
cargo nextest run -p diagnostics
cargo nextest run -p scar
cargo nextest run -p rune --test integration run_srt
```

- [ ] **Step 8: Commit**

```bash
git add crates/scar crates/sindr crates/diagnostics tests/fixtures
git commit -m "refactor(diagnostics): render signature failures from typed reasons"
```

---

### Task 10: SafeBind Correction, Heuristic Removal, and the `do` Start Gate

**Files:**

- Modify: `crates/scar/src/checker/expr.rs`
- Modify: `crates/scar/src/checker/patterns.rs`
- Modify: `crates/scar/src/checker/matching.rs`
- Modify: `crates/scar/src/typed.rs`
- Modify: `crates/forge/src/codegen.rs`
- Modify: `lib/bootstrap.srt`
- Modify: `doc/要件定義v9.md`
- Modify: `docs/dev/テスト方針.md`
- Modify: `docs/site/{error-handling,function-operators,language-reference}.md`
- Modify: `crates/spire/src/error.rs`
- Modify: `crates/sigil/src/error.rs`
- Modify: `crates/sigil/src/resolver/{declarations,expr,imports,patterns,special_forms}.rs`
- Modify: `crates/eldr/src/error.rs`
- Modify: `crates/diagnostics/src/{debug_render,parse,render,repl,report,resolve,runtime,source,surtr_code,typecheck}.rs`
- Delete after callers migrate: `crates/diagnostics/src/heuristics.rs`, `crates/diagnostics/src/heuristics/`
- Modify adapters: `crates/rune/src/{compile,error}.rs`, `crates/xldr/src/{error_display,repl/logic/core}.rs`
- Delete: `doc/signature_level_type_constructor_inference_draft.md`
- Delete: `tests/fixtures/script/fail/typecheck/option_safebind_rejected.srt`
- Delete: `tests/fixtures/script/fail/typecheck/option_safebind_rejected.error`
- Add fixtures: `tests/fixtures/script/pass/functions/safe_bind_non_result_pass_through.{srt,expected}`, `tests/fixtures/script/pass/functions/safe_bind_nested_result_once.{srt,expected}`, `tests/fixtures/script/fail/typecheck/safe_bind_pattern_type_mismatch.{srt,error}`, `tests/fixtures/script/fail/typecheck/safe_bind_failure_target_mismatch.{srt,error}`

**Interfaces:**

- Produces: canonical `SafeBindRhsProjection`, explicit failure-target relation, structured parse/resolve/policy/runtime diagnostics, and a heuristic-free renderer.
- Consumes: common type relation and diagnostic structures from Tasks 2 and 9.
- Invariant: only canonical `Result<A, E>` unwraps one layer; every other RHS type is passed unchanged to the normal MatchBlock pattern checker.

- [ ] **Step 1: Add failing SafeBind correction tests**

Cover canonical `Result` success/error, nested `Result` one-layer projection, non-Result scalar pass-through, `Option::Some` pass-through without automatic unwrap, ordinary constructor/list/tuple/record/extractor patterns, non-`Ok` constructor patterns, pattern annotation mismatch, and failure-target error mismatch. Replace the old Option rejection expectation with:

```surtr
def keep_option(value: Option<Int>) -> Result<Option<Int>, Error> {
  option =? value
  Ok(option)
}

def mismatch(value: Option<Int>) -> Result<Int, Error> {
  option: Int =? value
  Ok(option)
}
```

The first passes with `option: Option<Int>`; the second fails `AnnotationTypeMismatch` and reports both `Int` and `Option<Int>`.

- [ ] **Step 2: Run focused tests and confirm the old policy fails**

```bash
cargo nextest run -p scar safe_bind
cargo nextest run -p rune --test integration run_srt
```

Expected: Option-specific rejection and the `Ok`-only constructor restriction still fail the new cases.

- [ ] **Step 3: Normalize SafeBind semantics before pattern checking**

```rust
pub enum SafeBindRhsProjection {
    UnwrapResultOnce {
        payload: CanonicalTy,
        error: CanonicalTy,
        result_identity: BuiltinTypeId,
    },
    PassThroughNonResult {
        pattern_input: CanonicalTy,
    },
}
```

Classify using canonical builtin type identity, not the rendered name. Send the projected `pattern_input` to the existing MatchBlock pattern checker and remove Option-specific rejection and constructor-pattern `Ok` filtering. Keep the enclosing-return failure target explicit in typed IR so Forge does not infer it from `in_function` or a name.

Update `doc/要件定義v9.md`, `docs/dev/テスト方針.md`, the three affected site guides, and `lib/bootstrap.srt` to state the same Result-one-layer/non-Result-pass-through rule. Remove advice requiring conversion from Option to Result.

- [ ] **Step 4: Add structured reasons to every remaining phase family**

Migrate parser syntax/position, resolver name/namespace/visibility/import/capture, pattern/extractor/exhaustiveness/assignment, Error policy, Facet policy, Process/Task lifecycle, source/compile policy, runtime values, and compiler invariants. Each phase keeps its phase-specific error type and dedicated reason where the rule is not expressible by a signature; any embedded callable/type relation uses the common data variants.

- [ ] **Step 5: Remove every message and source inference caller**

After the last family has a direct producer, delete `heuristics.rs` and the `heuristics/` modules. Replace all template selection or data extraction based on `starts_with`, `strip_prefix`, `contains`, `split_once`, expected/got parsing, label markers, or source searches with `reason`, `origin`, typed facts, declaration identity, and remediation data. Do not retain an `UnmigratedMessage` variant after this step.

- [ ] **Step 6: Make remediation an identity-driven overlay**

Compute optional conversion/help overlays from canonical source/target identities, visibility, and a unique visible `From` / `TryFrom` implementation. Multiple or invisible implementations fall back to generic guidance. Overlay selection may not change reason, primary span, expected/got, or inspect rendered names/messages.

- [ ] **Step 7: Update adapters and perform the required REPL check**

Make Rune and Xldr forward every structured field without reconstructing it. Because this step changes `crates/xldr/**`, start an iTerm2 `Codex` tmux session and tell the user:

```text
tmux attach -t surtr-repl
detach: Ctrl-b, then d
```

Exercise one type error and one resolve error in the REPL and verify their human layout matches the corresponding Rune diagnostics.

- [ ] **Step 8: Delete superseded draft and run zero-match audits**

Delete `doc/signature_level_type_constructor_inference_draft.md` only after all normative content is represented by the four input specs and the implementation. Delete the obsolete Option-rejection fixture after its pass-through success and mismatch replacements pass. Run:

```bash
rg -n '\b(FunParams|fun_params|fun_param|FunParam)\b' crates tests lib docs doc \
  --glob '!doc/return_type_argument_rules.md' \
  --glob '!doc/trait_method_type_list_dispatch.md' \
  --glob '!doc/signature_diagnostics_unification.md' \
  --glob '!doc/do_intrinsic_spec.md' \
  --glob '!doc/type_constructor_signature_unification_implementation_plan.md'
rg -n 'UnmigratedMessage|infer_type_error_template|extract_expected_got|heuristics::' crates
rg -n 'Option.*SafeBind|SafeBind.*Option|constructor.*Ok|Ok.*constructor' crates lib tests
```

Expected: all three commands produce no matches. Inspect any documentation-only match before changing it; do not globally replace historical prose.

- [ ] **Step 9: Run the correction gate twice**

```bash
cargo nextest run --workspace
cargo nextest run --workspace
```

Expected: both consecutive runs pass before any `do` file or symbol is introduced. Also verify:

```bash
rg -n 'Ast::Do|Resolved::Do|TypedDo|DoBlock|IntrinsicId::Do' crates lib
```

Expected: no matches.

- [ ] **Step 10: Commit and mark the gate**

```bash
git add crates lib tests doc/signature_level_type_constructor_inference_draft.md doc/要件定義v9.md docs/dev/テスト方針.md docs/site
git commit -m "refactor(diagnostics): finish signature correction gate"
```

Record the two passing workspace command IDs or logs in the commit/PR notes. Tasks 11--15 may start only from this commit.

---

### Task 11: Compiler-Owned `do` Contract, `DoBlock`, and Surface Validation

**Precondition:** Task 10's zero-match audits and two consecutive workspace runs passed at the recorded correction-gate commit.

**Files:**

- Create: `crates/sindr/src/intrinsic.rs`
- Modify: `crates/sindr/src/{lib,builtin,names}.rs`
- Modify: `crates/spire/src/ast.rs`
- Modify: `crates/spire/src/parser/{decl,mod,tests}.rs`
- Modify: `crates/sigil/src/{error,semantic_metadata}.rs`
- Modify: `crates/sigil/src/resolver/{declarations,scope_init,tests}.rs`
- Modify: `lib/types/special_types.srt`
- Modify: `lib/bootstrap.srt`

**Interfaces:**

- Produces: `IntrinsicId::Do`, `DoIntrinsicContract`, reserved canonical `DoBlock` type identity, and structural stdlib-surface validation.
- Consumes: Task 3 `CallableSignature` and canonical identities for Monad, Alternative, Result, `bind`, and `empty`.
- Invariant: raw source signature text is display/docs input only; Sindr metadata is the type-checking contract and Eldr receives no new builtin.

- [ ] **Step 1: Add failing Sindr contract tests**

```rust
let contract = do_intrinsic_contract();
assert_eq!(contract.identity, IntrinsicId::Do);
assert_eq!(contract.signature.return_type_arguments.len(), 1);
assert_eq!(contract.signature.value_parameters.len(), 1);
assert_eq!(contract.capability_rules[0].required_trait, monad_trait_id());
assert_eq!(contract.bind_method, monad_bind_method_id());
assert_eq!(contract.empty_method, alternative_empty_method_id());
assert_eq!(builtin_type_meta("DoBlock").unwrap().usage, BuiltinTypeUsage::IntrinsicSignatureOnly(IntrinsicId::Do));
```

Also assert the Result-first SafeBind policy, both conditional Alternative predicates, and that every same-carrier rule references RTA ordinal 0.

- [ ] **Step 2: Run the failing Sindr test**

```bash
cargo nextest run -p sindr
```

Expected: the intrinsic contract and `DoBlock` metadata do not exist.

- [ ] **Step 3: Add the closed compiler-owned contract**

```rust
pub enum IntrinsicId { Do }

pub enum DoCapabilityPredicate {
    Always,
    HasPartialExtractPattern,
    HasSafeBindAndNonResultCarrier,
}

pub enum DoSafeBindInputPolicy {
    UnwrapCanonicalResultOnceOtherwisePassThrough,
}

pub enum DoSafeBindFailurePolicy {
    PreserveCanonicalResultOtherwiseAlternativeEmpty,
}

pub struct DoIntrinsicContract {
    pub identity: IntrinsicId,
    pub owner: CanonicalOwnerId,
    pub signature: CallableSignature<CanonicalTypeExpr>,
    pub capability_rules: &'static [DoCapabilityRule],
    pub safe_bind_input: DoSafeBindInputPolicy,
    pub safe_bind_failure: DoSafeBindFailurePolicy,
    pub bind_method: CanonicalTraitMethodId,
    pub empty_method: CanonicalTraitMethodId,
}
```

Use canonical identities for Result/Monad/Alternative/methods. The signature is exactly one direct-Monad RTA, one `DoBlock<$Result>` value parameter, and `Monad<$Result>` return. A TypeCtorTrait with multiple mapped slots retains and checks every mapping from metadata; no slot is chosen by position convention.

- [ ] **Step 4: Add the documented stdlib surface**

Add top-level `@builtin type DoBlock<$Result>` with `IntrinsicSignatureOnly(IntrinsicId::Do)` metadata to `lib/types/special_types.srt`. Add the documented declaration below inside `@autoimport defmod Bootstrap`:

```surtr
@intrinsic def do::<Monad>(block: DoBlock<$Result>) -> Monad<$Result>
```

Document carrier inference sources, Result one-layer projection, non-Result pass-through, Result error preservation, and Alternative-empty behavior. Do not add a runtime body or `BUILTIN_METAS` runtime entry.

- [ ] **Step 5: Parse and validate the surface structurally**

Retain a structured validation signature beside any raw intrinsic display text in Spire. In Sigil, compare owner, one RTA role, one value-parameter role/name/type, return relation, repeated `$Result`, and canonical identities against Sindr. A unit test may inject a malformed declaration into the resolver input; it must not edit `lib/bootstrap.srt` during the test. Emit `InvalidIntrinsicSurfaceContract` at the declaration span.

- [ ] **Step 6: Reserve `DoBlock`**

Reject user `DoBlock` declarations and impl/inherent-impl targets as `ReservedIntrinsicMarkerDeclaration` and `ReservedIntrinsicMarkerImpl`. Keep ordinary type-position use for Task 13, where Scar has full context.

- [ ] **Step 7: Verify metadata and surface parity**

```bash
cargo nextest run -p sindr
cargo nextest run -p spire
cargo nextest run -p sigil
git diff -- crates/eldr
```

Expected: all tests pass and the Eldr diff is empty.

- [ ] **Step 8: Commit**

```bash
git add crates/sindr crates/spire crates/sigil lib/bootstrap.srt lib/types/special_types.srt
git commit -m "feat(intrinsic): define canonical do contract"
```

---

### Task 12: `do` Syntax, AST, Resolver, and Sequential Scope

**Files:**

- Create: `crates/spire/src/parser/do_expr.rs`
- Modify: `crates/spire/src/{token,lexer,ast}.rs`
- Modify: `crates/spire/src/parser/{decl,expr,mod,tests,validate}.rs`
- Create: `crates/sigil/src/resolver/do_expr.rs`
- Modify: `crates/sigil/src/resolved.rs`
- Modify: `crates/sigil/src/resolver/{mod,expr,captures,warnings,tests}.rs`
- Modify exhaustive traversals: `crates/xldr/src/repl/logic/core.rs`, `crates/surtr-analysis/src/{project_runner,query,service}.rs`
- Add parse fixtures: `tests/fixtures/script/fail/parse/do_{carrier_syntax,carrier_arity,carrier_applied,carrier_variable,empty,no_final_expression}.{srt,error}`

**Interfaces:**

- Produces: `Ast::Do`, `AstDoStatement`, `Resolved::Do`, and `ResolvedDoStatement` with source spans and `IntrinsicId::Do`.
- Consumes: Task 1 call-site RTA parser and Task 11 intrinsic identity.
- Invariant: every RHS resolves before its LHS pattern binds; bindings are visible only in subsequent items and not outside the block.

- [ ] **Step 1: Add failing lexer/parser tests**

Test `do { return(1) }`, `do::<_>`, `do::<Option>`, nested blocks, `pattern <- rhs`, `pattern =? rhs`, and a pre-existing Facet `<-` case. Add negative cases for `do<Container>`, `do::<>()`, `do::<Option, List>`, `do::<Either<String, _>>`, `do::<$F>`, an empty block, and a final bind/discard statement.

- [ ] **Step 2: Run Spire tests**

```bash
cargo nextest run -p spire
```

Expected: `do` is still parsed as an identifier and statement forms are unavailable.

- [ ] **Step 3: Add syntax-owned structures**

```rust
pub enum AstDoStatement {
    Extract { span: Span, operator_span: Span, pattern: AstPattern, rhs: Box<Ast> },
    SafeBind { span: Span, operator_span: Span, pattern: AstPattern, rhs: Box<Ast> },
    Ordinary(Ast),
}

Ast::Do {
    span: Span,
    call_site_return_type_arguments: Vec<ReturnTypeArgument>,
    items: Vec<AstDoStatement>,
}
```

Add `Token::Do` and a dedicated expression parser. Omitted RTA stays an empty syntactic list and explicit `_` stays one item; Task 13 normalizes both to one inference slot. Parse may reject missing final expression by statement shape, but only Scar decides whether the final expression has monadic type.

- [ ] **Step 4: Add failing resolver scope tests**

Assert that `x <- rhs(x)` cannot see the new `x`, a later item can see it, nested blocks shadow correctly, and the name is absent after the block.

- [ ] **Step 5: Resolve sequentially with all origins**

```rust
pub enum ResolvedDoStatement {
    Extract { span: Span, operator_span: Span, pattern: ResolvedPattern, rhs: Box<Resolved> },
    SafeBind { span: Span, operator_span: Span, pattern: ResolvedPattern, rhs: Box<Resolved> },
    Ordinary(Resolved),
}
```

Create a child scope, resolve each RHS, then resolve/bind its pattern, and continue. Preserve block, operator, pattern, RHS, and RTA spans plus `IntrinsicId::Do`; do not convert the node into a normal call.

- [ ] **Step 6: Update exhaustive consumers and manually check REPL parsing**

Update AST/resolved rebase, capture, tolerant traversal, warning, and analysis visitors. In the iTerm2 profile `Codex`, start a `surtr-repl` tmux session, present `tmux attach -t surtr-repl`, and present `Ctrl-b` then `d`; verify a multiline nested `do` parses and a name does not leak from it.

- [ ] **Step 7: Verify syntax and scope**

```bash
cargo nextest run -p spire
cargo nextest run -p sigil
cargo nextest run -p xldr --test repl_core
cargo nextest run -p rune --test integration run_srt
```

- [ ] **Step 8: Commit**

```bash
git add crates/spire crates/sigil crates/xldr crates/surtr-analysis tests/fixtures
git commit -m "feat(syntax): parse and resolve do blocks"
```

---

### Task 13: `do` Carrier Inference, Capabilities, and Core Lowering

**Files:**

- Create: `crates/scar/src/checker/do_intrinsic.rs`
- Modify: `crates/scar/src/checker/{mod,expr,matching,patterns,specialize,types,signatures,trait_selection,carriers}.rs`
- Modify: `crates/scar/src/{typed,error}.rs`
- Create: `crates/scar/tests/do_intrinsic.rs`
- Add pass fixtures: `tests/fixtures/script/pass/do/{explicit,inferred_rhs,inferred_expected,payload_changes,user_carrier}.{srt,expected}`
- Add fail fixtures: `tests/fixtures/script/fail/typecheck/do_{ambiguous,explicit_conflict,carrier_conflict,captured_conflict,missing_monad,missing_alternative,reserved_marker_usage}.{srt,error}`

**Interfaces:**

- Produces: one do-local carrier constraint set, conditional capability obligations, concrete bind/empty dispatch, and lowered typed nodes without a residual `TypedDo`.
- Consumes: validated `DoIntrinsicContract`, Task 5 solver, Task 8 carriers and static Trait dispatch.
- Invariant: SafeBind RHS never infers the do carrier; all other monadic origins share one family/carrier including captured arguments, while statement payloads remain independent.

- [ ] **Step 1: Add failing carrier-inference tests**

Cover explicit RTA, expected result, extract RHS, known same-carrier bare expression, normal call including `guard`/`pure`/`return`, final expression, omitted-equals-underscore, source-order reversal, nested `F<F<A>>` single extraction, payload changes, user-defined carrier, `Either<String, _>` success, and `Either<String, _>` versus `Either<Error, _>` failure. `guard`, `pure`, and `return` participate only through their ordinary signatures; do not add a name branch or synthesize a new guard definition here.

- [ ] **Step 2: Run the focused test**

```bash
cargo nextest run -p scar --test do_intrinsic
```

Expected: Scar has no `Resolved::Do` checker.

- [ ] **Step 3: Instantiate the contract into one constraint set**

```rust
pub(super) enum DoOriginKind {
    ExplicitReturnTypeArgument,
    ExpectedResult,
    ExtractRhs,
    BareExpression,
    OrdinaryCall,
    FinalExpression,
}

pub(super) struct DoCarrierConstraint {
    pub carrier: CarrierPattern,
    pub payload: CanonicalTy,
    pub origin: DiagnosticOrigin,
}
```

Normalize omitted RTA and explicit `_` to one inference hole. Collect all permitted sources before committing a carrier, retain pending calls/bare expressions as deferred, and use the shared checkpoint/rollback environment. A known same-carrier bare `F<_>` is always sequenced; an ordinary non-monadic value remains an ordinary statement under existing block rules.

- [ ] **Step 4: Enforce same-family/same-carrier rules**

Compare family ID, constructor head, arity, every mapped slot and position, and recursively resolved captured/fixed arguments. Keep mapped payloads per statement. If an explicit RTA participates in the conflict emit `ReturnTypeArgumentMismatch`; otherwise emit `TypeConstructorFamilyMismatch`, retaining both full-type origins.

- [ ] **Step 5: Resolve conditional capabilities**

Always require Monad on the selected carrier. Add Alternative only for a partial `<-` pattern or, in Task 14, non-Result SafeBind. Use existing MatchBlock totality; never infer totality from pattern text. A rigid carrier missing a declared bound emits `MissingGenericBound`, a concrete carrier without an impl emits `NoApplicableTraitImplementation`, and a non-family capability origin emits `MissingTypeConstructorCapability`.

- [ ] **Step 6: Lower backwards with static dispatch**

Keep the final monadic expression as the tail. Lower known monadic bare expressions and total extracts to concrete `Monad::bind` calls with synthetic continuations. Lower a partial extract to bind plus an exhaustive synthetic match whose wildcard invokes the selected `Alternative::empty` at the block result payload. Preserve original origins on synthetic nodes; resolve all dispatch before leaving Scar.

- [ ] **Step 7: Reject `DoBlock` in ordinary type positions**

Allow the reserved marker only in the validated intrinsic signature. Parameters, returns, fields, bindings, aliases, and user signatures using it emit `ReservedIntrinsicMarkerUsage` at the type span.

- [ ] **Step 8: Audit the pre-Forge typed result**

Add a test traversal asserting no `TypedDo`, pending RTA/carrier, or pending Trait dispatch remains. Then run:

```bash
cargo nextest run -p scar --test do_intrinsic
cargo nextest run -p scar
```

- [ ] **Step 9: Commit**

```bash
git add crates/scar tests/fixtures
git commit -m "feat(typecheck): infer and lower do carriers"
```

---

### Task 14: `do` SafeBind Failure Policy and Forge Lowering

**Files:**

- Modify: `crates/scar/src/checker/{do_intrinsic,expr,matching,patterns,specialize,types}.rs`
- Modify: `crates/scar/src/{typed,error}.rs`
- Modify: `crates/scar/tests/do_intrinsic.rs`
- Modify: `crates/forge/src/{codegen,error}.rs`
- Modify: Forge unit tests in `crates/forge/src/codegen.rs`
- Add pass fixtures: `tests/fixtures/script/pass/do/{safebind_result,safebind_option,safebind_list,safebind_user_alternative}.{srt,expected}`
- Add fail fixtures: `tests/fixtures/script/fail/typecheck/do_safebind_{ambiguous_carrier,missing_alternative,pattern_type,error_type}.{srt,error}`

**Interfaces:**

- Produces: explicit do-local SafeBind RHS projection and failure target, with Forge emission through existing branch/pattern/call machinery.
- Consumes: Task 10 shared SafeBind correction and Task 13 selected do carrier/result type.
- Invariant: canonical Result do preserves existing errors; every non-Result do uses the selected carrier's Alternative `empty`; RHS is evaluated exactly once.

- [ ] **Step 1: Add failing SafeBind/do tests**

Cover nested Result one-layer projection, Option/List/String/user RHS pass-through, a two-level `Option::Some` pattern, a deliberate `Option<Int>` versus `Int` annotation mismatch, carrier not inferred from SafeBind alone, Result error preservation, non-Result empty override, later-effect skipping, and RHS evaluation count.

- [ ] **Step 2: Run Scar tests and confirm the missing policy**

```bash
cargo nextest run -p scar --test do_intrinsic
```

Expected: the shared projection exists but has no do-local failure target.

- [ ] **Step 3: Add explicit typed control data**

```rust
pub enum SafeBindFailureTarget {
    PreserveResult {
        result_identity: BuiltinTypeId,
        expected_error_type: CanonicalTy,
    },
    AlternativeEmpty {
        obligation: TraitObligation,
        dispatch: TraitDispatchTarget,
    },
}

pub struct TypedDoSafeBind {
    pub pattern: TypedPattern,
    pub rhs: Box<TypedNode>,
    pub rhs_projection: SafeBindRhsProjection,
    pub continuation_result_type: CanonicalTy,
    pub failure_target: SafeBindFailureTarget,
    pub origins: DoSafeBindOrigins,
    pub on_success: Box<TypedNode>,
}
```

Represent the temporary control node as `TypedInner::DoSafeBindControl(Box<TypedDoSafeBind>)`. It is a typed lowering aid, not a runtime opcode or unresolved do expression.

- [ ] **Step 4: Select failure policy after the carrier is solved**

Check canonical Result identity first, even if Result later gains Alternative. Result preserves the RHS `Err` and existing pattern-generated SafeBind failure kind, proving its error relation against the enclosing do result. Every other carrier adds Alternative on the same carrier and resolves `empty` at the do continuation result `F<R>`. Keep policy deferred until carrier identity is known; never choose a carrier from SafeBind RHS or visible impl count.

- [ ] **Step 5: Build the SafeBind continuation backwards**

Use Task 10's projection and the full MatchBlock pattern checker. Success continues with `on_success`. Result failure returns the existing error/failure value. Alternative failure discards any Result error or pattern failure payload and calls the stored static `empty` target. Ensure the RHS node has one evaluation edge.

- [ ] **Step 6: Add the Forge emitter**

Emit `DoSafeBindControl` with existing conditional branch, pattern, closure, and `Call` instructions. Select behavior only from `SafeBindFailureTarget`; do not inspect `in_function`, display names, or source syntax. Do not change Eldr or add an opcode.

- [ ] **Step 7: Verify static and runtime boundaries**

```bash
cargo nextest run -p scar --test do_intrinsic
cargo nextest run -p forge
cargo nextest run -p eldr
rg -n 'Opcode::.*Do|Do[A-Z].*Opcode|candidate.*runtime' crates/forge/src crates/eldr/src
git diff -- crates/eldr
```

Expected: tests pass, both scans/diff show no do-specific VM implementation, and all dispatch targets are concrete.

- [ ] **Step 8: Commit**

```bash
git add crates/scar crates/forge tests/fixtures
git commit -m "feat(lowering): route do SafeBind failures explicitly"
```

---

### Task 15: `do` Diagnostics and Acceptance Fixtures

**Files:**

- Modify: `crates/diagnostics/src/{data,typecheck,render,report}.rs`
- Modify: `crates/diagnostics/src/tests/{typecheck,render_and_source}.rs`
- Modify: `crates/scar/src/error.rs`
- Modify: `crates/rune/src/{compile,error}.rs`
- Modify: `lib/bootstrap.srt`
- Modify: `docs/dev/diagnostics.md`
- Modify pass fixtures from Tasks 13--14: `tests/fixtures/script/pass/do/{explicit,inferred_rhs,inferred_expected,payload_changes,user_carrier,safebind_result,safebind_option,safebind_list,safebind_user_alternative}.{srt,expected}`
- Add pass fixtures: `tests/fixtures/script/pass/do/{option,list,result,either,total_pattern,partial_pattern,safebind_preserve,safebind_empty,bare_sequence,inference,user_defined}.{srt,expected}`
- Modify parse fixtures from Task 12: `tests/fixtures/script/fail/parse/do_{carrier_syntax,carrier_arity,carrier_applied,carrier_variable,empty,no_final_expression}.{srt,error}`
- Add resolve fixtures: `tests/fixtures/script/fail/resolve/do_{surface_contract,reserved_marker_declaration,reserved_marker_impl}.{srt,error}`
- Modify typecheck fixtures from Task 13: `tests/fixtures/script/fail/typecheck/do_{ambiguous,explicit_conflict,carrier_conflict,captured_conflict,missing_monad,missing_alternative,reserved_marker_usage}.{srt,error}`
- Add typecheck fixtures: `tests/fixtures/script/fail/typecheck/do_{missing_capability,missing_bound,no_impl,safebind_pattern,safebind_error}.{srt,error}`
- Create: `tests/integration/language_features/do_intrinsic.rs`
- Modify: `tests/integration/language_features.rs`

**Interfaces:**

- Produces: exact Ariadne layouts and additive JSON fields for do failures, plus the full standard and user-defined runtime matrix.
- Consumes: common Task 9 reasons/data and Task 11--14 semantic origins.
- Invariant: `do` adds context to common failures but does not introduce a second carrier/type-inference reason system or expose inference IDs/synthetic spans.

- [ ] **Step 1: Add failing diagnostic unit tests**

Cover explicit-RTA versus RHS, RHS versus RHS, ambiguity, partial-pattern Alternative, non-Result-SafeBind Alternative, Result error relation, invalid carrier syntax, `DoBlock` restrictions, and surface-contract mismatch. Assert two source labels for relation conflicts and exact message/label/note/help roles from `do_intrinsic_spec.md` section 11.

- [ ] **Step 2: Add failing JSON tests**

Assert typed fields for RTA ordinal, stable family ID, required Trait identity, explicit constructor, left/right full types and origins, captured argument ordinal, pattern totality, and:

```rust
assert_eq!(json.data["safe_bind_mode"], "preserve_result");
assert_eq!(json.data["safe_bind_rhs_projection"], "unwrap_result_once");
assert_eq!(json.data["pattern_input_type"], "Int");
```

Add the corresponding `override_with_empty` / `pass_through_non_result` case. These values are closed enums; an unresolved policy is never user-facing JSON.

- [ ] **Step 3: Run diagnostics tests**

```bash
cargo nextest run -p diagnostics
```

Expected: structured failures exist but do context templates/data projections are incomplete.

- [ ] **Step 4: Implement do-context rendering**

Map parse/resolve/typecheck reasons exhaustively to the specified templates. An explicit RTA conflict uses `ReturnTypeArgumentMismatch`; inferred-origin carrier conflicts use `TypeConstructorFamilyMismatch`; rigid bound failures and concrete impl failures remain distinct. Reuse common typed facts and remediation; never branch on the string `do`, Result/Option display names, or a rendered message.

- [ ] **Step 5: Replace the obsolete Option rejection fixture**

Confirm Task 10 deleted the two obsolete Option-rejection files and that their non-Result pass-through success plus static pattern-type mismatch replacements remain covered. Update `lib/bootstrap.srt` docs only if Task 10/11 wording is not yet identical to the final behavior.

- [ ] **Step 6: Fill the pass and fail matrix**

Exercise Option/List/Result/Either and a user-defined Monad+Alternative; total and partial patterns; SafeBind preserve/empty; effect skipping; bare sequencing; changing payloads; explicit/expected/RHS/final inference; malformed RTA; ambiguity/conflicts; captured arguments; capability/bound/impl errors; reserved marker cases. Use a resolver unit-test injection for malformed canonical stdlib surface rather than altering the installed `lib` in a fixture.

- [ ] **Step 7: Register runtime language-feature tests**

Add `do_intrinsic` to `tests/integration/language_features.rs`. Verify user-defined carrier lowering and observable evaluation order through Rune; do not add an Eldr-specific test path or runtime intrinsic.

- [ ] **Step 8: Run focused integration tests**

```bash
cargo nextest run -p diagnostics
cargo nextest run -p rune --test integration -E 'test(/language_features/)'
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
```

- [ ] **Step 9: Commit**

```bash
git add crates/diagnostics crates/scar crates/rune lib/bootstrap.srt tests docs/dev/diagnostics.md
git commit -m "test(do): cover diagnostics and runtime behavior"
```

---

### Task 16: Final Acceptance and Repository Audit

**Files:**

- Review: all files modified by Tasks 1--15
- Review: `doc/{return_type_argument_rules,trait_method_type_list_dispatch,signature_diagnostics_unification,do_intrinsic_spec}.md`
- Review: `docs/dev/{Trait_system_spec,diagnostics}.md`
- Modify only if an audit finds a concrete gap: the owning implementation/test/doc file from Tasks 1--15

**Interfaces:**

- Produces: an acceptance-criterion matrix, clean forbidden-route scans, passing focused/integration/workspace gates, and a reviewable final diff.
- Consumes: all task commits and the four normative input specs.
- Invariant: this task does not widen scope or create an empty “audit” commit.

- [ ] **Step 1: Build the acceptance matrix**

Map every numbered acceptance criterion in the four input specs and every diagnostics/do test-matrix bullet to a named unit test or fixture path. Record the matrix in the PR/implementation log; any uncovered row is work, not a waived item.

- [ ] **Step 2: Run canonical vocabulary and old-route scans**

```bash
rg -n '\b(FunParams|fun_params|fun_param|FunParam)\b|Ast::TypeApply|Resolved::TypeApply|BuiltinContract|builtin_contracts|check_builtin_contract' \
  crates tests lib docs/dev docs/site doc/要件定義v9.md
rg -n 'UnmigratedMessage|infer_type_error_template|extract_expected_got|heuristics::|constructor_witness_traits|constructor_context_candidates' crates
```

Expected: no matches. A renamed neutral source/span helper must live outside a `heuristics` module.

- [ ] **Step 3: Run semantic-fallback and runtime scans**

```bash
rg -n 'Option is not a SafeBind target|SafeBind constructor pattern only supports Ok|first impl|unique impl|default carrier' crates lib tests docs/dev docs/site doc/要件定義v9.md
rg -n 'TraitDispatch::Pending' crates/forge crates/eldr
rg -n 'Opcode::.*Do|Do[A-Z].*Opcode|IntrinsicId::Do|DoBlock' crates/eldr/src
```

Expected: no forbidden semantic fallback and no executable pending dispatch or do runtime feature. Explicit negative-test assertions explaining a prohibition must be reviewed and excluded with a narrower glob rather than accepted blindly.

- [ ] **Step 4: Verify formatting and focused suites**

```bash
cargo fmt --all -- --check
cargo nextest run -p sindr -p spire -p sigil -p scar -p forge -p diagnostics
```

- [ ] **Step 5: Verify integration suites**

```bash
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
cargo nextest run -p xldr --test repl_core
```

- [ ] **Step 6: Run the final workspace gate twice**

```bash
cargo nextest run --workspace
cargo nextest run --workspace
```

Expected: two consecutive clean passes.

- [ ] **Step 7: Inspect the final repository state**

```bash
git status --short
git diff --check
git log --oneline --decorate -20
```

Confirm each semantic task has its scoped commit, deleted legacy files are absent, no unrelated user changes were included, and Task 10 precedes the first do commit.

- [ ] **Step 8: Commit only audit fixes**

If the audit changes an owning file, rerun its focused suite and both workspace gates, then commit only those concrete fixes:

Stage only the exact paths shown by `git diff --name-only` that close a mapped acceptance gap, then commit them with message `fix(types): close signature and do audit gaps`.

If no file changed, do not create an empty commit. Attach the acceptance matrix and command results to the handoff.
