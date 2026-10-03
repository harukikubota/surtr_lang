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
対象は`and`、`or`、`if`、`if_then`、`if_let`、`if_let_then`、`assert`、`ensure`、`Result::map_err`、`Result::cause`。
案内は生成されたsignatureとplaceholderの対応に従い、引数の並べ替えも反映する。関数名や関数型の形、エラー文面から由来を推測しない。
同じplaceholderに通常値とLazyの要求が競合する場合は番号を分ける案内とし、両branchが未知の場合は具体的な期待関数型を与える案内とする。
Pattern bindingを伴う成功branchにはDirectExpressionの契約を適用し、通常値をthunkで包む修正案を出さない。
入れ子callの失敗を外側のLazyキャプチャへ付け替えず、由来が確定していない通常の関数値には通常の型診断を使う。
error placeholderを持つ`assert`、`ensure`、`Result::map_err`、`Result::cause`のcaptureにも既存のError受け渡し制約を適用する。
通常callの拒否reasonは維持し、error式をcapture内に固定する修正案を示す。`(-> Error)`を通常引数として渡す修正で既存制約を迂回しない。
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
| constraint / Trait | `InvalidTraitConstraintSubject`, `MissingGenericBound`, `MissingTraitCapability`, `NoApplicableTraitImplementation`, `UnresolvedTraitMethodInstantiation`, `MissingTraitDispatchTarget` |
| TypeCtorTrait | `MissingTypeConstructorConstraint`, `TypeConstructorFamilyMismatch`, `TypePayloadMismatch`, `MissingTypeConstructorCapability` |
| Trait method contract | `TraitMethodTypeListMismatch`, `TraitMethodTypeListArityMismatch`, `TraitMethodConstraintMismatch` |
| branch | `IfBranchTypeMismatch`, `MatchArmTypeMismatch`, `CondBranchTypeMismatch` |
| SafeBind input | `SafeBindTotalPatternNonMonadRhs`, `SafeBindTotalPatternNonResultMonadRhs` |
| pattern / Extractor | `PatternTypeMismatch`, `PatternShapeMismatch`, `PatternArityMismatch`, `NonTotalBindingPattern`, `NestedResultErrorPattern`, `MatchGuardTypeMismatch`, `ConstructorPatternRequiresEnumOrResultRhs`, `ExtractorInputTypeMismatch`, `ExtractorArityMismatch`, `NonExhaustiveMatch` |
| policy | `SafeBindErrorTypeMismatch`, `SafeBindRequiresResultTarget`, `InvalidResultEffectAnnotation`, `ErrorValueMustBeWrapped`, Facet / Process / source / compile policy reason、`NominalDeclarationConstraintViolation`, `TraitImplementationForbidden`, `TraitHelperCaptureNeedsExpectedType`, `ReservedIntrinsicMarkerUsage` |
| producer contract | `TypecheckInvariantViolation` |

`MissingGenericBound`はrigid genericの宣言済みproof不足、`MissingTraitCapability`は具象subjectの能力不足、
`MissingTypeConstructorCapability`はconstructor carrier occurrenceの能力不足であり、相互に置換しない。
未確定inference variableのobligationは`Deferred`として保持し、候補数や登録順からreasonや型を決めない。
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
- constructor capture の禁止 policy は resolver の `ConstructorCaptureForbidden` reason とし、通常の `Capture` reason や表示 message の文字列判定へ fallback しない。constructor identity と source span は診断 producer が保持する。
- bare または `_` を含む通常 enum constructor の型引数が Scar の finalization まで未確定なら `UnresolvedEnumConstructorTypeArgument` とする。`DiagnosticOrigin::EnumConstructor` と constructor span、未確定 ordinal、`Insufficient` constraint status を保持する。builtin-special `Result` constructor の専用診断・推論経路はこの reason の対象外とする。
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

言語レベルの Error の主キャプションは、その Error を生成したソース位置を使う。
明示的な `deferror` の構築は構築式、構文 Pattern の不一致は実際に失敗した子 Pattern を指す。
list の長さや空入力など構造自体の不一致は、失敗した構造 Pattern 全体を指す。
入れ子の失敗を親 Pattern、alias、SafeBind の RHS、外側の呼出し位置へ置き換えない。
関数、named Extractor、ExtractorClosure による区別は設けない。

`Err` / `MatchResult::Err` への格納、SafeBind、および Result-effect context の partial `<-` は、
元 Error の kind / message / location / cause を保持する。新しい Error で wrap した場合は、
新しい Error の構築位置を主キャプションとし、元 Error の位置は cause に保持する。
呼出し経路は stack trace で追跡し、stack trace の先頭で生成位置を上書きしない。
位置の由来を Error 名、表示文、carrier 名から推測するフォールバックは設けない。
REPL も同じ生成位置の契約を使う。入力単位のソースを保持し、後続入力で呼び出した関数・Extractor 内の
Error は生成元の入力内の行・列を指す。`eprint` はその位置をそのまま表示する。

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
返却・構築、abstract Error の手書き Err 再投入を静的拒否する。
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
