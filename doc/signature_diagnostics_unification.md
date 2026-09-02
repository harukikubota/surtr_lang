# シグネチャ診断統一仕様

## 1. 状態と正本関係

本書は、通常関数、Trait method / helper、非 intrinsic builtin、operator lowering を、
シグネチャレベルの共通型検査・型推論・診断経路へ移行するための docs-only implementation input である。

次を正本とし、本書はその診断統合境界を具体化する。

- [`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md): `message` / `labels` / `notes` / `help` と既存 JSON の役割
- [`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md): Trait identity、TypeCtorTraitFamily、phase ownership
- [`return_type_argument_rules.md`](return_type_argument_rules.md): ReturnTypeArgument の構文、導入、推論、failure
- [`trait_method_type_list_dispatch.md`](trait_method_type_list_dispatch.md): role 付き型リスト、候補 applicability、static dispatch

本書の規則は上記正本と現行実装の監査結果から再構成した確定入力である。旧ドラフトは正本、参照先、
互換要件にせず、本書の実装時に読み戻さない。

## 2. 目的

本変更の目的は、surface 名ごとの型検査と完成済み文章の再解析を廃止し、次を一つの構造化経路で得ることである。

1. callable signature に基づく arity、argument、return、Trait obligation、型推論
2. ReturnTypeArgument、通常型変数、TypeCtorTrait carrier、mapped payload の制約解決
3. 意味上の failure reason、source origin、完全な型、関連 span
4. 同じ入力から生成する Ariadne と JSON
5. standard type 固有の修正案を、意味論データに基づき追加する remediation overlay

通常callable routeに属する関数と builtin、helper と operator、standard type と user-defined typeは、同じ失敗なら
同じreasonを持つ。surfaceの違いはreasonではなくoriginとcontext layoutで表す。special formとpolicyの固有reasonは
第6節の境界に従って維持する。

## 3. 非目標

- phase 固有の `ParseError`、`ResolveError`、`TypeError`、`CodegenError`、`RuntimeError` を一つの enum に統合しない。
- Ariadne の色、罫線、空白、label 順序を安定契約にしない。
- Trait coherence、parent coverage、constructor slot mapping の意味を変更しない。
- runtime trait object、dictionary、dynamic dispatch を追加しない。
- intrinsic、source policy、runtime policy を通常 callable signature だけで表現しない。
- compiler invariant や壊れた bytecode を利用者向け型診断へ偽装しない。
- `do` intrinsic の構文、型生成、lowering の詳細を定めない。第 13 節の診断境界だけを定める。

## 4. 基本不変条件

### 4.1 reason、origin、typed data

phase error は少なくとも次の構造化情報を保持する。

```text
PhaseDiagnostic {
  reason,
  origin,
  primary: SourceFact,
  related: Vec<SourceFact>,
  data: DiagnosticData,
  remediation_candidates,
}

SourceFact {
  role,
  source_id,
  span,
  ty,
  declaration_identity,
}

DiagnosticData =
  | ArgumentRelation { ... }
  | ReturnTypeArgument { ... }
  | ConstraintSubject { ... }
  | TraitObligation { ... }
  | TraitDispatch { ... }
  | TypeConstructorCarrier { ... }
  | BranchAssertion { ... }
  | Policy { ... }
  | Runtime { ... }
```

- `reason` は意味上の失敗であり、callable 名、operator 記号、standard type 名を分類軸にしない。
- `origin` は `Call`、`TraitCall`、`Operator`、`Annotation`、`Return`、`Branch`、`Pattern`、
  `Declaration`、`Intrinsic`、`Runtime` などの source context である。
- `data` は reason ごとの閉じた型付き variant である。自由形式の message や文字列 map を主要契約にせず、
  reason を追加するときは対応する `DiagnosticData` variant と JSON schema を同時に追加する。
- `SourceFact.ty` は source 上の値に結び付く場合、caption に必要な完全な型を保持する。
- renderer は reason と typed data から `DiagnosticSpec` を作り、自然言語から reason や型を復元しない。

phase 固有 error 型は維持する。共通化するのは envelope の概念契約と renderer 入力であり、
parse error に typecheck data を持たせるような cross-phase union ではない。

### 4.2 表示と JSON の単一入力

`DiagnosticSpec.message`、`labels`、`notes`、`help` と JSON は、同じ `PhaseDiagnostic` から別々に投影する。

```text
phase checker
  -> structured reason / origin / typed data
      -> DiagnosticSpec -> Ariadne
      -> SerializableDiagnostic -> JSON
```

禁止事項:

- `message.starts_with`、`strip_prefix`、`contains`、`split_once` で template を選ぶこと
- message から `expected` / `got` / operand type / Trait 名を抽出すること
- hint 内へ signature や span を埋め込み、renderer が再解析すること
- label 本文を分類 marker として再利用すること
- source text を検索して declaration identity や call target を推測すること

source search は移行前 fallback にだけ存在し得る。対象 reason を構造化した時点で、その reason の fallback を削除する。
新規診断には fallback message だけの経路を追加しない。

### 4.3 `DiagnosticSpec` の役割

- `message`: 短い主原因。修正命令、長い規則説明、standard type の候補列挙を置かない。
- `labels`: source span に対応する事実。値の完全な型、明示指定、関連宣言、衝突位置を置く。
- `notes`: 同一型、同一 carrier、位置別 capability、special form の規則など、source を直接指さない説明を置く。
- `help`: 利用者が取る操作。型注釈、ReturnTypeArgument、変換、宣言書換えを置く。

## 5. 共通 callable route

### 5.1 正規化入力

通常関数、Trait method / helper、非 intrinsic builtin は、Scar の call 検査前に同じ形へ正規化する。

```text
CallableSignature {
  callable_identity,
  return_type_arguments: Vec<CanonicalReturnTypeArgument>,
  value_parameters: Vec<CanonicalValueParameter>,
  return_type: CanonicalTypeOccurrence,
  where_constraints: CanonicalConstraintSet,
  runtime_target,
  declaration_origins,
}

CanonicalReturnTypeArgument {
  ordinal,
  ty: CanonicalTy,
  origin,
}

CanonicalValueParameter {
  ordinal,
  name,
  mode,       # positional / named / variadicなど
  ty: CanonicalTy,
  origin,
}

CanonicalTypeOccurrence {
  ty: CanonicalTy,
  origin,
}
```

`runtime_target` は user function、builtin ID、concrete Trait dispatch などを型推論後へ運ぶ情報である。
runtime target の違いを型推論 route の違いにしてはならない。
parameter name / mode / ordinalはnamed argumentとarityの意味入力であり、diagnostic装飾だけのmetadataへ落とさない。

operator は parser / resolver が operator identity と operand span を保持し、Scar で対応する full Trait obligation と
callable invocation へ lower する。lower 後は通常の Trait call と同じ signature、candidate applicability、
constraint solving、failure reason を使う。operator 記号は `Origin::Operator` に残す。

### 5.2 制約収集

call-site では次を検査順に依存しない constraint set として収集する。

- call-site ReturnTypeArgument
- 各 value argument の型と span
- call expression に与えられた expected return type と origin
- closure parameter / return の既知 shape
- callable signature の repeated type relation
- Trait arguments、obligation subject、`where` obligations
- TypeCtorTraitFamily の carrier identity と位置別 capability

impl 数、builtin 登録順、Trait impl 登録順を未拘束入力の既定値に使わない。結果は `Solved`、`Deferred`、`Failed` を
区別し、definition、callable instantiation、program の各 boundary で残った `Deferred` を ambiguity として拒否する。

### 5.3 ReturnTypeArgument

ReturnTypeArgument は [`return_type_argument_rules.md`](return_type_argument_rules.md) の導入規則をそのまま使う。

- value parameter 由来入力との二重導入を拒否する。
- return-only 入力は定義側 ReturnTypeArgument で宣言する。
- call-site list の省略は全項目を `_` にした制約と同じである。
- 明示 list は arity を厳密一致させる。
- value argument、明示項目、expected return の全制約を一度に解く。
- head だけが明示され、carrier の固定引数が未確定なら ambiguity とする。

call-site ReturnTypeArgument の source label は `::<Option>` のような型名だけでもよい。
一方、値同士、値と expected return、値と declaration contract が衝突する場合は、関係する各 span に完全な型を表示する。
annotation / value、expected / value、contract / impl、branch / branch、left / right の任意の二 origin が衝突した場合、
phase error は双方を `primary` / `related` の `SourceFact` として保持し、Ariadne は双方の span に完全な型を表示する。
型名だけの caption を許す例外は call-site ReturnTypeArgument origin だけである。

### 5.4 TypeCtorTraitFamily と carrier

同じ callable 内で同じ `TypeCtorTraitFamilyId` に属する全 occurrence は、一つの carrier を共有する。
carrier identity は nominal constructor head、mapped slot、mapped slot 以外の captured / fixed type arguments から構成する。
mapped payload は通常の型変数関係で検査する。

- `Option<Int>` と `Option<String>` は同じ `Option<_>` carrier になり得る。
- `Either<String, Int>` と `Either<String, Boolean>` は同じ `Either<String, _>` carrier になり得る。
- `Either<String, Int>` と `Either<Error, Boolean>` は captured argument が異なる carrier である。
- mapped payload 内に別 container が現れても、それを外側 carrier occurrence と誤認しない。
- capability view は signature 位置ごとに保持し、同じ carrier だから上位 capability を暗黙付与しない。

data type 名を文字列比較して carrier を選択、比較、既定化してはならない。canonical Type identity、family identity、
slot metadata、captured arguments を使う。

### 5.5 Trait method 型リスト

Trait contract、impl method、invocation は role 付き順序型リストで検査する。

```text
MethodSignatureTypeList = [
  ReturnTypeArgument(0..r),
  ValueParameter(0..p),
  ReturnType(0),
]
```

Trait arguments と impl target、上記 method list、`where` obligations は一つの fresh substitution environment を使う。
候補選択後に value argument だけを zip して mapping を作り直してはならない。不一致は structural type path、role、ordinal、
contract origin、impl origin を保持する。表示名、source generic 名、内部 variable 番号、登録順を identity にしない。

### 5.6 non-intrinsic builtin の正本と統合順

non-intrinsic builtin の追加・変更は `crates/sindr/src/builtin.rs` の `BUILTIN_METAS` だけを起点にする。
一つのruntime builtinと、一つ以上のsurface callable signatureを分離して保持する。

```text
BuiltinMeta {
  runtime_name,
  builtin_id,          # table ordinalから導出
  runtime_arity,
  runtime_signature,
  surface_variants: Vec<BuiltinSurfaceSignatureMeta>,
}

BuiltinSurfaceSignatureMeta {
  callable_identity,   # canonical owner + surface name + declaration kind
  return_type_arguments,
  value_parameters,    # name、mode、型、ordinalを含む
  return_type,
  where_constraints,
}
```

現行の`name`、`arity`、`sig_str`はruntime contractを表すfieldとして維持・構造化できる。これとは別に、同じ
`BuiltinId`へ`Int::safe_div`と`Float::safe_div`のような異なるsurface specialization、qualified owner、parameter名、
named argument modeを関連付けられなければならない。surface variantの登録順をoverload選択へ使わず、canonical
callable identityで取得する。

`BUILTIN_METAS`だけから各surface variantの完全な`CallableSignature`と共有runtime targetを構築できなければならない。
`lib/*.srt`のsource `@builtin def`は利用者向けdocとsource provenanceを与えるsurfaceであり、parameter名を含む
ReturnTypeArgument / value parameter / return / `where`契約を追加・上書きしてはならない。

Scar predeclare は次の順で一つの `CallableSignature` を構築する。

1. `BUILTIN_METAS`からcanonical runtime identity、`BuiltinId`、全surface variantsを取得する。
2. source `@builtin def`のcanonical owner / nameから対応surface variantを一意に取得する。
3. variantからReturnTypeArguments、named / positional mode付きvalue parameters、return、canonical `where` setを持つ
   `CallableSignature`を構築する。
4. source declarationを同じrole付き型リスト、parameter name / mode、constraint setへ正規化し、variantと完全一致することを検証する。
5. 一致したsource declarationのdocとprovenanceだけをcanonical signatureへ関連付ける。
6. `runtime_target`を共有`BuiltinId`として保持し、通常関数と同じargument、ReturnTypeArgument、expected return、
   obligation solverへ登録する。

対応variantがない、複数variantが同じidentityを持つ、またはsource declarationがvariantと一致しない場合はdeclaration検証errorとし、
片方を黙って優先しない。`BuiltinId`はsurface callable identityでも型推論入力でもなく、全constraint解決後に選択済みruntime targetとして
運ぶ。`@intrinsic`とcontrol-flow / evaluation policyを持つspecial formはこの通常callable登録の対象外だが、その内部の通常型relationは
共通type-assertion primitiveを使う。

### 5.7 共通 reason family

最終的な Rust variant 名は phase error の構造に合わせてよいが、JSON の安定 reason 名は実装開始時に固定する。

| family | reason | 意味 |
|---|---|---|
| arguments | `ArityMismatch` | positional / named を正規化した後の項目数不一致 |
| arguments | `ArgumentModeMismatch` | named と positional の禁止された混在 |
| arguments | `UnknownNamedArgument` / `DuplicateArgument` / `MissingArgument` | 引数名契約の違反 |
| type relation | `ArgumentTypeMismatch` | parameter と argument の通常型関係不一致 |
| type relation | `ReturnTypeMismatch` | callable return と expected return の不一致 |
| type relation | `AnnotationTypeMismatch` | 注釈と値の不一致 |
| callable | `NotCallable` / `CallableShapeMismatch` | function shape がない、または arity / input shape が違う |
| ReturnTypeArgument | `ReturnTypeArgumentArityMismatch` | call-site または impl method の項目数不一致 |
| ReturnTypeArgument | `ReturnTypeArgumentMismatch` | 明示項目と他制約の衝突 |
| ReturnTypeArgument | `AmbiguousReturnTypeArgument` | boundary まで入力が一意に決まらない |
| constraint | `InvalidTraitConstraintSubject` | Trait 名など、値型でない subject を constraint 左辺に置いた |
| Trait | `MissingGenericBound` | rigid generic に必要な宣言済み bound がない |
| Trait | `MissingTraitCapability` | concrete subject または確定 carrier occurrence が signature 位置の capability を満たさない |
| Trait | `NoApplicableTraitImplementation` | 確定した full obligation を満たす impl がない |
| Trait | `UnresolvedTraitMethodInstantiation` | body / dispatch に必要な型入力が残る |
| Trait | `MissingTraitDispatchTarget` | applicable impl に concrete dispatch target がない |
| type constructor | `MissingTypeConstructorConstraint` | constructor application の根拠がない |
| type constructor | `TypeConstructorFamilyMismatch` | 同じ family occurrence が異なる carrier を要求する |
| type constructor | `TypePayloadMismatch` | repeated mapped payload の通常型関係が衝突する |
| type constructor | `MissingTypeConstructorCapability` | occurrence の capability view が不足する |
| contract | `TraitMethodTypeListArityMismatch` | role ごとの entry 数が contract と違う |
| contract | `TraitMethodTypeListMismatch` | contract と impl の構造または変数関係が違う |
| contract | `TraitMethodConstraintMismatch` | canonical method `where` 集合が contract と違う |

ReturnTypeArgument の definition well-formedness reason は正本の
`DuplicateReturnTypeArgumentInput`、`MissingReturnTypeArgument`、`UnusedReturnTypeArgument`、
`ConcreteReturnTypeArgumentInDefinition`、`InlineReturnTypeArgumentConstraint` を維持する。

`MissingGenericBound` は宣言境界にある rigid generic の不足、`MissingTraitCapability` は既に concrete な subject または
carrier occurrence の位置別 capability 不足であり、相互に置換しない。inference variable の obligation は `Deferred` のまま保持する。
`MissingTypeConstructorCapability`はTypeCtorTraitFamilyのcarrier occurrenceに限り、通常Trait subjectの
`MissingTraitCapability`と分ける。
call-site の constructor head または captured / fixed argument が boundary まで未確定なら、独自の constructor ambiguity を
作らず `AmbiguousReturnTypeArgument` とする。

## 6. 固有診断を残す境界

### 6.1 原則

専用 reason を残せるのは、失敗条件が callable signature の arity、通常型関係、Trait obligation、carrier relation だけでは
表現できない場合である。専用 checker 内でも、引数と値の型関係を再実装してはならない。

```text
special-form checker
  -> context / policy constraints
  -> shared type-assertion primitive / substitution / typed facts
  -> dedicated special-form reason / kind / message + context layout
```

### 6.2 複数 block / arm の special form

`if`、`match`、`cond` など複数 branch / arm を一つの戻り値へ assert する form は、既存の専用 reason / kind / message と
context layout を維持する。各 arm の expected / actual 検査は共通 type-assertion primitive、substitution、typed facts を使うが、
その内部 failure を最終 `ReturnTypeMismatch` または `TypePayloadMismatch` へ置換しない。

| form | stable reason | 維持する message 意味 |
|---|---|---|
| `if` / `if_let` | `IfBranchTypeMismatch` | `if branches have different types: {then} and {else}` |
| `match` | `MatchArmTypeMismatch` | `Match arm type mismatch: expected {expected}, got {actual}` |
| `cond` | `CondBranchTypeMismatch` | 現行の cond 固有 branch mismatch headline |

- context は branch / arm 番号、guard、全体 expected type を保持する。
- 各 arm body の span と完全な型を phase error が直接保持する。
- renderer が source を再走査して arm span を推測しない。
- exhaustiveness、pattern legality、binding relation は branch return type と別 reason にする。

### 6.3 網羅分類

| 分類 | 固有 reason を残す根拠 | 共通部分との境界 | 現行の代表箇所 |
|---|---|---|---|
| parser syntax / position | token、許可位置、source grammar は型シグネチャで表せない | parse 後に得た callable signature の型関係は Scar へ渡す | `crates/spire/src/error.rs`, `crates/spire/src/parser/diagnostic.rs` |
| resolve name / namespace / visibility / import | canonical identity を得る前の失敗 | identity 解決後の call arity / type は共通 route | `crates/sigil/src/resolver/declarations.rs:90`, `imports.rs:593`, `expr.rs:3924` |
| capture / placeholder policy | `_1` の位置、nested capture、binding 禁止は構文 policy | capture target の callable shape は共通 reason | `crates/sigil/src/resolver/expr.rs:397`, `expr.rs:1427`, `special_forms.rs:232` |
| branch context | 複数 arm 全体の関係を一つの layout で示す必要がある | arm ごとの assertion primitive / substitution / typed facts だけ共通化し、最終 reason / kind は維持 | `crates/scar/src/checker/matching.rs`, `crates/diagnostics/src/heuristics/type_templates_tail.rs` |
| pattern / extractor legality | pattern shape、binding、extractor input contract、error pattern policy | scrutinee と pattern payload の型 relation は共通 reason | `matching.rs:396`, `matching.rs:558`, `matching.rs:584`, `expr.rs:6272` |
| exhaustiveness | missing case 集合は signature mismatch ではない | arm body の return relation と分離 | `crates/scar/src/checker/matching.rs:177` |
| assignment `=` policy | total MatchBlock pattern だけを許す | LHS annotation と RHS 型は共通 relation | `crates/scar/src/checker/expr.rs:1018` |
| SafeBind `=?` | Result RHSだけを一段自動分解すること、failure target、partial matchの構文意味を持つ | non-Result RHSとLHS pattern、Result payloadとLHS pattern、errorとfailure targetの照合は共通relation | `crates/scar/src/checker/expr.rs:1860` |
| Error / `deferror` policy | `Err` wrapping、escaping、error kind pattern は言語 policy | constructor payload の型は共通 relation | `expr.rs:1081`, `matching.rs:584` |
| Facet | path kind、compile-time value、rebuild、update permission は signature 外の意味論 | Facet API の通常 arity、callable argument、expected / actual は共通 route | `expr.rs:6740`, `expr.rs:6994`, `expr.rs:7913`, `expr.rs:8389` |
| Process / Task | handler scope、state、lifecycle、timeout は effect / lifecycle policy | handler callable signature と値型は共通 route | `expr.rs:8677`, `crates/scar/src/checker/mod.rs:2964`, `crates/eldr/src/error.rs:11` |
| source / compile policy | source kind、standby init、supervisor policy は配置・lifecycle 契約 | callable signature で表せる部分は Scar へ rehome | `crates/scar/src/checker/mod.rs:3096`, `crates/forge/src/codegen.rs:897` |
| runtime value / VM | 実値、timeout、call stack、opcode context は静的 signature の外 | 静的に証明できる型不一致を runtime へ送らない | `crates/eldr/src/error.rs`, `crates/diagnostics/src/heuristics/runtime_impl.rs` |
| compiler invariant | unresolved dispatch、未知 builtin、壊れた label は利用者の修正対象ではない | Scar boundary で user failure と invariant を分離 | `crates/forge/src/codegen.rs:6817`, `codegen.rs:8903` |

### 6.4 SafeBind の明確な境界

`=?` はRHSがcanonical `Result<A, E>`の場合だけ`Ok` payload `A`を一段自動的に取り出し、`Err<E>`をfailure targetへ
送る。それ以外のRHS `T`はcontainer種別を検査せず、値と型`T`をそのままLHSの通常MatchBlock pattern検査へ渡す。
`Option`を表示名またはcanonical identityで拒否したり、`Option::Some`を自動的に取り出したりしてはならない。

Result payloadまたはnon-Result RHSとLHS patternの衝突、RHS errorとeffective failure targetの衝突は共通type relation /
pattern reasonとして報告する。通常のSafeBindではenclosing return、do内では
[`do_intrinsic_spec.md`](do_intrinsic_spec.md)第8節が選ぶResult-preservingまたはAlternative-empty targetを使い、
rendererがcontext名や型表示からtargetを再判定しない。

### 6.5 Facet、Process、Task の明確な境界

Facet の path consistency、deferred update slot、mutation permission、case update policy、compile-time-only 値は専用 reason である。
ただし `Facet::chain` などの引数数、named argument、通常 callable shape を Facet 名で再実装しない。

Process / Task の handler 可視性、handler 内限定 API、state transition、supervision、timeout は専用 reason である。
handler parameter / return、closure、通常 builtin argument の型検査は `CallableSignature` 経路を使う。

## 7. 現行実装監査と移行 inventory

### 7.1 診断パイプライン

| 現行 | 問題 | 移行結果 |
|---|---|---|
| `crates/scar/src/error.rs` | `TypeError` が `message` / `span` / `hint` しか持たない | typecheck reason、origin、typed data、related facts を追加し phase 型を維持 |
| `crates/diagnostics/src/typecheck.rs::type_error_spec` | message prefix と hint を解析して label / help を生成 | structured type diagnostic から template を選択 |
| `crates/diagnostics/src/heuristics/type_templates_core.rs::infer_type_error_template` | operator message を多数の prefix で再解析 | operator origin と operand facts を Scar から渡す |
| `crates/diagnostics/src/heuristics/type_templates_extra.rs` の side 推定 helper | `contains` で left / right / both を推測 | `SourceRole::LeftValue` / `RightValue` を直接保持 |
| `crates/diagnostics/src/heuristics/type_templates_tail.rs` の branch template | `if` / `match` message と source text から branch span を復元 | branch context と arm facts を checker が保持 |
| `crates/diagnostics/src/heuristics/labels_impl.rs` の Trait method label 推定 | Trait method message と source text から declaration span を検索 | Trait contract / impl declaration origin を predeclare 時から保持 |
| `crates/diagnostics/src/heuristics/shared_impl.rs::extract_expected_got` | message から `expected` / `got` を抽出 | reason data から JSON field を生成 |
| `crates/diagnostics/src/render.rs::serializable_diagnostic_by_id` | JSON が `extract_expected_got` と label-derived hint に依存 | `reason` / `origin` / `data` / `related` を同じ typed input から serialize |
| `crates/rune/src/compile.rs` の resolve diagnostic layout | resolve message prefix で primary span layout を変更 | `ResolveOrigin` / related facts で layout を選択 |

### 7.2 共通 route へ移す型検査

| 対象 | 現行の代表箇所 | 移行 |
|---|---|---|
| generic call の arity / named argument / argument type | `crates/scar/src/checker/expr.rs:6438`--`6549`, `8580` 以降 | 一つの invocation builder と common argument checker |
| receiverless Trait helper witness | `expr.rs:3853`, `3894` | ReturnTypeArgument / expected return / signature constraint の ambiguity |
| Applicative / Monad helper | `expr.rs:3765`--`4458` | Trait method type list と共通 candidate applicability |
| operator / helper の arithmetic、concat、equality、comparison | `expr.rs` の binary operator 分岐、`crates/sindr/src/operator_diagnostics.rs` | operator lowering 後の full obligation と共通 reason |
| context map / apply / bind | `expr.rs:4855`--`5612` | Functor / Applicative / Monad signature と carrier solver |
| compose / lifted compose / Kleisli compose | `expr.rs:5653`--`6214` | callable shape、payload relation、carrier relation の共通 reason |
| `Result` / `List` / `Option` の名前別 flow 分岐 | `expr.rs:5421`--`5580`, `5760`--`6152` | canonical carrier / payload metadata。型名比較を削除 |
| non-intrinsic builtin | `crates/sindr/src/builtin.rs::BUILTIN_METAS`、`definitions.rs::check_builtin_decl`、`expr.rs` の `Ty::BuiltinFunc` call 分岐 | runtime entryごとに完全なsurface variantsを持たせ、canonical callable identityでsource declarationを完全一致検証して通常routeへ登録 |
| Trait contract / impl signature | `crates/scar/src/checker/predeclare.rs:3047`--`3164` | role 付き型リスト、structural path、両宣言 origin |
| expected propagation | `expr.rs:1405`, `matching.rs:4` | 型 shape に沿う共通 bidirectional check。callable 名による注入なし |

### 7.3 固有 reason として残す監査対象

| family | 現行の代表箇所 | 移行時の判断 |
|---|---|---|
| where position、Trait shape、slot mapping | `predeclare.rs:25`--`277`, `1552`--`1588` | declaration well-formedness reason を維持 |
| coherence、parent cycle、coverage | `predeclare.rs:1636`--`1801`, `3403`--`3426` | Trait system 専用 reason を維持 |
| duplicate / undefined / visibility / import | Sigil `declarations.rs`, `imports.rs`, `patterns.rs` | resolve reason と canonical identity / provenance を構造化 |
| pattern legality / exhaustiveness | `matching.rs:177`--`826` | pattern / exhaustiveness reason を維持し、type relation だけ共通化 |
| extractor context | `expr.rs:6272`--`6410` | extractor identity、definition origin、input type を直接保持 |
| SafeBind | `expr.rs:1860`--`1926`, `patterns.rs:298`--`333`, `lib/bootstrap.srt:96`--`135` | Result一段分解だけをpolicyとして残し、Option固有拒否とconstructor patternの`Ok`限定を削除。non-Result RHSは通常pattern relationへ渡し、payload / return relationを共通化 |
| Error policy | `expr.rs:1081`, `matching.rs:584`--`596` | error construction / pattern reason を維持 |
| Facet | `expr.rs:6740`--`8566` | Facet semantics reason と common call/type reason を分離 |
| Process / supervisor / handler | Scar `mod.rs:2964` 以降、Forge `codegen.rs:832` 以降 | user policy は構造化して適切な phase に置き、Forge invariant と分離 |
| RuntimeErrorKind | `crates/eldr/src/error.rs:6`--`103` | runtime reason / context を JSON data へ直接渡す |

special form と builtin 周辺は、次の単位を漏れなく監査する。

| 現行 form | 固有 policy / reason | 共通化する signature 部分 | invariant |
|---|---|---|---|
| `if` / `if_then` / `cond` | branch 数、遅延評価、branch assertion の専用 reason / kind | condition、各 branch result の型アサーション | resolved branch span または lazy target の欠落 |
| `if_let` / `if_let_then` / `is_match` | pattern legality、binding、match context | scrutinee、pattern payload、branch result の型アサーション | resolved pattern / extractor identity の欠落 |
| `assert` / `ensure` | failure生成、lazy error、predicate policy | Boolean、predicate callable、value / result payload relation | lowering target または builtin identity の欠落 |
| `map_err` / `cause` / `recover_kind` | Error propagation / recovery policy | argument、handler callable、result payload relation | resolved error kind / target の欠落 |
| lazy `and` / `or` | short-circuit と RHS lazy evaluation | Boolean parameter / return relation | lazy lowering target の欠落 |
| pair constructor `(,)` | tuple construction identity | arity、left / right payload と tuple return relation | canonical tuple constructor identity の欠落 |
| `dbg!` | 評価回数と observability policy | payload と戻り値が同じ型である assertion | debug lowering target の欠落 |

現行の `definitions.rs::builtin_contracts`、`Ty::BuiltinFunc` 専用 call 分岐、
`expr.rs::check_builtin_contract` の専用 obligation route は、non-intrinsic builtin の `CallableSignature` と共通 solver への移行後に
削除する。special form の declaration shape 検証は固有 policy として残せるが、通常 arity / argument / return / `where` relation を
名前別に再実装してはならない。

### 7.4 metadata と正規語彙

Spire AST / parser、Sigil Resolved IR / resolver / derive、Scar metadata / predeclare / expression checker、diagnostics、unit / fixture tests、
rustdoc、site docs に残る旧 `fun_params` / `FunParams` と旧 ReturnTypeArgument 相当 field は、
`return_type_argument(s)` と `value_parameter(s)` へ置換する。互換 field、alias、二重保持、旧 serialized cache 読み戻しは追加しない。
代表箇所だけで完了判定せず、repository 全体を inventory とする。

移行完了時は次が出力なしで成功しなければならない。本書自身は旧語彙の撤去規則と検証 command を説明するためだけに allowlist する。

```bash
rg -n '\b(fun_params|FunParams)\b' crates tests lib docs doc \
  --glob '!doc/signature_diagnostics_unification.md'
```

## 8. remediation overlay

### 8.1 規則

base reason と base template は standard / user-defined type で共通にする。standard type 固有の変換案や policy 説明は、
構造化 semantic data から選ぶ overlay である。

overlay の入力にできるもの:

- canonical source / target type identity
- 可視な `From` / `TryFrom` などの concrete impl
- 選択済み Trait implementation と capability closure
- SafeBind、Facet、Process などの明示 policy classification
- source scope と visibility

overlay の入力にしてはならないもの:

- rendered type string の prefix / substring
- callable / helper / builtin の表示名
- message、note、help の文字列
- impl table の先頭要素や登録順

可視な変換 impl が一意なら conversion help を追加できる。変換が複数、不可視、または意味を変える場合は、一般的な
「同じ carrier に揃える」「型注釈を追加する」までに留める。overlay は reason、expected / got、primary span を変更しない。

## 9. 診断例

### 9.1 operator と helper の parity

`1 + "x"` と同じ signature を呼ぶ helper は、どちらも `ArgumentTypeMismatch` を返す。

```text
message: arguments do not satisfy the callable signature
label 1: left value has `Int`
label 2: right value has `String`
note: both parameters must instantiate the same signature type
help: pass values with compatible types or provide the required implementation
```

operator は `origin = Operator("+")`、helper は `origin = TraitCall(Add::add)` とする。reason と typed types は同じである。

### 9.2 carrier mismatch

```text
message: values require different type-constructor carriers
label 1: left value has `Either<String, Int>`
label 2: right value has `Either<Error, Boolean>`
note: positions in the same TypeCtorTraitFamily must use one carrier
help: use the same fixed carrier arguments or convert one value explicitly
```

left / right value の各 span に完全な型を caption する。JSON は `left_type`、`right_type`、`left_origin`、
`right_origin`、`family_id`、`required_capability` を typed data と related facts から出す。

### 9.3 call-site ReturnTypeArgument conflict

```text
message: return type argument conflicts with the call constraints
label 1: explicit type: `Option`
label 2: expected result has `Result<Int>`
note: explicit and inferred return type inputs must agree
help: remove the explicit item or choose the constructor required by the expected result
```

明示項目 label は型名だけでよい。expected result は完全な型を表示する。

### 9.4 match arm mismatch

```text
message: Match arm type mismatch: expected `Result<Int>`, got `Option<Int>`
label 1: arm 1 returns `Result<Int>`
label 2: arm 2 returns `Option<Int>`
note: every arm of this match must satisfy one result type
help: make the arm results agree or convert one arm explicitly
```

reason / kind は match 固有の `MatchArmTypeMismatch`、origin は `Branch { form: Match, ordinal: 2 }` とする。
arm 1 / arm 2 の型アサーションは共通 primitive と substitution を使い、両 arm の typed facts を保持するが、最終 reason を
`ReturnTypeMismatch` へ置換しない。exhaustiveness failure と混ぜない。

### 9.5 SafeBind policy

```text
message: SafeBind pattern type mismatch
label 1: this pattern requires an `Int` payload
label 2: this non-Result RHS is passed to the pattern as `Option<Option<Int>>`
note: only a Result RHS is automatically unwrapped by `=?`
help: match the Option layers explicitly, for example `Option::Some(Option::Some(num))`
```

Resultかどうかの判定はcanonical identityによるSafeBind policyだが、上のfailure自体はLHS patternと検査対象型の共通
pattern type relationである。`Option`という文字列や`=?`というcontextを理由に専用RHS rejectionへ置換しない。

## 10. JSON 契約

既存の `kind`、`phase`、`line`、`column`、`span`、`message`、`expected`、`got`、`hint` を維持し、
次を additive に追加する。

`DiagnosticData` は次の閉じた variant を最低限持つ。各 JSON `data` は `kind` discriminator と表中の typed field を必須 key として
serialize する。値がその reason に存在しない field は Rust 側で `Option` として表し JSON では `null` にする。任意 key の
string map や message から組み立てた field で代用しない。

| `DiagnosticData` variant | 対象 reason | 必須 typed fields |
|---|---|---|
| `ArgumentRelationData` | argument / annotation / return の通常型 relation | `ordinal`, `expected_type`, `actual_type`, `expected_origin`, `actual_origin` |
| `ReturnTypeArgumentData` | definition / call-siteの全ReturnTypeArgument reason | `return_type_argument_ordinal`, `declared_origin`, `value_parameter_origin`, `return_origin`, `left_type`, `right_type`, `left_origin`, `right_origin`, `required_trait` |
| `ConstraintSubjectData` | `InvalidTraitConstraintSubject`, constructor constraint不足 | `subject_type`, `subject_origin`, `required_trait`, `suggested_type_variable` |
| `TraitObligationData` | `MissingGenericBound`, `MissingTraitCapability` | `trait_id`, `trait_arguments`, `subject_type`, `obligation_origin` |
| `TraitDispatchData` | Trait method type-list / applicability / dispatch reason | `trait_id`, `trait_arguments`, `subject_type`, `method_name`, `type_list_role`, `ordinal`, `expected_type`, `actual_type`, `impl_declaration` |
| `TypeConstructorCarrierData` | carrier family / payload / capability reason | `family_id`, `left_type`, `right_type`, `left_origin`, `right_origin`, `required_capability` |
| `BranchAssertionData` | `IfBranchTypeMismatch`, `MatchArmTypeMismatch`, `CondBranchTypeMismatch` | `form`, `left_ordinal`, `right_ordinal`, `left_type`, `right_type`, `left_origin`, `right_origin` |
| `SafeBindRelationData` | SafeBind pattern input / error target relation | `rhs_projection`, `rhs_type`, `pattern_input_type`, `pattern_origin`, `rhs_origin`, `failure_target`, `error_type` |

`TraitDispatchData` の対象には `TraitMethodTypeListArityMismatch`、`TraitMethodTypeListMismatch`、
`TraitMethodConstraintMismatch`、`NoApplicableTraitImplementation`、`UnresolvedTraitMethodInstantiation`、
`MissingTraitDispatchTarget` を含む。`ReturnTypeArgumentData.required_trait` と nullable fields も key 自体を省略せず、schema を reason ごとに
変形させない。

`SafeBindRelationData.rhs_projection`は`unwrap_result_once`または`pass_through_non_result`のclosed enumとする。
`failure_target`は通常関数のenclosing Result、doのResult-preserving、doのAlternative-emptyをtyped discriminatorで表し、
messageやRHS表示型から復元しない。

JSONの`family_id`はprocess-localな連番や一つのfamily root名を出さず、TypeCtorTraitFamilyに属するcanonical Trait IDsを
stable sortして導出したsemantic identityをserializeする。複数rootを持つfamilyでも同じidentityにならなければならない。

```json
{
  "reason": "TypeConstructorFamilyMismatch",
  "origin": {
    "kind": "Operator",
    "operator": "|>="
  },
  "data": {
    "kind": "TypeConstructorCarrier",
    "family_id": "family:Alternative+Applicative+Functor+Monad",
    "left_type": "Either<String, Int>",
    "right_type": "Either<Error, Boolean>",
    "left_origin": { "kind": "left_value" },
    "right_origin": { "kind": "right_value" },
    "required_capability": "Monad"
  },
  "related": [
    {
      "role": "left_value",
      "source_id": 0,
      "span": [10, 14],
      "type": "Either<String, Int>",
      "declaration_identity": null
    },
    {
      "role": "right_value",
      "source_id": 0,
      "span": [20, 38],
      "type": "Either<Error, Boolean>",
      "declaration_identity": null
    }
  ]
}
```

- stable identity は `(phase, reason)` とする。
- `expected` / `got` は reason data から出す。message にその語がなくても出力できる。
- `hint` は最終 help / remediation projection から出す。
- `related` は role、source id、span、完全な型、nullable な declaration identity を持つ。
- internal inference variable、runtime 内部 ID、impl 登録順を出力しない。
- legacy fallback は偽の stable reason を付けない。対象 family の移行完了時に fallback を削除する。

## 11. phase ownership

### 11.1 Spire

- callable 種別に依存せず definition / call-site ReturnTypeArgument と各 source span を保持する。
- operator identity、operand span、special form の branch / arm span を失わない。
- token、空 list、inline constraint、位置違反など identity 不要の違反を parse reason として構造化する。
- renderer 用の完成文章を AST に埋め込まない。

### 11.2 Sigil

- callable、Trait、type、operator lowering target、declaration の canonical identity を解決する。
- helper alias を解決しても ReturnTypeArgument や value argument の role を変更しない。
- related declaration の source provenance を保持する。
- undefined、duplicate、visibility、import、source policy を resolve reason として構造化する。

### 11.3 Scar predeclare

- 全 callable を `CallableSignature` へ正規化する。
- non-intrinsic builtinは`BUILTIN_METAS`のruntime entry / `BuiltinId` / surface variantsからcanonical callable identityごとの
  `CallableSignature`を構築し、source `@builtin def`のparameter name / modeを含む全signatureを完全一致検証した後、docとprovenanceを関連付ける。
- ReturnTypeArgument 導入規則と TypeCtorTrait direct syntax を検査する。
- Trait contract / impl method の role 付き型リストと declaration origins を構築する。
- TypeCtorTraitFamily ID、carrier metadata、位置別 capability view を構築する。
- standard definition、derive、builtin、user definition を同じ metadata に載せる。

### 11.4 Scar expression checking

- call-site constraints を一度収集し、通常関数、Trait helper、builtin、lowered operator を同じ solver で解く。
- `definitions.rs::builtin_contracts`、`Ty::BuiltinFunc` 専用 call 分岐、`expr.rs::check_builtin_contract` の別 obligation solver を使わない。
- failure reason、origin、typed data、left / right / declaration facts を生成する。
- special form は context constraints を生成し、通常型関係を common solver に委譲する。
- expected type を callable 名ではなく型 shape に沿って伝播する。
- boundary まで残る carrier、ReturnTypeArgument、pending dispatch を typecheck error として拒否する。

### 11.5 diagnostics / Rune

- `crates/diagnostics` は reason template、origin layout、notes、remediation overlay を合成する。
- Ariadne と JSON は同じ structured input を受け取る。
- Rune は phase、source registry、structured diagnostic を渡し、message prefix で span を変更しない。
- compile-error fixture harness は rendered text から reason / types を抽出しない。

### 11.6 Forge / Eldr

- Forge は具体化済み type、callable、Trait dispatch だけを受け取る。
- unresolved dispatch や abstract carrier は Scar invariant で止める。
- Forge に残る user-facing process / supervisor policy は構造化して適切な phase ownership を決め、internal invariant と区別する。
- Eldr は runtime value、opcode、call site、stack、`RuntimeErrorKind` を runtime reason / data として保持する。

## 12. 移行順序

### 12.1 構造化契約

1. phase error に reason / origin / typed data / related facts を保持できる最小 interface を追加する。
2. `DiagnosticSpec` と JSON へ構造化入力を渡す。
3. JSON に additive field を追加し、fixture parser に `reason`、`origin`、`expected`、`got`、`not_contains` を追加する。
4. message-only fallback を未移行診断として明示する。

### 12.2 callable 基盤

1. user function、Trait method / helper、non-intrinsic builtin を `CallableSignature` へ正規化する。
2. builtin metadataをruntime entry + 完全surface variantsへ拡張し、source declarationを第5.6節の順で完全一致検証して共有runtime `BuiltinId`を関連付ける。
3. common argument builder、expected return constraint、closure shape propagation を導入する。
4. ReturnTypeArgument と Trait method role 付き型リストを同じ substitution へ接続する。
5. `builtin_contracts`、`Ty::BuiltinFunc` 専用 call / obligation route を削除する。
6. 旧 ReturnTypeArgument 相当 field と用語を互換表現なしで置換する。

### 12.3 reason family ごとの移行

1. arity / named arguments / ordinary argument / annotation / return
2. Trait contract / impl type list / applicability / dispatch ambiguity
3. arithmetic / concat / equality / comparison operator と helper parity
4. Functor / Applicative / Monad の map / apply / bind / compose
5. TypeCtorTrait carrier / payload / capability / ambiguity
6. branch / arm context と pattern / exhaustiveness 分離
7. SafeBindをResult一段分解／non-Result pass-through + 通常pattern relationへ統一し、Option固有拒否と
   constructor patternの`Ok`限定を削除する。続けてError、Facet、Process / Taskのcommon type relationと専用policyを分離する
8. parse / resolve / runtime / codegen policy の構造化

各 family は structured reason、Ariadne、JSON、fixture を同時に移行し、その family の文字列 heuristic を削除する。
全 family 完了後に `crates/diagnostics/src/heuristics.rs` 配下の user-facing message parsing を削除する。

## 13. `do` intrinsic との境界

`do`の構文、statement分類、Monad / Alternative obligation生成、loweringは
[`do_intrinsic_spec.md`](do_intrinsic_spec.md)で定める。本書が要求する境界は次だけである。

- `do` checker が生成した通常型 relation、Trait capability、carrier relation は本書の共通 reason を使う。
- block、statement、bind RHS、最終式の表示は `Origin::Intrinsic` の context layout として追加できる。
- `do` 固有 reason は statement legality や block policy など signature で表現できない規則に限定する。
- `do` 内でも data type 名、helper 名、impl 登録順による carrier 推論を行わない。carrier確定後にcanonical `Result`
  identityからSafeBind failure policyを選ぶことは推論ではなく、`do_intrinsic_spec.md`第8節の意味規則として分離する。
- `do` の詳細実装を本書の migration の前提にしない。

## 14. テストマトリクス

### 14.1 structured contract

- phase error の reason、origin、typed data、primary / related facts
- `DiagnosticSpec` の message / labels / notes / help の役割分担
- JSON の reason / origin / expected / got / data / related
- `DiagnosticData` 各 variant の必須 key、nullable key、JSON `kind` discriminator の serialization
- ReturnTypeArgument、Trait dispatch、carrier、branch assertion の全 typed field が message に依存せず serialize されること
- message を言い換えても JSON typed fields が変わらないこと
- source text や label 本文を変更しても reason が変わらないこと

### 14.2 callable parity

- user function、Trait method、auto-import helper、qualified helper、non-intrinsic builtin の同型 signature
- arity、named argument、argument type、return expected、ambiguity の同じ reason
- `BUILTIN_METAS`のsurface variantとsource `@builtin def`のowner / name / ReturnTypeArgument / parameter name・mode・型 / return / `where`の一致 / 不一致
- `Int::safe_div`と`Float::safe_div`など、異なるsurface variantsが同じruntime `BuiltinId`を共有すること
- surface callable identityと`BuiltinId`を取り違えず、variant登録順でoverloadを選ばないこと
- builtin が `builtin_contracts`、`Ty::BuiltinFunc` 専用 call 分岐、専用 obligation route を通らないこと
- builtin ID と user function target の違いが runtime target にだけ残ること
- intrinsic が通常 callable registry に誤登録されないこと

### 14.3 operator parity

- `+` / `Add::add`、`++` / `Concat::concat`、`==` / `Eq::eq`、comparison / `Compare`
- operator と helper が同じ reason / data を持ち origin だけ異なること
- operator LHS / RHS の各 span に完全な型があること
- user-defined impl と standard impl が同じ template を使うこと

### 14.4 ReturnTypeArgument / Trait type list

- 明示項目、全 `_`、list 省略、expected return、value argument の各制約源
- 任意の二制約源の衝突
- arity 不足 / 過剰、head 明示後の fixed argument ambiguity
- `InvalidTraitConstraintSubject` と constructor constraint 不足を区別すること
- rigid generic の `MissingGenericBound` と concrete subject / carrier の `MissingTraitCapability` を区別すること
- `TraitMethodTypeListArityMismatch`、`TraitMethodTypeListMismatch`、`TraitMethodConstraintMismatch`、
  `NoApplicableTraitImplementation`、`UnresolvedTraitMethodInstantiation`、`MissingTraitDispatchTarget` をそれぞれ維持すること
- contract / impl の両 declaration label、role、ordinal、nested structural path
- source generic 名の alpha rename と impl 登録順で結果が変わらないこと

### 14.5 TypeCtorTrait

- same-family carrier 一致 / 不一致、different-family 分離
- position-local capability view
- `Option<A>` / `Option<B>` の同一 carrier
- `Either<L,A>` / `Either<L,B>` の同一 carrierと、`Either<L,A>` / `Either<R,B>` の不一致
- mapped payload に別 container が入る成功
- standard / user-defined TypeCtorTrait の reason / template parity
- explicit ReturnTypeArgument と argument / expected carrier の衝突
- carrier 不一致の二地点に完全な型があること

### 14.6 special form / policy

- `if`、`match`、`cond` の全 branch / arm expected propagation
- branch return mismatch は form 固有 reason / kind / message + dedicated context layout を維持すること
- branch 内部の type assertion、substitution、typed facts だけが共通 primitive を使うこと
- `if_then`、`if_let`、`if_let_then`、`is_match`、`assert`、`ensure`、`map_err`、`cause`、`recover_kind`、lazy `and` / `or`、
  pair constructor、`dbg!` の固有 policy / 共通 signature / invariant 分類
- pattern mismatch と exhaustiveness が branch return mismatch から分離されること
- `=` total-pattern policy と通常 LHS / RHS type relation の分離
- SafeBindのcanonical Result一段分解とnon-Result pass-through
- `Option::Some(num) =? Option::Some(Option::Some(1))`で`num: Option<Int>`になり、Option RHS自体を拒否しないこと
- SafeBindのLHS constructor patternを`Ok`に限定せず、通常MatchBlock pattern relationを使うこと
- SafeBind policy とpattern input / payload / return relation の分離
- Facet path policy と Facet callable arity / type relation の分離
- Process / Task lifecycle reason と handler signature reason の分離
- runtime timeout / process failure が runtime reason / context を持つこと

### 14.7 remediation

- visible concrete conversion impl がある場合だけ変換案を追加
- user-defined conversion でも同じ overlay selection を使うこと
- rendered type 名を変えても overlay 判定が変わらないこと
- impl が不可視、複数、または不適切なら具体的変換を提案しないこと
- overlay が base reason、expected / got、primary span を変更しないこと

### 14.8 regression / commands

```bash
cargo nextest run -p diagnostics --lib
cargo nextest run -p scar --tests
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
cargo nextest run --workspace
```

family ごとの focused test を先に実行し、最後にworkspace全体を実行する。

## 15. 受け入れ基準

1. 通常関数、Trait method / helper、非 intrinsic builtin が同じ `CallableSignature` と call checker を使う。
2. operator lowering 後の call が helper / Trait call と同じ signature、solver、failure reason を使う。
3. callable 名、builtin 名、data type 表示名による推論分岐がない。
4. ReturnTypeArgument、value arguments、expected return、Trait arguments、obligation subject が一つの constraint set で解かれる。
5. Trait contract / impl / invocation が role 付き型リストと一つの substitution で照合される。
6. 同じ TypeCtorTraitFamily の occurrence は canonical carrier を共有し、capability view は位置ごとに保持される。
7. Ariadne は annotation / value、expected / value、contract / impl、branch / branch、left / right の任意の二 origin について、
   双方の `SourceFact` と span に完全な型を表示する。call-site ReturnTypeArgument label だけは型名のみでもよい。
8. `if` / `match` / `cond` は専用 reason / kind / message と context layout を維持し、内部の type assertion、substitution、
   typed facts だけを共通化する。
9. syntax、name、policy、runtime、Facet、Process / Task、SafeBind、pattern、exhaustiveness の専用 reason は、本書の境界と根拠を満たす。
10. Facet、Process / Task、SafeBind の通常 arity / callable / type relation は共通 route を使う。SafeBindはResult RHSだけを
    canonical identityで一段分解し、non-Result RHSを変更せず通常pattern検査へ渡す。Option固有拒否と`Ok` pattern限定がない。
11. standard type 固有 remediation は canonical identity と可視 semantic data から導き、型名文字列 heuristic を使わない。
12. `message`、label、note、help、hint を解析して reason、origin、expected、got、data を生成する経路がない。
13. Ariadne と JSON が同じ structured reason / origin / closed `DiagnosticData` variant を参照する。
14. JSON は既存 field を維持し、`reason`、`origin`、`data`、`related` を additive に出力する。
15. phase 固有 error 型を維持し、user-facing policy と compiler invariant を区別する。
16. ReturnTypeArgument と value parameter の正規語彙だけを使い、旧 field、alias、compatibility layer を残さない。
    第 7.4 節の repository 全体 `rg` が本書以外で出力なしになる。
17. Forge に unresolved carrier、pending Trait dispatch、未具体化 callable を渡さない。
18. `do` intrinsic の詳細を本書へ取り込まず、第 13 節の診断境界だけを適用する。
19. 移行対象 family の文字列 heuristic と型名 / callable 名分岐が削除される。
20. non-intrinsic builtinの追加・変更起点は`BUILTIN_METAS`だけであり、各runtime entryのsurface variantsからparameter
    name / modeを含む完全な`CallableSignature`を構築できる。複数variantはruntime `BuiltinId`を共有できるがcanonical
    surface callable identityは別に保つ。source declarationは完全一致検証とdoc / provenanceだけを担い、旧builtin専用call /
    obligation routeがない。
21. 第 14 節の focused test と workspace test が成功する。
