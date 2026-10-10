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

module stageのparse workerをOSが起動できない場合は、`WorkerSpawnFailure`をparse phaseの診断として返す。
対象module名とOSのエラーを保持し、対象ファイルのsource IDと先頭の空spanをprimaryにする。
構文上の誤りやsource policy違反として扱わず、通常のmodule読み込みではCLIとREPLが同じ構造化診断を表示する。
標準定義snapshotの構築中は、既存の`LoadError::BootstrapFailed`でparse phase・ファイル名・messageを返す。
このbootstrap経路はstructured reasonとspanを保持しない。
stage内の結果は入力順に扱い、起動済みworkerはjoinする。worker内部のpanicは元のpayloadを再送する。

## 型関係と呼び出しの診断（実装済み）

- Scar の `assert_type_relation` は、失敗時に部分的な型代入と capability / obligation の変更を rollback する。成功した制約だけを後続へ渡す。両側の source fact は照合対象の型とともに保持する。
- 呼び出しの arity / mode / named 引数、通常の型関係、Trait obligation / dispatch、constructor family / payload / capability は共通の reason を使う。演算子と対応する helper は同じ検査を通し、文脈の違いを `DiagnosticOrigin` に保持する。入れ子の呼び出し・annotation の失敗を外側の演算子へ付け替えない。
- carrier 未確定の constructor 呼び出しでは、予備検査と候補検査を rollback しても、型注釈を解決する producer が返したエラーをそのまま保持する。未知型や型引数 arity など候補に依存しない失敗は、パラメータ・body・入れ子の注釈の元 span、reason、source fact を返す。正しく解決した注釈との型関係が候補ごとに成立しない場合は `CandidateSelection` の候補情報を保持する。message や structured 情報の有無から失敗を分類せず、最初の候補の失敗を最終診断に採用しない。
- `cond` の節と `if_let` の発生文脈は Spire / Sigil から Scar まで保持する。分岐診断には全 body の型・span・ordinal と guard の source fact を含める。`cond` の実行は型検査後に既存の `TypedInner::If` へ lowering する。
- JSON の `data` は source location の rebase 後に typed projection から生成する。必須 key は省略せず、該当しない値は `null` にする。`related` は primary fact も含み、型は `type`、source role は `left_value` / `right_value` などの snake_case とする。
- constructor `family_id` は同じ族の canonical Trait ID をソートして構成する。familyはcapability継承を表し、別direct parameterのcarrier同一性を暗黙に作らない。完全な source value の型には captured 引数と `Result` の error 型も含める。登録順や内部 inference ID を表示しない。
- structured input がある場合、optional field の欠落を理由に message / label / source の解析へ戻らない。SafeBind、pattern / Extractor / exhaustiveness、policy、runtime、parser、resolver、REPL command/query の各 family は producer-owned reason/data から表示する。
- OR Pattern は `match` arm、`if_let`、`if_let_then` と binding-free な `is_match` で root / nested ともに許可する。Sigil は alternative 間の束縛名・順序不一致を Pattern resolve error、Scar は同名束縛の解決済み型不一致を `PatternTypeMismatch` で拒否する。`is_match` の全 alternative で binding を禁止する既存診断は維持する。Spire は `=` / `=?`、do `<-` / `=?`、`apply_pattern` の root / nested OR を `PatternSyntax` で拒否し、禁止された `|` token を primary span にする。consumer input / RHS にある通常 `match` の OR はこの禁止対象ではない。

- `apply_pattern` の projection は選択済み Pattern 引数だけで検査する。index の範囲・重複・欠番・許可位置と注釈型の不一致は静的エラーとし、runtime `Err` に変換しない。通常 Pattern 不一致には既存の literal / list / constructor 診断を使い、Extractor の `Err` は kind・message・cause・source を保持する。consumer 自体は外側 callable / do へ早期 return しない。

## Lazy・PatternキャプチャとErrorKind

正規化と各フェーズの責務は[Lazy special formの実装契約](Lazy_spec.md)に従う。
Pattern 位置の直接プレースホルダ、binding を作る Pattern の成功 branch に対する DirectExpression 要求、eager 式から成功 binding を参照した `UndefinedVariable` は、それぞれの構文・scope・型契約に従って診断する。
Lazy の eager 式からキャプチャの生成引数を参照した場合は、Sigil の `Capture` reason で拒否し、プレースホルダの参照 span を示す。直接置換の `&N`・`(&N)` は対象外とする。通常の名前解決失敗はその診断を維持し、型不一致や thunk 化による救済へ進めない。
Pattern を通常 Expr として解析し直すことや、照合前の eager 式を成功 scope へ戻す救済は行わない。

Lazy の正規化は最大一段の wrap に限定する。正規化後の型不一致と、同じ capture placeholder の要求型競合は通常の型関係で拒否する。
両 branch が未知の場合は期待される関数型から確定し、REPL 行の完了時に未確定 signature が残れば callable の具体化契約で拒否する。
外側の注釈によって既知 branch の評価方法を変更しない。

Lazyキャプチャの型不一致では、通常のreasonを維持し、解決済み標準関数の由来が確定している場合に関数別の説明・修正案を追加する。
対象は`and`、`or`、`if`、`if_then`、`if_let`、`if_let_then`、`require`、`ensure`、`Result::map_err`、`Result::cause`。
案内は生成されたsignatureとplaceholderの対応に従い、引数の並べ替えも反映する。関数名や関数型の形、エラー文面から由来を推測しない。
同じplaceholderに通常値とLazyの要求が競合する場合は番号を分ける案内とし、両branchが未知の場合は具体的な期待関数型を与える案内とする。
Pattern bindingを伴う成功branchにはDirectExpressionの契約を適用し、通常値をthunkで包む修正案を出さない。
入れ子callの失敗を外側のLazyキャプチャへ付け替えず、由来が確定していない通常の関数値には通常の型診断を使う。
error placeholder を持つ capture は、正規化後の `(-> Error)` を通常の callable として受け渡せる。Error の運搬を理由とする拒否診断は設けず、通常の引数型の不一致を報告する。
裸の標準Lazy special formのcaptureはSigilで拒否し、引数を記述したcaptureへ案内する。Lazy markerを保持したbuiltin参照を通常の関数値として後段へ渡さない。

Sigilはcanonical calleeの種類、直接placeholderの引数位置・span、通常Expr内での使用を生成parameterへ記録する。
入れ子の使用は最も近いcallの引数として分類し、Pattern consumerは宣言から選択済みの引数roleだけを使う。
未確定のPattern roleから成功branchの由来を推測しない。診断用metadataの収集によって通常のresolveエラーやその優先順位を変更しない。
Scarは生成signatureと由来をbindingのUIDに対応させ、alias・`&f`・groupingを通じて保持する。
REPLの保存・復元と候補検査のrollbackにはこの状態も含め、失敗した候補やcaptureの一時状態を後続の診断へ漏らさない。
placeholderの競合では、その正規化で確定した要求型をまとめて使う。未確定型を含むsignatureを完成済みとして表示しない。
追加の案内はhuman表示と構造化remediationへ同じ内容を渡し、通常のreason・primary spanを保持する。

`ErrorKind` には具体的な `deferror` の解決済み型 identity だけを渡す。
未定義型名、非エラー型、抽象 `Error`、runtime Error 値、constructor call、文字列、直接 placeholder を静的に拒否する。
標準引数以外の marker 使用も拒否し、旧 Lazy marker や任意文字列へ fallback しない。
表示名の比較で ErrorKind の許可を判定せず、Sigil が確定した canonical identity を使う。

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
| constraint / Trait | `InvalidTraitConstraintSubject`, `MissingGenericBound`, `MissingTraitCapability`, `NoApplicableTraitImplementation`, `CyclicTraitObligation`, `UnresolvedTraitMethodInstantiation`, `MissingTraitDispatchTarget` |
| TypeCtorTrait | `MissingTypeConstructorConstraint`, `TypeConstructorFamilyMismatch`, `TypePayloadMismatch`, `MissingTypeConstructorCapability` |
| Trait method contract | `TraitMethodTypeListMismatch`, `TraitMethodTypeListArityMismatch`, `TraitMethodConstraintMismatch` |
| branch | `IfBranchTypeMismatch`, `MatchArmTypeMismatch`, `CondBranchTypeMismatch` |
| SafeBind input | `SafeBindTotalPatternNonMonadRhs`, `SafeBindTotalPatternNonResultMonadRhs` |
| pattern / Extractor | `PatternTypeMismatch`, `PatternShapeMismatch`, `PatternArityMismatch`, `NonTotalBindingPattern`, `NestedResultErrorPattern`, `MatchGuardTypeMismatch`, `ConstructorPatternRequiresEnumOrResultRhs`, `ExtractorInputTypeMismatch`, `ExtractorArityMismatch`, `NonExhaustiveMatch` |
| policy | `SafeBindErrorTypeMismatch`, `SafeBindRequiresMonadFailTarget`, Facet / Process / source / compile policy reason、`NominalDeclarationConstraintViolation`, `TraitImplementationForbidden`, `TraitHelperCaptureNeedsExpectedType`, `ReservedIntrinsicMarkerUsage` |
| producer contract | `TypecheckInvariantViolation` |

`MissingGenericBound`はrigid genericの宣言済みproof不足、`MissingTraitCapability`は具象subjectの能力不足、
`MissingTypeConstructorCapability`はconstructor carrier occurrenceの能力不足であり、相互に置換しない。
未確定inference variableのobligationは`Deferred`として保持し、候補数や登録順からreasonや型を決めない。
`CyclicTraitObligation`は、同じTraitとcanonical subjectの証明を再訪した循環を表す。
Traitとsubjectを構造化して保持し、constructor投影でも能力不足やmetadata破損へ置き換えない。
必須の能力検証では要求元のcall / annotation位置をprimaryにし、元のreasonと原因を返す。
候補headが一致しないimplや、要求されていないTraitの能力列挙で見つけた失敗は、
無関係なcallの循環診断へ昇格させない。
`TraitImplementationForbidden`は対象型の実装権限に反する宣言に使い、`Policy` dataに対象型、Trait、
制限理由を保持する。impl対象とTrait名をそれぞれ source fact に結び、単なる能力不足と区別する。
生成された `Eq` / `Show` の実装本体または `Eq` の呼出しで能力が不足するときは、元のTrait失敗reasonを維持し、
`TraitDispatch.dependency` に発生文脈、root、field / variant payload / container の依存段階、
末端型とその実装権限を保持する。derive では失敗した field / payload の型注釈を primary、
derive 宣言を related にする。呼出しでは元の呼出し位置を primary にする。
失敗した field / payload は元の型注釈 span と解決済み型から特定し、表示用の型名で照合しない。
通常型の内部はその型の明示 impl / derive の境界とし、外側の型から無条件に展開しない。
内側の derive が失敗した場合は、その derive を root とする経路を報告する。
container の要素へ進むのは、対象に一致する impl の実際の条件が同じ Trait をその要素へ要求する場合だけとする。
異なる Trait の条件や複数候補で原因を特定できない場合は外側の型で止める。循環は型の同一性で検出し、深さで打ち切らない。
Human の依存経路 note と JSON の `dependency` を同じ typed data から生成する。

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
| `SafeBindRelation` | `lhs_type`, `rhs_type`, `lhs_is_total`, `rhs_is_canonical_result`, `monad_capability` |
| `Pattern` / `Policy` / `Runtime` / `Parse` / `Resolve` / `Repl` | family固有の閉じた入力。`detail`は表示・追跡用であり、reason再分類には使わない |

SafeBind固有reasonは、通常pattern型検査を通過したtotal pattern + non-Result RHSにだけ生成する。
canonical Monad proofが成立すれば`SafeBindTotalPatternNonResultMonadRhs`、closed concrete typeで
不成立なら`SafeBindTotalPatternNonMonadRhs`とする。Deferred、rigid genericのbound不足、solverの
既存structured failureをこの二reasonへ畳み込まない。どちらのheadlineもcanonical Resultだけが
外側一段の自動分解対象であることを本文に含め、変換APIのhelpは生成しない。

Pattern failure の処理先は通常 callable では `MonadFail`、do では `MonadFail > Alternative` の順で解決する。
MonadFail は元の Error を通常の `fail(error)` 呼出しへ渡す。Alternative は `empty()` に置き換える。
Monad 単独は失敗を構築しない。total `<-` は Monad だけを要求し、guard は常に Alternative の通常呼出しとする。
返り型が未確定の通常 closure は外側の能力を借りず拒否する。do の未確定 carrier は Deferred obligation として
保持し、候補数や登録順で選ばない。generic の能力は宣言した bound に限る。

必要 metadata の欠落や不正、曖昧な dispatch を Alternative への切替えで隠さない。
SafeBind / partial `<-` は元の operator、pattern、RHS、return / do result span と Error の
kind、message、location、cause を保持する。失敗処理先の Trait 呼出しで Error を再生成しない。

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

doのcarrier衝突は明示RTA、expected result、RHS等の元source factsを保持する。
partial `<-`の能力不足はpattern、SafeBindの能力不足は`=?`をprimaryにする。
末尾SafeBindのreturn不一致は`TypedDoSafeBind.origins.result_span`を使い、synthetic control nodeのspanへ依存しない。
生成matchのexhaustivenessではなく、user patternやbranch自体の診断を保持する。
human / JSONは同じreason、message、primary span、related source factsを投影し、
do専用の未採用JSON fieldを一般診断schemaへ混入させない。詳細は[do intrinsic](./Do_intrinsic_spec.md)を参照する。

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
- `Ok` / `Err` / `True` / `False` は通常 Enum の解決済み variant を診断 subject にする。再定義・shadowing は共有予約名規則で拒否する。`Err` の引数は Error 型として検査し、既存 Error の再格納も受理する。
- constructor capture の禁止 policy は resolver の `ConstructorCaptureForbidden` reason とし、通常の `Capture` reason や表示 message の文字列判定へ fallback しない。constructor identity と source span は診断 producer が保持する。
- bare または `_` を含む通常 enum constructor の型引数が Scar の finalization まで未確定なら `UnresolvedEnumConstructorTypeArgument` とする。`DiagnosticOrigin::EnumConstructor` と constructor span、未確定 ordinal、`Insufficient` constraint status を保持する。`Err` の失敗値が保持する未確定の成功 slot は多相性として許可する。constructor capture の callable signature は既存の具体化規則で検査する。
- bare capability の未使用、fresh result witness の未確定、full obligation / pending dispatch の未解決は typecheck phase にする。position rule は `notes`、constraint の削除または必要な式の利用は `help` に置く。
- Trait-head / nominal declaration binder の inline constraint 違反では、`where $P: Bound` への help を一意に提示し、binder内constraintや direct TypeCtorTrait binder という別の書換え候補を併記しない。通常型注釈の`_`は`Hole`、RTA内の`_`は推論変数として別々に診断する。

## 出力契約

CLI の実行時オプション、stack trace、REPL の表示レベルは [display_error.md](display_error.md) に従う。

compiler 内部の source span は Unicode scalar value（Rust `char`）単位の半開区間を使う。
UTF-8 byte offset や LSP の UTF-16 code unit ではない。human diagnostic と JSON の
`line` / `column` / `span`、`.eldr` source map、runtime error location もこの単位を引き継ぐ。
byte range や UTF-16 position を要求する外部 API へは、その protocol 境界でだけ変換する。
source ID を符号化する現行の span 範囲は、各 source の Unicode scalar value 数が
`MODULE_SPAN_STRIDE`（1,000,000）未満であることを要求する。CLI は script と include module を含め、
上限以上の source をコンパイル前の `LoadError` として拒否する。

### Human-readable

renderer は `message`、`labels`、`notes`、`help` をそれぞれ headline、source caption、note、help として出力する。Ariadne の色、罫線、空白、label の順序は安定契約にしない。

言語レベルの Error の主キャプションは、[Error spec](Error_spec.md) の生成位置を使う。
明示的な `deferror` の構築は構築式、構文 Pattern の不一致は実際に失敗した子 Pattern を指す。
list の長さや空入力など構造自体の不一致は、失敗した構造 Pattern 全体を指す。
入れ子の失敗を親 Pattern、alias、SafeBind の RHS、外側の呼出し位置へ置き換えない。
関数、named Extractor、ExtractorClosure による区別は設けない。

`Err` / `MatchResult::Err` への格納、SafeBind、および MonadFail context の partial `<-` は、
元 Error の kind / message / location / cause を保持する。新しい Error で wrap した場合は、
新しい Error の構築位置を主キャプションとし、元 Error の位置は cause に保持する。
呼出し経路は stack trace で追跡し、stack trace の先頭で生成位置を上書きしない。
位置の由来を Error 名、表示文、carrier 名から推測するフォールバックは設けない。
REPL も同じ生成位置の契約を使う。入力単位のソースを保持し、後続入力で呼び出した関数・Extractor 内の
Error は生成元の入力内の行・列を指す。`eprint` はその位置をそのまま表示する。

停止要求済み・回収済みの process への新規要求を拒否する `ProcessStopped` は、その要求の呼出し位置を生成元にする。Stop handler、生成 wrapper の内部 builtin、deferror 宣言の位置へ置き換えない。direct / capture / 高階関数 / Workers / lease の各経路も同じ source origin 契約を使う。kind は canonical な Error 宣言の identity から得て、message の表示文字列から推測しない。timeout や後発 reply との競合で確定済み Error の位置を上書きしない。

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

### Result / MatchResult の型表示

値の型名、期待型、実際の型、関連する source fact は `Result<T>` / `MatchResult<T>` を使い、内部の Error 型を第二型引数として表示しない。変数、引数、field、関数型、入れ子の型注釈でも型定義どおりの型引数数を受理する。`Result<T, NoneError>` の例外は設けない。

第二引数の補助表記は、関数定義の直接の戻り値と named `defextractor` 定義の直接の戻り値に限る。REPL コマンドによる定義の照会では、定義に記載された Error 位置を保持する。省略された Error を補わない。補完候補とシグネチャヘルプには値の型と同じ正規表記を使う。

### MatchResult の Extractor 境界

named `defextractor` の戻り型は `MatchResult<P>` を正規表記とし、`MatchResult<P, Error>` も受理する。ExtractorClosure の型注釈は `MatchResult<P>` のみを受理する。
旧 Option / 通常 Result、第二型引数の非 abstract Error、一般の値位置、通常 Closure の
返却・構築を静的拒否する。`MatchResult::Err` の引数は Error 型で検査し、
既存の Error も具象 constructor の生成値と同じ規則で受理する。
Unit payload の子 Pattern は0または1であり、arity / annotation 不一致は型エラーとする。
入力の末尾を照合対象、それ以前を事前引数として検査する。signature から引数領域を
確定し、総 arity の不足・余剰を拒否してから、事前引数の型と payload の子 Pattern を
検査する。同じ Pattern で新しく束縛する名前は事前引数・pin・head の解決に使わず、
Pattern 開始時の外側 scope に名前がない場合は名前解決エラーとする。
照合対象の head が裸の型変数、未確定 constructor、Hole の場合は
`Extractor target type must have a concrete head` として宣言位置で拒否する。Extractor の
`where` は parser で通常関数を使う案内とともに拒否する。
SafeBind は元 Error の source facts を保持し、consumer や Extractor 名から message を作り直さない。
未知 tag / 不正 field 数 / discriminant / Err payload は内部契約違反として停止し、
Result の利用者エラー、通常不一致、Alternative empty に変換しない。

ExtractorClosure は専用 signature 型を持ち、通常 call と通常 Closure との暗黙変換を拒否する。
Closure / capture / ExtractorClosure を変数へ束縛するときに非 rigid の未確定型が残れば
`Callable binding requires a concrete signature` とし、型注釈または expected type のある
高階関数への直接引数を案内する。後続 call-site ごとの暗黙 generalize へ fallback しない。
local head は選ばれた lexical identity の型を検査し、named Extractor へ探し直さない。
引数の Expr / Pattern 候補は signature で選択し、未選択候補の診断を発行しない。
選択された候補の Parse / Resolve 診断は元の phase、reason、span、cursor、関連ラベルを保持する。

## Result を返す算術演算子の補助ラベル

`/`・`%` の結果と外側の型要求が不一致になる場合、既存の型不一致 reason、primary span、主ラベル、期待型・実際型を維持する。同じ ariadne 診断に、原因となった演算子トークン位置の補助ラベルを追加する。外側の型表示は正規表記の `Result<T>` を使う。

補助ラベルには解決済みの `Div::safe_div` / `Mod::safe_mod` 実装の具体的なシグネチャを表示する。例えば Int の除算は `` `/`: (Int, Int) -> Result<Int, ZeroDivisionError> ``、Float の除算は `` `/`: (Float, Float) -> Result<Float, ZeroDivisionError> ``、Int の剰余は `` `%`: (Int, Int) -> Result<Int, ZeroDivisionError> `` となる。ユーザー実装にも同じ規則を適用し、定義の独自エラー契約を保持する。省略されたエラー位置に `Error` を補わない。

型検査側が演算子の由来、ソース位置、解決済みシグネチャを構造化データとして渡し、表示側が補助ラベルを生成する。表示側でエラー文、ソース文字列、Result 型の形から由来を推測しない。未確定シグネチャを具体化済みとして表示するフォールバックは設けない。

外側の演算、関数引数、型注釈、戻り値などで、型不一致に関係する演算結果だけを表示する。同じ式にある無関係な演算子は列挙しない。通常の型検査順序とエラー優先順位を変えず、fmap、do、unwrap などへの誘導、修正例、Help は追加しない。

## Result を返す位置の文末 `?`

文末 `?` 自体の型は Unit とする。Result を返す関数やクロージャの末尾に置いた場合は、
必要な Result と Unit の不一致を通常の型診断で報告する。
構文段階の末尾禁止や専用のエラー経路は設けず、暗黙の `Ok(())` も挿入しない。
途中の Unit が受理される位置では、最も近い callable / do の失敗処理能力の制約に従って使用できる。

## Boolean 関数名の suffix

関数名末尾の `?` は Spire が関数名位置だけで受理する。変数・束縛・フィールド・Extractor 名などの不正な位置は構文エラーとし、名前解決や型検査の結果を使った再解析は行わない。suffix 名の解決失敗は通常の名前解決診断を維持し、suffix を取り除いて再検索しない。

Scar は解決・正規化した署名の返り型が canonical Boolean でない宣言を拒否し、宣言の span に Boolean 制約と実際の返り型を示す。未確定型・コンテナ型・関数型は拒否する。型 alias の既存規則は変更しない。`predicate?(value)?` の最後の `?` に対する拒否は、既存の文末アンラップ診断を維持する。廃止済み OptionalSelector も構文位置で拒否する。

## Error Payload の診断

宣言・入力・Payload・内部構築・局所具象型・Facet の成功／拒否条件は [Error spec](Error_spec.md) を正本とする。拒否診断では、可能なら宣言側と指定側の位置を併記し、生成ノードの位置だけを表示しない。runtime Error の主キャプションには、同仕様の生成位置を使う。診断用の source facts を message や Error 名から再構成しない。
