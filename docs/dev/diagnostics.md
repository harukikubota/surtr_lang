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

typecheck 診断は、phase 固有の error 型を維持したまま、次の構造化入力を正本とする。

- `TypeDiagnosticReason` は意味上の失敗を表す閉じた reason enum、`DiagnosticOrigin` は `Call`、`TraitCall`、`Operator`、`Annotation`、`Return`、`Branch` などの発生文脈を表す。reason に callable 名や演算子名を埋め込まない。
- `DiagnosticData` は reason ごとの閉じた型付き variant、`SourceFact` は `role`、`source_id`、`span`、必要に応じた完全な `ty` と `declaration_identity` を保持する。structured envelope は `reason`、`origin`、`data`、`primary`、`related`（および remediation）を持つ。
- Human-readable 出力（`DiagnosticSpec` の message / labels / notes / help）と JSON は、同じ structured input から投影する。自然言語、label 本文、source text を再解析して reason や typed field を推測しない。
- JSON の既存フィールド `kind`、`phase`、`line`、`column`、`span`、`message`、`expected`、`got`、`hint` は意味を変えずに保持し、`reason`、`origin`、`data`、`related` を additive に追加する。
- 移行中の未移行診断は明示的に不安定な legacy payload として扱い、legacy message から安定した reason を推測したり、偽の reason を付与したりしない。各診断 family の移行完了時に対応する legacy payload を削除する。

## 型関係と呼び出しの診断（実装済み）

- Scar の `assert_type_relation` は、失敗時に部分的な型代入と capability / obligation の変更を rollback する。成功した制約だけを後続へ渡す。両側の source fact は照合対象の型とともに保持する。
- 呼び出しの arity / mode / named 引数、通常の型関係、Trait obligation / dispatch、constructor family / payload / capability は共通の reason を使う。演算子と対応する helper は同じ検査を通し、文脈の違いを `DiagnosticOrigin` に保持する。入れ子の呼び出し・annotation の失敗を外側の演算子へ付け替えない。
- `cond` の節と `if_let` の発生文脈は Spire / Sigil から Scar まで保持する。分岐診断には全 body の型・span・ordinal と guard の source fact を含める。`cond` の実行は型検査後に既存の `TypedInner::If` へ lowering する。
- JSON の `data` は source location の rebase 後に typed projection から生成する。必須 key は省略せず、該当しない値は `null` にする。`related` は primary fact も含み、型は `type`、source role は `left_value` / `right_value` などの snake_case とする。
- constructor `family_id` は同じ族の canonical Trait ID をソートして構成する。完全な source value の型には captured 引数と `Result` の error 型も含める。登録順や内部 inference ID を表示しない。
- structured input がある場合、optional field の欠落を理由に message / label / source の解析へ戻らない。未移行の policy / runtime 等の legacy 経路、SafeBind是正、heuristic全撤去は未実装であり、[`../../doc/diagnostics_cleanup_spec.md`](../../doc/diagnostics_cleanup_spec.md)を実装入力とする。

## stable reason と typed data

型関係・callable・Trait・TypeCtorTrait・branchの実装済み経路は、次の閉じた`TypeDiagnosticReason`を使う。
同じ意味の失敗にsurface名別のreasonを増やさず、call、Trait call、operator、annotation、return、branchなどの
違いは`DiagnosticOrigin`で表す。

| family | stable reason |
|---|---|
| argument contract | `ArityMismatch`, `ArgumentModeMismatch`, `UnknownNamedArgument`, `DuplicateArgument`, `MissingArgument` |
| type relation / callable | `ArgumentTypeMismatch`, `ReturnTypeMismatch`, `AnnotationTypeMismatch`, `NotCallable`, `CallableShapeMismatch`, `CallableSignatureMetadataMismatch` |
| ReturnTypeArgument | `ReturnTypeArgumentArityMismatch`, `ReturnTypeArgumentMismatch`, `AmbiguousReturnTypeArgument`, `DuplicateReturnTypeArgumentInput`, `MissingReturnTypeArgument`, `UnusedReturnTypeArgument`, `ConcreteReturnTypeArgumentInDefinition`, `InlineReturnTypeArgumentConstraint` |
| constraint / Trait | `InvalidTraitConstraintSubject`, `MissingGenericBound`, `MissingTraitCapability`, `NoApplicableTraitImplementation`, `UnresolvedTraitMethodInstantiation`, `MissingTraitDispatchTarget` |
| TypeCtorTrait | `MissingTypeConstructorConstraint`, `TypeConstructorFamilyMismatch`, `TypePayloadMismatch`, `MissingTypeConstructorCapability` |
| Trait method contract | `TraitMethodTypeListMismatch`, `TraitMethodTypeListArityMismatch`, `TraitMethodConstraintMismatch` |
| branch | `IfBranchTypeMismatch`, `MatchArmTypeMismatch`, `CondBranchTypeMismatch` |

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
| `CallableSignature` | `callable`, `role`, expected/actual count, `detail` |
| `TraitMethodTypeList` / `TraitMethodConstraint` | optional `identity`、`method_name`、role/ordinal/nested path、expected/actual type・countまたはconstraints、`impl_declaration` |
| `TraitDispatch` / `CandidateSelection` | `trait_name`, `trait_arguments`, `subject_type`, method, `impl_declaration`。後者はcandidate failuresも保持する |
| `ConstraintSubject` / `TraitObligation` | subject/constraint、またはtrait name/arguments・subject type・position |
| `TypeConstructorCarrier` | `family`, `family_id`, `expected_carrier`, `actual_carrier` |
| `BranchAssertion` | `expected_type`, `actual_type`, `branch` |
| `SafeBindRelation` / `Policy` / `Runtime` | 現行payload。N06でprojectionとfailure targetを拡張する |

constructor-context経路の`CandidateFailureData`は候補ごとの型と失敗detailを保持する。通常のTrait候補選択は
内部`CandidateFailure`からrelated factsとsummary noteを構築する。どちらも全候補reject時に情報を捨てて
一般callable経路へfallbackせず、最終診断はrequested obligation全体から作る。一候補のlocal failureを
そのまま最終reasonにしない。共通のtyped candidate projectionへの統合はN06の残件である。

remediationはbase reason、expected/actual type、primary spanを変更しないoverlayである。具体的な変換案は
canonical identityと可視なconversion implから一意に裏付けられる場合だけ追加し、rendered type名やmessageの
文字列一致から選ばない。

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
- contextual type syntax では、`Trait<...>` を where RHS に置いた parser diagnostic、`Type<...>` の位置違反、constructor application の位置違反、`Self::...` / `Type::...` の owner-path 違反を parser phase にする。position rule は `notes`、bare bound や許可された位置への書換えは `help` に置く。
- `Enum<...>::Variant` は Spire で callable の `::<...>` と別の expression として保持する。owner が enum でない、variant が owner に属さない、owner 型引数 arity が一致しない場合は Sigil の resolve diagnostic とする。payload または expected type と明示型引数が一致しない場合は Scar の既存 type relation / argument diagnostic とする。
- bare capability の未使用、fresh result witness の未確定、full obligation / pending dispatch の未解決は typecheck phase にする。position rule は `notes`、constraint の削除または必要な式の利用は `help` に置く。

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
