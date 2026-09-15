# Diagnostics 開発指針

`crates/diagnostics` の user-facing diagnostics に適用する正本ルール。診断文を追加・変更するときは、表示文より先に `DiagnosticSpec` の役割分担と構造化契約を確認する。

## 適用範囲

対象は parser、resolver、typechecker、runtime、REPL が生成する `DiagnosticSpec` と、その Ariadne / JSON 出力。

対象外:

- `DebugLabel` を使う inspect / debug 表示
- compiler 内部のログ・トレース
- concrete `deferror` の runtime 値
- JSON クライアント固有のレイアウト

## `DiagnosticSpec` の役割

| フィールド | 役割 | 入れてよい内容 |
|---|---|---|
| `message` | headline | 診断の主原因。短い一文 |
| `labels` | source caption | span に結び付く対象、関連定義、失敗箇所、期待値と実際値 |
| `notes` | 補足 | ルール、推論・変換過程、runtime context、入力の分類 |
| `help` | 修正案 | 利用者が取るべき操作、代替構文、書き換え例 |

source span を必要としない説明や修正案を `labels` に置かない。迷う場合は「その文がコード上のどこを指すか」を基準にし、指さない説明は `notes`、利用者への命令は `help` に置く。

## 構造化診断契約（実装済み）

各 phase の診断は phase 固有の error 型を維持したまま、次の構造化入力を正本とする。

- `TypeDiagnosticReason`、`ParseDiagnosticReason`、`ResolveDiagnosticReason`、`RuntimeDiagnosticReason`、`ReplDiagnosticReason` は意味上の失敗を表す閉じた reason enum、`DiagnosticOrigin` は `Call`、`TraitCall`、`Operator`、`Annotation`、`Return`、`Branch`、各 phase adapter などの発生文脈を表す。reason に callable 名や演算子名を埋め込まない。
- `DiagnosticData` は reason ごとの閉じた型付き variant、`SourceFact` は `role`、`source_id`、`span`、必要に応じた完全な `ty` と `declaration_identity` を保持する。structured envelope は `reason`、`origin`、`data`、`primary`、`related`（および remediation）を持つ。
- Human-readable 出力（`DiagnosticSpec` の message / labels / notes / help）と JSON は、同じ structured input から投影する。自然言語、label 本文、source text を再解析して reason や typed field を推測しない。
- JSON の既存フィールド `kind`、`phase`、`line`、`column`、`span`、`message`、`expected`、`got`、`hint` は意味を変えずに保持し、`reason`、`origin`、`data`、`related` を additive に追加する。
- renderer と Rune/Xldr adapter は producer が reason を供給しない入力を、message 解析で救済しない。Scar の producer 契約違反は `TypecheckInvariantViolation` として fail closed にし、元 message を安定 reason へ見立てない。
- runtime SafeBind failure は typed IR の `RuntimeErrorDiagnostic` を VM まで保持する。表示用 marker を message へ埋め込まない。
- source-backed builtin declaration は `BUILTIN_METAS.surfaces` の owner/name/RTA/parameter mode・型/return/where と完全一致させる。compiler-generated declaration identityは別の`compiler_generated_surfaces`へ正確なowner/nameだけを登録し、ASTの生成由来も同時に要求する。runtime/compiler-only builtinをsource surfaceへ混入させず、名前接頭辞や任意owner合成で検証を迂回しない。

## 型関係と呼び出しの診断（実装済み）

- Scar の `assert_type_relation` は、失敗時に部分的な型代入と capability / obligation の変更を rollback する。成功した制約だけを後続へ渡す。両側の source fact は照合対象の型とともに保持する。
- 呼び出しの arity / mode / named 引数、通常の型関係、Trait obligation / dispatch、constructor family / payload / capability は共通の reason を使う。演算子と対応する helper は同じ検査を通し、文脈の違いを `DiagnosticOrigin` に保持する。入れ子の呼び出し・annotation の失敗を外側の演算子へ付け替えない。
- `cond` の節と `if_let` の発生文脈は Spire / Sigil から Scar まで保持する。分岐診断には全 body の型・span・ordinal と guard の source fact を含める。`cond` の実行は型検査後に既存の `TypedInner::If` へ lowering する。
- JSON の `data` は source location の rebase 後に typed projection から生成する。必須 key は省略せず、該当しない値は `null` にする。`related` は primary fact も含み、型は `type`、source role は `left_value` / `right_value` などの snake_case とする。
- constructor `family_id` は同じ族の canonical Trait ID をソートして構成する。familyはcapability継承を表し、別direct parameterのcarrier同一性を暗黙に作らない。完全な source value の型には captured 引数と `Result` の error 型も含める。登録順や内部 inference ID を表示しない。
- structured input がある場合、optional field の欠落を理由に message / label / source の解析へ戻らない。SafeBind、pattern / Extractor / exhaustiveness、policy、runtime、parser、resolver、REPL command/query の各 family は producer-owned reason/data から表示する。

## stable reason と typed data

型関係・callable・Trait・TypeCtorTrait・branchの実装済み経路は、次の閉じた`TypeDiagnosticReason`を使う。
同じ意味の失敗にsurface名別のreasonを増やさず、call、Trait call、operator、annotation、return、branchなどの
違いは`DiagnosticOrigin`で表す。

| family | stable reason |
|---|---|
| argument contract | `ArityMismatch`, `ArgumentModeMismatch`, `UnknownNamedArgument`, `DuplicateArgument`, `MissingArgument` |
| type relation / callable | `ArgumentTypeMismatch`, `ReturnTypeMismatch`, `AnnotationTypeMismatch`, `NotCallable`, `CallableShapeMismatch`, `CallableSignatureMetadataMismatch` |
| ReturnTypeArgument | `ReturnTypeArgumentArityMismatch`, `ReturnTypeArgumentMismatch`, `AmbiguousReturnTypeArgument`, `DuplicateReturnTypeArgumentInput`, `MissingReturnTypeArgument`, `UnusedReturnTypeArgument`, `ConcreteReturnTypeArgumentInDefinition`, `InlineReturnTypeArgumentConstraint` |
| Enum constructor | `UnresolvedEnumConstructorTypeArgument` |
| constraint / Trait | `InvalidTraitConstraintSubject`, `MissingGenericBound`, `MissingTraitCapability`, `NoApplicableTraitImplementation`, `UnresolvedTraitMethodInstantiation`, `MissingTraitDispatchTarget` |
| TypeCtorTrait | `MissingTypeConstructorConstraint`, `TypeConstructorFamilyMismatch`, `TypePayloadMismatch`, `MissingTypeConstructorCapability` |
| Trait method contract | `TraitMethodTypeListMismatch`, `TraitMethodTypeListArityMismatch`, `TraitMethodConstraintMismatch` |
| branch | `IfBranchTypeMismatch`, `MatchArmTypeMismatch`, `CondBranchTypeMismatch` |
| SafeBind input | `SafeBindTotalPatternNonMonadRhs`, `SafeBindTotalPatternNonResultMonadRhs` |
| pattern / Extractor | `PatternTypeMismatch`, `PatternShapeMismatch`, `PatternArityMismatch`, `NonTotalBindingPattern`, `NestedResultErrorPattern`, `MatchGuardTypeMismatch`, `ConstructorPatternRequiresEnumOrResultRhs`, `ExtractorInputTypeMismatch`, `ExtractorArityMismatch`, `NonExhaustiveMatch` |
| policy | `SafeBindErrorTypeMismatch`, `SafeBindRequiresResultTarget`, `InvalidResultEffectAnnotation`, `ErrorValueMustBeWrapped`, Facet / Process / source / compile policy reason、`NominalDeclarationConstraintViolation`, `TraitHelperCaptureNeedsExpectedType`, `ReservedIntrinsicMarkerUsage` |
| producer contract | `TypecheckInvariantViolation` |

`MissingGenericBound`はrigid genericの宣言済みproof不足、`MissingTraitCapability`は具象subjectの能力不足、
`MissingTypeConstructorCapability`はconstructor carrier occurrenceの能力不足であり、相互に置換しない。
未確定inference variableのobligationは`Deferred`として保持し、候補数や登録順からreasonや型を決めない。

`DiagnosticData`はreasonに必要な型付きpayloadを保持する。JSONでは`kind` discriminatorと、次表の
serialized fieldだけを投影する。source factなど一部の内部fieldはrendererでlabel/noteを構成するための値で、
`#[serde(skip)]`によりJSON dataには出ない。optionalなserialized値が存在しない場合も、reasonごとのschemaを
messageに合わせて変形させない。

| data kind | 主なserialized fields |
|---|---|
| `CallableShape` / `ArgumentContract` | `callable`, arity/count, parameter name, return shape |
| `ArgumentRelation` | `callable`, `ordinal`, `expected_type`, `actual_type` |
| `ReturnTypeArgument` | `callable`, `ordinal`, `declared_origin`, `value_parameter_origin`, `return_origin`, expected/actual typeとcount |
| `EnumConstructorTypeArgument` | `enum_name`, `constructor`, `ordinal`, `constraint_status` |
| `CallableSignature` | `callable`, `role`, expected/actual count, `detail` |
| `TraitMethodTypeList` / `TraitMethodConstraint` | optional `identity`、`method_name`、role/ordinal/nested path、expected/actual type・countまたはconstraints、`impl_declaration` |
| `TraitDispatch` / `CandidateSelection` | `trait_name`, `trait_arguments`, `subject_type`, method, `impl_declaration`。後者はcandidate failuresも保持する |
| `ConstraintSubject` / `TraitObligation` | subject/constraint、またはtrait name/arguments・subject type・position |
| `TypeConstructorCarrier` | `family`, `family_id`, `expected_carrier`, `actual_carrier` |
| `BranchAssertion` | `expected_type`, `actual_type`, `branch` |
| `SafeBindRelation` | `lhs_type`, `rhs_type`, `lhs_is_total`, `rhs_is_canonical_result`, `monad_capability`, `result_effect`, `alternative_capability` |
| `Pattern` / `Policy` / `Runtime` / `Parse` / `Resolve` / `Repl` | family固有の閉じた入力。`detail`は表示・追跡用であり、reason再分類には使わない |

SafeBind固有reasonは、通常pattern型検査を通過したtotal pattern + non-Result RHSにだけ生成する。
canonical Monad proofが成立すれば`SafeBindTotalPatternNonResultMonadRhs`、closed concrete typeで
不成立なら`SafeBindTotalPatternNonMonadRhs`とする。Deferred、rigid genericのbound不足、solverの
既存structured failureをこの二reasonへ畳み込まない。どちらのheadlineもcanonical Resultだけが
外側一段の自動分解対象であることを本文に含め、変換APIのhelpは生成しない。

SafeBind/failureMatcher の ResultContext は `ResultEffect > Alternative > Monad` の順で解決する。
canonical `Result` または検証済み `@result_effect` carrier は既存 Error を保持し、Result effect がない場合だけ
`Alternative::empty` へ置換する。`Monad` 単独は sequencing capability であり failure target ではない。
failureMatcher/partial `<-` は Result effect があれば Error を保持し、なければ `Alternative::empty`、どちらもなければ
capability error とする。total `<-` は Monad のみを要求し、`guard` は Result effect を参照しない通常の Alternative
call として扱う。annotation、carrier、policy が未確定な場合は `Deferred` を保持し、候補数や登録順で
Result/Alternativeを選ばない。

`InvalidResultEffectAnnotation` は annotation span を primary とし、sole field、visibility、canonical Monad / MonadT
impl、captured base relation のうち失敗根拠となる宣言 span を related fact として保持する。必要 metadata の
欠落を annotation 無視や `Alternative` route への切り替えで隠さない。SafeBind/failureMatcher では元の `=?`、
pattern、RHS、return/do result span と Error の kind、message、location、cause を、Result-preserving target まで保持する。

constructor-context経路の`CandidateFailureData`は候補ごとの型と失敗detailを保持する。通常のTrait候補選択は
閉じた`CandidateRejection`からrelated factsとsummary noteを構築する。どちらも候補の失敗をtyped dataとして保持し、
全候補reject時に情報を捨てて一般callable経路へfallbackせず、最終診断はrequested obligation全体から作る。
一候補のlocal failureをそのまま最終reasonにしない。

remediationはbase reason、expected/actual type、primary spanを変更しないoverlayである。具体的な変換案は
canonical identityと可視なconversion implから一意に裏付けられる場合だけ追加し、rendered type名やmessageの
文字列一致から選ばない。

compiler-owned intrinsic の標準 surface が canonical contract と異なる場合は resolve reason
`InvalidIntrinsicSurfaceContract`、intrinsic 専用 marker の user declaration / impl target は
`ReservedIntrinsicMarkerDeclaration` / `ReservedIntrinsicMarkerImpl` を使う。通常 type position に現れた
marker は typecheck reason `ReservedIntrinsicMarkerUsage` とし、raw intrinsic signature や表示文を再解析して
reason を決めない。

## 実装規則

- `labels` の各 `span` は、表示する本文と対応するソース範囲を指す。関連ファイルの定義は `source_id` を設定する。
- headline だけで十分な診断に無理な source label を追加しない。
- `kind`、`phase`、`primary_span`、`expected`、`got`、`hint` の意味を表示文の改善目的で変更しない。
- テンプレートを変更したら、source label・note・help の分類が変わる renderer 判定 helper も確認する。
- 修正方法・代替構文・書き換え例は `message` や label ではなく `help` に書く。`message` は原因を短い headline として残す。
- 新しい診断文を追加する前に既存の同 phase / 同種の診断を見渡し、headline の簡潔さ、命令形の Help、句読点などの温度感を統一する。

現在の代表例:
- operator の `OP rule` は演算子 span を指す source label、`BIND_RULE_TEXT` は typecheck の `notes`、`=?` などの書き換え案は `help` に置く。
- extractor の `input source` は `notes`、extractor 定義は関連 source label に置く。
- runtime の失敗値・pattern・`call target` は label、`expected rule`・`runtime rule`・`opcode`・入力分類は `notes` に置く。
- `assert_eq` の LHS/RHS term は比較対象の span を指すため label、失敗の説明は `help` に置く。
- contextual type syntax では、`Trait<...>` を where RHS に置いた parser diagnostic、Trait-head / nominal binderへ constraint を併記した位置違反、通常型注釈などでの constructor application の位置違反、`Self::...` / `Type::...` の owner-path 違反を parser phase にする。TypeCtorTrait の call-site ReturnTypeArgument 内の完全・部分型applicationと`_`は合法な型入力として扱う。position rule は `notes`、bare constraint や許可された位置への書換えは `help` に置く。
- `Enum<...>::Variant` は Spire で callable の `::<...>` と別の expression として保持する。owner が enum でない、variant が owner に属さない、owner 型引数 arity が一致しない場合は Sigil の resolve diagnostic とする。payload または expected type と明示型引数が一致しない場合は Scar の既存 type relation / argument diagnostic とする。
- bare または `_` を含む通常 enum constructor の型引数が Scar の finalization まで未確定なら `UnresolvedEnumConstructorTypeArgument` とする。`DiagnosticOrigin::EnumConstructor` と constructor span、未確定 ordinal、`Insufficient` constraint status を保持する。builtin-special `Result` constructor の専用診断・推論経路はこの reason の対象外とする。
- bare capability の未使用、fresh result witness の未確定、full obligation / pending dispatch の未解決は typecheck phase にする。position rule は `notes`、constraint の削除または必要な式の利用は `help` に置く。
- Trait-head / nominal declaration binder の inline constraint 違反では、`where $P: Bound` への help を一意に提示し、binder内constraintや direct TypeCtorTrait binder という別の書換え候補を併記しない。通常型注釈の`_`は`Hole`、RTA内の`_`は推論変数として別々に診断する。

## 出力契約

### Human-readable

renderer は `message`、`labels`、`notes`、`help` をそれぞれ headline、source caption、note、help として出力する。Ariadne の色、罫線、空白、label の順序は安定契約にしない。

### JSON

`serializable_diagnostic_by_id` が出力する次の値を安定させる。

```json
{
  "kind": "TypeError",
  "phase": "typecheck",
  "line": 2,
  "column": 14,
  "span": [13, 23],
  "message": "expected Int, got String",
  "expected": "Int",
  "got": "String",
  "hint": "..."
}
```

自然言語の `message`、label 本文、note、help は意味を保つ範囲で変更できる。クライアントが新しい情報へ依存する場合は文字列解析を増やさず、typed field を追加する。

## テスト規則

- unit test は `kind` と主な構造化値を確認し、`labels`・`notes`・`help` を個別に検証する。
- ルールや help が label に戻っていないことを、代表的な診断の負の assertion で固定する。
- renderer test は headline、source、note、help の存在と意味を確認し、ANSI・罫線・空白・全文一致に依存しない。
- compile-error fixture は既存 parser の形式を使う。

```text
phase: typecheck
contains: expected Int
contains: got String
```

- `stdout`、exit code、runtime value、無関係な CLI 文言の厳密な検証は弱めない。
- contextual type syntax の fixture は parser/typecheck phase、primary span、position-rule note、rewrite help を検証する。rule や rewrite を source label に置かない。

## 検証コマンド

```bash
cargo nextest run -p diagnostics --lib
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
cargo nextest run --workspace
```

変更範囲に応じて focused test を先に実行し、最後に workspace 全体を実行する。失敗が既存か変更起因かを分けて記録する。
