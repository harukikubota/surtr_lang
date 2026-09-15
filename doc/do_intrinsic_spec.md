# `do` intrinsic 構文・型検査仕様

## 1. 状態と正本関係

本書は、ReturnTypeArgument と TypeCtorTrait の修正フェーズ完了後に追加する
`do` intrinsic の surface syntax、型推論、lowering、診断、テストを定める実装入力である。

用語、TypeCtorTraitFamily、carrier、Trait obligation の正本は
[`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)、診断の
`message` / `labels` / `notes` / `help` 分類は
[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md) とする。ReturnTypeArgument の構文・省略、
Trait method のrole付き型リスト、Deferred、dispatch、callable instantiationも同じ開発者向け正本に従う。
TypeCtorTraitのcall-site ReturnTypeArgumentにおける完全・部分型applicationと`_`も同じ規則に従い、
`do`固有のcarrier表記として別実装しない。
通常callableで同じdirect TypeCtorTrait名を共有し、異なるdirect Trait名を独立させる規則は
開発者向け正本の実装済み契約に従う。
`do`固有の同一性はfamily所属から導出せず、本書のcompiler-owned contractが所有する
一つのdo-local carrier入力へ明示的に結び付ける。

本書と正本が衝突する場合は正本を優先し、本書を先に修正してから実装する。ReturnTypeArgument を使わない
carrier 指定、自然言語 message の再解析を追加してはならない。具象データ型固有の failure route は原則として
追加しないが、canonical `Result` または検証済み Result effect carrierで既存 SafeBind の failure を保存する
[8節](#8-safebindとの統合) の限定規則だけは例外とする。

本書のSafeBindは[`diagnostics_cleanup_spec.md`](diagnostics_cleanup_spec.md) §§3–4で実装済みの契約を再利用する。
N01–N07は完了し、compiler-owned contract、`DoBlock`、surface validationまで実装済みである。
N08のdo構文・AST・resolver・scopeと、N09のcarrier推論・core loweringは実装済みである。
N10のSafeBind / Forge loweringと、N11の診断・全carrier統合検証まで実装済みである。N11は末尾SafeBindのreturn診断に保存済みresult spanを使い、
標準carrier・Transformer・ユーザ定義carrierをpipelineと実行比較し、Alternative不足とFacet source scopeの境界を検証する。
Extractorは現行の`Option<T>`返却を前提とし、更改タスクへの依存はない。
以下の`Result<R, E>`は内部型関係の説明表記であり、doの変数注釈やRTAに二引数Result構文を追加しない。
現行のerror値はabstract `Error`へ収束するため、独立したcaptured error parameterを新設しない。

## 2. 実装順序と開始条件

`do` の実装は、既存の ReturnTypeArgument / Trait dispatch / diagnostics の修正より後に行う。
次の条件をすべて満たすまでは、lexer、parser、AST、Sigil、Scar、Forge、標準ソースへ
`do` 実装を追加してはならない。

1. すべての callable が定義側と call-site の `::<...>` を ReturnTypeArgument として保持する。
2. 通常callableでは同じdirect TypeCtorTrait名を単一のconstructor variableとして共有し、異なるdirect
   Trait名は同じfamilyでも独立carrierとして扱う。同じ名前付きconstructor variable、`Self`、
   ReturnTypeArgumentが宣言した関係も共有する。
3. user function、Trait helper、非 intrinsic builtin が同じ role 付き型リスト、constraint set、
   obligation solver を使う。
4. 未確定の call-site 入力と Trait obligation が `Deferred` のまま保持され、boundary で
   `AmbiguousReturnTypeArgument` などの構造化 failure になる。
5. impl の個数、登録順、具象データ型名から未確定 carrier を逆決定する経路がない。
6. carrier identity が constructor head、arity、全 mapped slot、captured / fixed arguments を保持する。
7. source diagnostic、Ariadne、JSON が同じ構造化 failure と source origin を参照する。
8. Forge 前の監査が pending dispatch、未確定 carrier、未具体化 callable を拒否する。
9. SafeBindのResult一段分解、partial non-Result pass-through、total non-Result二診断、通常pattern TypeError優先がN06で固定される。

この条件は[`type_constructor_monad_do_implementation_plan.md`](type_constructor_monad_do_implementation_plan.md)の
N06ゲートで一度だけ判定する。互換fallbackを作らず、修正フェーズと`do`追加フェーズを分離し、前者の完了を
focused testとworkspace testで確認してから後者へ進む。

## 3. 目的と基本契約

`do` は、compiler-owned contractが所有する一つのdo-local carrier入力上で、`Monad::bind` による
値取り出しと逐次実行を記述する intrinsic である。標準の `Option`、`List`、`Result`、`Either` に
限定せず、同じ Trait 契約を満たす user-defined carrier にも適用する。

一つの `do` block は次を満たす。

- `Monad` capability を常に要求する。
- block 内の全 monadic origin と block 結果は、同じdo-local carrier入力へ結び付く。
- mapped payload は文ごとに通常どおり変化できる。
- captured / fixed arguments を含む carrier identity は厳密に一致する。
- partial pattern の `<-` は SafeBind/failureMatcher と共通の ResultContext を使う。Result effect があれば Error を保持し、
  なければ同じ carrier の `Alternative::empty`、どちらもなければ capability error とする。
- SafeBind `=?` は do 内でも受理する。ResultContext は `ResultEffect > Alternative > Monad` の順で解決し、canonical `Result` または検証済み `@result_effect` carrier では既存 SafeBind failure を保持する。Result effect がない carrierでは同じ carrier の `Alternative::empty` へ failure を上書きし、Monad 単独は failure target としない。
- `guard`、`pure`、`return` は通常 call として検査し、`do` checker は名前で分岐しない。
- Result effect のない failureMatcher/partial `<-`/SafeBind failure は resolved `Alternative` dispatch から構築し、
  Result effect がある場合は既存 Error を保持する。do checker が具象データ型固有の failure 値を新規生成しない。

通常関数の `where` だけでは「`Monad` は常時、`Alternative` はResult effectのないfailureMatcher/SafeBindを含むdo carrierの場合だけ」
という条件付き capability を表現できない。この条件付き obligation とSafeBind failure targetの選択が`do`を
intrinsicにする理由である。

## 4. compiler-owned contract と標準ソース宣言

`do` の型検査正本は、Sindr の compiler-owned intrinsic metadata に置く。

```text
DoIntrinsicContract {
  identity: IntrinsicId::Do,
  owner: Bootstrap,
  return_type_arguments: [DirectTypeCtorTrait(Monad)],
  value_parameters: [DoBlock<$Result>],
  return_type: Monad<$Result>,
  safe_bind_input: [
    CanonicalType(Result) => UnwrapOneLayer,
    Otherwise + PartialPattern => PassThroughToPattern,
    Otherwise + TotalPattern => RejectAfterPatternTypeCheck(
      NonMonad | NonResultMonad,
    ),
  ],
  failure_effect: FailureEffectContract {
    result_effect_action: PreserveResultEffectFailure,
    fallback_capability: Alternative,
    fallback_action: OverrideWith(Alternative::empty),
  },
  routes: [
    Always => Monad + SameCarrier(ReturnTypeArgument(0))
      + Sequence(Monad::bind),
    HasPartialExtractPattern => Monad + SameCarrier(ReturnTypeArgument(0))
      + FailureEffect(failure_effect),
    HasLegalSafeBind => Monad + SameCarrier(ReturnTypeArgument(0))
      + FailureEffect(failure_effect),
  ],
}
```

最終的な Rust 型名は既存 Sindr metadata の構造へ合わせてよい。必要な不変条件は、ReturnTypeArgument position 0、
`DoBlock<$Result>` parameter、`Monad<$Result>` return、常時`Monad`、Result effectのないfailureMatcher/SafeBindを含むdo carrierでだけ必要な
`Alternative`、Result effect metadata、両能力とReturnTypeArgument position 0のsame-carrier関係、共通 FailureEffect policy、lowering先method identityが
文字列ではなくcanonical identityとcanonical type structureで保持されることである。`Result` の判定も表示名ではなく
canonical builtin type identityで行い、Result effect は検証済み `@result_effect` metadata と直接 base の identityから解決する。Scar はこの validated contract を instantiate し、display-only
`IntrinsicDecl` や raw signature 文字列を callable scheme として扱わない。

`safe_bind_input`はRHSのcanonical Result identityとLHS totalityを分類し、non-Resultの具体型を列挙しない。
non-ResultはRHS値と型をそのまま通常pattern checkerへ渡してstatic pattern検査を先に行う。
partial patternだけがその後もpass-throughとして残り、total patternはpattern型関係が成立した場合に
canonical Monad capability proofを使うN06の二reasonで拒否する。

`DoBlock` の canonical builtin type identity も Sindr の builtin type metadata を起点に登録し、
`IntrinsicSignatureOnly(IntrinsicId::Do)` の利用区分を持たせる。標準ソースの宣言を canonical identity の
作成根拠にしてはならない。

標準ソースでは、`DoBlock` marker を `lib/types/special_types.srt` の top level に置く。

```surtr
@doc """
Compiler-reserved `do` block marker.
Use level: intrinsic-signature-only marker.
`DoBlock<$Result>` is not an ordinary first-class value type.
"""
@builtin type DoBlock<$Result>
```

`do` の display / docs surface は、既存の `match` / `cond` と同じく `lib/bootstrap.srt` の
`@autoimport defmod Bootstrap` 内に置く。canonical owner は `Bootstrap`、intrinsic identity は
`IntrinsicId::Do` とする。

```surtr
@doc """
Sequence values in one Monad carrier.
The carrier may be supplied with `do::<Carrier>`, inferred from the block,
or obtained from the expected block type.
Inside `do`, SafeBind unwraps one outer Result layer only. Partial patterns may
inspect other RHS values unchanged; total patterns reject non-Result RHS values.
ResultContext is selected as ResultEffect > Alternative > Monad: canonical Result
and validated Result-effect carriers preserve failureMatcher/SafeBind failures,
while carriers without Result effect use Alternative::empty when available and
otherwise report a capability error.
"""
@intrinsic def do::<Monad>(block: DoBlock<$Result>) -> Monad<$Result>
```

`DoBlock<$Result>` は first-class value ではなく、`do` が消費する構文blockを docs / signature help で
表す compiler-reserved marker である。do intrinsic surface signature 以外の user declaration、通常parameter、
return、field、binding、impl target、inherent impl では受理しない。

`@intrinsic` 宣言は構文の所有者と表示契約を示すが、通常 callable の body や runtime functionを作らない。
Spire は surface 宣言を signature grammar で構造化した validation input と raw display text に分けて保持し、
Sigil は canonical identity を解決して `DoIntrinsicContract` と構造比較する。owner、ReturnTypeArgument arity / role、
parameter、return、repeated `$Result` relation が違う場合は、標準ソースの declaration span で
`InvalidIntrinsicSurfaceContract` として拒否する。Scar の call-site 推論は検証済み Sindr contractだけを使い、
raw signature 文字列を解析しない。

条件付き`Alternative`、SafeBind failure policy、lowering contractはsource callable signatureだけでは表現しない
compiler-owned部分である。
Scarは`do`名やpattern種別ごとの散在したhardcodeからTrait名を選ばず、`routes`のpredicateをpattern totality、
SafeBindの有無、確定済みcarrier identityへ評価する。一つのrouteからcapability obligation、same-carrier制約、
canonical lowering method identityを一体として具体化する。

宣言側の `::<Monad>` と call-site の `::<Carrier>` は ReturnTypeArgument である。do 内部で使う block-local
carrier inference variable は `DoIntrinsicContract.return_type_arguments[0]` を instantiate した結果であり、
別の型入力チャネルではない。

標準の `guard` は intrinsic にせず、次の通常関数シグネチャを持つ。

```surtr
def guard::<Alternative>(condition: Boolean) -> Alternative<Unit>
```

`guard` の body が使う `pure` / `empty` と、利用側の carrier 推論は通常 call と Trait dispatch の責任である。

## 5. Surface syntax

### 5.1 grammar

```text
DoExpression :=
  "do" DoCarrierReturnTypeArgument? "{" DoItems "}"

DoCarrierReturnTypeArgument :=
  "::" "<" (BareTypeConstructorHead | AppliedCarrierType | "_") ">"

DoItems :=
  DoStatement* FinalExpression

DoStatement :=
  MatchBlockPattern "<-" Expression
  | MatchBlockPattern "=?" Expression
  | OrdinaryDoStatement
  | Expression

OrdinaryDoStatement :=
  OrdinaryStatementOtherThanSafeBind
```

`do` は予約語とし、callable 名や変数名として shadow できない。compiler-ownedな
`@intrinsic def do...` の宣言名だけは、canonical stdlib surfaceを構造検証するために予約tokenを受理する。
block は空にできず、最後に block 全体の
monadic result を返す式を一つ持つ。最後が `<-`、`=?`、`=`、または明示的に `Unit` へ捨てる文なら、
最終 monadic result がないため拒否する。

`pattern =? rhs` は最終式ではなく、成功時にpattern bindingを後続文へ導入するdo statementとして受理する。
parserは`=?`をordinary statementより先に`SafeBind`へ分類する。
SafeBindを無加工のordinary statementとしてsynthetic bind closureへ移してはならない。Scarはdo carrier確定後に
[8節](#8-safebindとの統合)のfailure targetを選び、do式の継続を返す専用のnormalized statementへlowerする。

`<-` は global binary operator ではない。`do` block では値取り出し文、`Facet::bulk_update` block では
既存の path update として enclosing syntax で区別する。それ以外の位置では受理しない。

### 5.2 carrier ReturnTypeArgument

正規surfaceは次の三形である。

```surtr
do::<Option> { ... } # 明示
do::<_> { ... }      # carrier位置だけ推論
do { ... }           # ReturnTypeArgument全体を省略

result: Option<Int> = do { ... } # block全体の型注釈から推論
```

`do` の宣言側ReturnTypeArgumentは一項なので、`do { ... }` は `do::<_> { ... }` と同じ制約を生成する。
明示listの項目数は厳密に一つとし、`do::<Option, Int>` は `ReturnTypeArgumentArityMismatch` にする。

`do<Container> { ... }` は受理しない。`<...>` は Trait-head type parameter や通常の型applicationに見えるため、
parser はその範囲をlabelし、`do::<Container> { ... }` へ書き換える help を出す。

call-site ReturnTypeArgument は次を受理する。

- `Option`、`Either` のような具象 TypeConstructor head
- `Either<String, Int>` のような完全なcarrier型application
- `Either<String, _>` のような`_`を含む部分carrier型application
- `_`

head だけの `do::<Either>` は constructor head だけを固定する。mapped slot と captured / fixed arguments は
RHS、通常call、block expected type、型注釈、最終式から取得する。`Either` impl が一つしかなくても、
未確定の captured argument を impl 一覧から埋めてはならない。

`do::<$F>` のような外側constructor variableだけの明示は受理しない。applied carrierの型applicationは
通常のTypeCtorTrait RTA入力として扱い、captured / fixed argumentを含むcarrier identityを他のconstraintと照合する。
Spireは外側constructor variableの項目spanをlabelし、`InvalidDoCarrierReturnTypeArgument` と次のhelpを出す。

```text
message: `do` return type argument must be a constructor head, applied carrier, or `_`
label: this is an outer constructor variable, not a valid carrier type input
note: captured and fixed arguments in an applied carrier are checked against do block constraints
help: write `do::<Either> { ... }`, `do::<Either<String, _>> { ... }`, or add an expected result type
```

外側constructor variableを使うgeneric contextでは、`do { ... }` または `do::<_> { ... }` と書き、
RHS、expected type、型注釈、最終式、通常callのconstraintからそのrigid variableへ統一する。明示ReturnTypeArgumentは
新しい型変数やcapabilityを宣言しない。

### 5.3 文の分類

| source form | 意味 | do carrierへの参加 |
|---|---|---|
| `pattern <- rhs` | `rhs: F<A>`をbindし、payloadへMatchBlock patternを適用する | RHSが参加 |
| `pattern =? rhs` | RHSを一度評価し、Resultなら成功payload、partial pattern + non-Resultなら値全体へpatternを適用する。total + non-ResultはN06診断で拒否する | RHSは参加せず、合法なSafeBindのfailure targetだけがdo carrierを使う |
| non-final `expr: F<A>` | payloadを捨てて次の文へ進むbare monadic expression | 式結果が参加 |
| final `expr: F<R>` | block全体の結果 | 式結果が参加 |
| `pattern = rhs` | 既存のtotal binding | do carrierへは直接参加しない |
| non-final plain expression | 通常blockと同じ逐次評価。結果を捨てる | monadic型でなければ参加しない |

通常callが carrier origin になるのは、そのcall結果が `<-` RHS、bare monadic expression、最終式のいずれかを
占めるか、block expected typeがそのcallへ伝播する場合である。通常bindingでmonadic value自体を変数へ保存する
だけなら、doのsequencingとはみなさない。

bare expression の結果が TypeCtorTrait carrier か未確定な場合、Scar は早い段階でplain expressionへ固定せず、
通常callの pending ReturnTypeArgument とともに `Deferred` にする。boundaryまで文分類に必要な型が決まらなければ、
原因となった通常callの `AmbiguousReturnTypeArgument` など既存の構造化failureを報告する。

SafeBind RHSはResult一段分解／partial non-Result pass-through／total non-Result拒否規則で独立に型検査し、
do carrierの推論元にはしない。合法なSafeBindの存在は、ResultContext（Result effect、次に`Alternative`、最後に
`Monad`）を選ぶためのconstraintになる。Monad単独は failure target ではない。
carrier未確定時にResultまたはAlternative実装一覧から逆決定せず、他のoriginによるcarrier確定までpolicy選択を
`Deferred`にする。

## 6. Carrier inference

### 6.1 一つのcarrier変数

Scar は `do` ごとに、宣言側ReturnTypeArgument position 0をinstantiateしたcarrier変数を一つ作る。
この一意性は通常callableの同じdirect Trait名を共有する規則やfamily所属だけから導かず、
`DoIntrinsicContract.return_type_arguments[0]`が所有するsame-carrier関係である。canonical `Monad`のfamilyは
capabilityとslot mappingを解決するために使い、別々のdo-local carrierを生成する根拠にはしない。

次を検査順に依存しないconstraint sourceとして収集する。

1. `do::<...>` のcall-site ReturnTypeArgument
2. block全体へ与えられたexpected typeまたはblock結果の型注釈
3. 各 `<-` RHS
4. 各bare monadic expression
5. monadic originを占める通常callのcall-site ReturnTypeArgumentと推論結果
6. blockの最終式

`guard`、`pure`、`return` も5または6の通常callとして参加する。名前ではなく各 callable signature と
ReturnTypeArgument、value arguments、expected returnから制約を得る。

### 6.2 carrier identity

carrier identityは少なくとも次を構造的に比較する。

```text
CanonicalDoCarrier {
  family_id: TypeCtorTraitFamilyId,
  constructor_head,
  arity,
  mapped_slots,
  captured_arguments,
}
```

mapped payload はcarrier identityから分離する。同じmapped slotのpayloadであっても、文ごとに別の通常型変数を
使える。captured / fixed argumentsは再帰的な型構造までcarrier identityに含め、一致しなければならない。

```surtr
do::<Either> {
  number <- left_source  # Either<String, Int>
  flag <- right_source   # Either<String, Boolean>
  return((number, flag))
}
```

上は同じcarrier `Either<String, _>` なので受理する。次はcaptured argumentが異なるため拒否する。

```surtr
do::<Either> {
  number <- left_source  # Either<String, Int>
  flag <- right_source   # Either<Int, Boolean>
  return((number, flag))
}
```

複数mapped slotを持つTypeCtorTraitでも、全`slot_id`とpositionをmetadataから照合する。先頭または末尾の一位置を
暗黙にmapped slotとして選んではならない。

### 6.3 payload型

各 `<-` RHS を `F<A_i>`、各bare monadic expressionを `F<B_j>`、最終式を `F<R>` とする。
`A_i`、`B_j`、`R` は独立した通常型入力であり、同じsource型変数や式制約で接続された場合だけ一致を要求する。
`pattern <- rhs` のpatternは `A_i` に対して既存MatchBlock pattern checkerで検査する。

`F<F<A>>` のRHSから一回の `<-` が取り出すのは `F<A>` である。`do` は一回のbindを再帰flattenに変えない。

### 6.4 Deferredとboundary

carrier head、captured argument、mapped payload、通常callのReturnTypeArgument、Trait dispatchのいずれかが
未確定なら、その待機inference variableとoriginを保持して `Deferred` にする。一つがbindされた後も残りの
variablesへre-homeし、失敗した候補probeはcarrier bindingとpending stateをともにrollbackする。

definition boundary、callable instantiation boundary、program boundaryでは全Deferredを監査する。
入力が尽きてもdoのReturnTypeArgumentを一意に決められない場合は `AmbiguousReturnTypeArgument`、
concrete carrierに必要Traitがなければcapability error、二つのoriginが矛盾すればcarrier mismatchにする。
最初のimpl、唯一のimpl、builtin既定型をfallbackにしてはならない。

## 7. Capabilityとdispatch

### 7.1 Monad

`do` はblockが最終式だけの場合も、carrierに`Monad`を常に要求する。各 `<-` とbare monadic expressionの
sequencingは、canonical `Monad::bind` contractのrole付き型リストを通常のTrait callと同じsolverで具体化する。

各signature位置のcapability viewを超えてmethodを利用してはならない。外側のrigid constructor variableが
`Applicative`しか宣言していない場合、具象化候補が偶然`Monad` implを持っていても`do`でbindできず、
`MissingGenericBound`とする。direct callを理由にproof environmentへboundを追加してはならない。

### 7.2 guard、pure、return

`guard(condition)` は `def guard::<Alternative>(...)` の通常callである。`guard::<Option>(condition)` の
明示ReturnTypeArgument、周囲のexpected carrier、他のRHSのどれからでも通常のcall-site推論ができる。
do checkerは`guard`という名前を見て`Alternative`を追加しない。

`pure(value)` と `return(value)` も通常のTrait helper callである。do checkerは名前、module、builtin ID、
body形状からcarrierを選ばず、そのsignatureが生成したconstraintとdoのexpected carrierを統一する。

### 7.3 partial patternとFailureEffect

`<-` のpattern totalityは既存のtotal binding判定を再利用する。variable、annotation、wildcard、totalな
as-pattern、要素がすべてtotalなtupleはtotalである。literal、pin、list/string分解、constructor、Extractor、
or-patternなどno-matchし得るpatternはpartialである。

total patternは`Monad`だけを要求する。partial patternはSafeBind/failureMatcherと同じ共通 FailureEffect routeを使い、
`ResultEffect > Alternative > Monad` の順で解決する。Result effectがあれば Error を保持し、なければ同じcarrierの
`Alternative::empty`、どちらもなければ capability error とする。failure resultのpayloadはblock最終結果`R`であり、
必要な dispatch は expected `F<R>` の下で具体化する。

`Monad`と`Alternative`のTrait identityが異なっても、contractは両obligationをReturnTypeArgument position 0へ
明示的に結び付けるため同じcarrierを要求する。family所属だけをこの同一性の根拠にせず、異なるfamilyとして
解決されるTrait結果もdo全体へ混ぜてはならない。

### 7.4 SafeBindの条件付き能力

N06のRHS / pattern入力検査を通過したSafeBindが一つ以上ある場合、carrier確定後に ResultContext を次の順序で
選ぶ。

1. do carrierがcanonical builtin type identityの`Result`、または直接 base が canonical `Result` に具体化された
   検証済み `@result_effect` carrierなら、既存SafeBindのResult-style failureを保持する。
   `Alternative` obligationは failure target のためには追加しない。
2. Result effect がない場合に限り、同じcarrierへ`Alternative` obligationを追加し、SafeBindのすべてのfailure exitを
   resolved `Alternative::empty` dispatchへ上書きする。
3. Result effect も `Alternative` もない場合は capability error とする。`Monad` 単独は sequencing capability であり、
   SafeBind/failureMatcher の failure target にはならない。

この順序を固定する。Result effect carrierが`Alternative` implも持っていても、SafeBind/failureMatcher/partial `<-` は
Error を保持する。表示名、surface constructor名、field、implの登録順ではなく、canonical type identityと検証済み
metadataで判定する。

non-Result carrierがrigid constructor variableで、その宣言済みcapability viewに`Alternative`がなければ
`MissingGenericBound`、concrete carrierに適用可能なimplがなければ`NoApplicableTraitImplementation`とする。
SafeBindがあることを理由に未確定carrierを`Result`へdefaultしたり、`Alternative`実装一覧から候補を選んだりしてはならない。
total non-Result RHSはこのfailure policyへ到達する前に拒否し、Alternativeの有無で合法化しない。

### 7.5 具象データ型固有routeの禁止と例外

partial patternのfailureで次を直接生成してはならない。

- `Result` 固有の`Err`またはno-match error
- `Option` 固有の`None`
- `List` 固有の空list
- `Either`その他の固有constructor

Result effectがないfailureは選択済み`Alternative` implementationのconcrete dispatch targetから得る。Result effectも
`Alternative`も持たないcarrierは、total patternなら使用でき、partial patternなら capability errorになる。この規則は
`<-`のpartial patternとSafeBindに共通して適用する。

SafeBindのcanonical `Result`分岐は新しいResult固有failureを作る経路ではない。RHSの既存`Err`値と、通常MatchBlock
pattern checkerが構築するfailureを、do式の`Result<R, E>`へ保存して返す経路である。これ以外のdata type名を条件に
例外を追加してはならない。

### 7.6 標準MonadとTransformerの接続

以下は現行compilerで実行する仕様例である。全carrierのpipeline比較は`lib/tests/do.srt`、
TransformerのAlternative不足は`tests/fixtures/script/fail/typecheck/do_state_t_*_requires_alternative.*`で固定する。
Identity、Reader、Stateは通常のMonad carrierとして扱い、型名ごとのloweringを追加しない。

```surtr
identity_result: Identity<Int> = do {
  value <- Identity::new(1)
  pure(value + 1)
}

reader_result: Reader<Config, Int> = do {
  value <- Reader::ask()
  pure(read_port(value))
}

state_result: State<Int, Int> = do {
  value <- State::get()
  State::put(value + 1)
  pure(value)
}
```

Identity、Reader、Stateは標準`Alternative`を持たないため、totalな`<-`は使えるが、partial patternの`<-`と
合法なSafeBindを含むResult effectのないdo carrierは必要capability不足として拒否する。都合のよいemptyやfailure valueをdo checkerが生成しない。

標準Transformerもrepresentationではなく、具体化された外側carrierのMonad/Alternative実装と検証済み Result effect metadata だけを使う。
base Monadの値は自動liftしない。

```surtr
source: Result<Int> = Ok(1)
lifted: OptionT<Result, Int> = MonadT::lift(source)

transformed: OptionT<Result, Int> = do {
  value <- lifted
  pure(value + 1)
}
```

`OptionT<Result, _>`のdoへ`Result<_>`を直接monadic originとして混ぜる例はcarrier不一致で拒否する。
明示的な`MonadT::lift`を要求するが、`do::<OptionT<Result, _>>`は通常のTypeCtorTrait RTAとして受理し、
applied carrier専用のdo文法は追加しない。
`StateT<S, Result, _>`のように外側carrierが`Alternative`を持たず、かつ有効な `@result_effect` を持たない構成では、
partial `<-`と合法なSafeBindを含むdoを同じcapability規則で拒否する。MonadT の内部に Result があるだけでは effect に
ならない。

## 8. SafeBindとの統合

`=?` は既存の独立intrinsicであり、RHSを一度だけ評価する。Result RHSだけは成功payloadをpatternへ渡す。
non-Result RHSは値全体を通常pattern checkerへ渡してstatic検査を先に行い、partial patternだけを受理する。
total patternは通常pattern型関係が成立した後にN06の二reasonで拒否する。合法なSafeBindはRHS failureまたは
pattern failureから早期に脱出する。`pattern =? rhs` は `pattern <- rhs` の
別表記ではない。SafeBind RHSはdo carrier `F<_>`である必要がなく、do carrierの推論元にもならない。

### 8.1 RHSの一段Result分解

LHS patternへ渡す検査対象型と値は次の規則だけで決める。

| RHS | LHS patternへ渡す値と型 | failure |
|---|---|---|
| `Result<A, E>`の`Ok(value)` | `value: A` | なし。通常pattern検査へ進む |
| `Result<A, E>`の`Err(error)` | patternへ渡さない | `error: E`を8.2節または8.3節のfailure targetへ送る |
| Result以外の`value: T` + partial pattern | `value: T`を変更せずそのまま渡す | RHS container自体からは生成しない。pattern不一致はfailureになる |
| Result以外の`value: T` + total pattern | `value: T`を通常pattern checkerへ渡しstatic検査する | 型関係が成立すればN06の非Monad / 非Result Monad reasonでcompile error |

自動分解はcanonical `Result`の外側一段だけであり、再帰的にflattenしない。Result以外のRHSをcontainerとして
分類するためのimpl検索、constructor名検査、payload抽出は行わない。LHSのconstructor patternは通常どおり解決・検査する。
特に`Alternative`を実装するdo carrierであっても、
`Option::Some(value)`を自動分解して`value`だけをLHSへ渡してはならない。

```surtr
do::<Option> {
  Option::Some(num: Int) =? Option::Some(Option::Some(1))
  return(num)
}
```

上は型エラーである。RHSはResultではないため、LHSが受け取る型は`Option<Option<Int>>`のままである。
`Option::Some` patternが一段だけ通常分解した後の`num`は`Option<Int>`であり、annotationの`Int`と一致しない。
annotationがなければ`num: Option<Int>`として推論する。`Int`を得るにはLHSにも二段を明記する。

```surtr
do::<Option> {
  Option::Some(Option::Some(num)) =? Option::Some(Option::Some(1))
  return(num)
}
```

この通常pattern検査はfailure target選択とSafeBind固有RHS分類より先に型検査する。
`num: Int =? Option::Some(10)`は通常のpattern型不一致TypeErrorであり、Result-only分解の説明を追加しない。
partial patternが型として妥当でruntimeに不一致となった場合だけ、
ResultContext-preserving doでは既存SafeBind error、Result effectのない Alternative-doでは`empty`へ進む。

do は残りの文列を `Monad::bind` の synthetic continuationへlowerするため、SafeBindを無加工でclosure内へ移すと
early-return targetが変わる。ScarはSafeBindのsuccess / failureを先にnormalized control flowとして表現し、failureを
enclosing source functionではなく、現在のdo continuationが返す`F<R>`へ接続する。

ここでreturn targetがsynthetic continuationになること自体は問題ではない。ResultContext-preserving doではcontinuationが返した`Err`を
外側の`Result` bindがそのまま伝播し、Result effectのないdoではcontinuationが`empty`を返して外側bindが後続を実行しない。
問題になるのはtargetを暗黙のままにすることであり、次の二modeをtyped IRへ明示すればよい。

### 8.2 ResultContext-preserving do

do carrierがcanonical `Result`、または検証済み `@result_effect` を持ち直接 base が canonical `Result` の carrierなら、
N06完了後のSafeBind意味論を次のとおり保持する。

- RHSが`Err(error)`なら、その`error`を変更せずdo式の`Err(error)`として返し、後続文を評価しない。
- Result payloadまたはpartial patternへ渡したnon-Result RHS全体に対するpattern不一致／Extractor failureは、N06完了後のSafeBindが選ぶ`PatternMismatch`、`EmptyList`、
  `IndexOutOfBounds`などのfailure kind / detailを保持し、do式の`Err`として返す。
- 成功時はpattern bindingを後続continuationのscopeへ導入する。
- do結果の既存Result error関係は、RHSから伝播するerrorとpatternが生成し得るerrorをすべて受理しなければならない。
- Extractorの`Some`はpayloadを渡し、`None`はno-matchからpattern failureへ変換する。Extractor自身のErr返却・独自error伝播を前提にしない。

最後の型関係は、通常関数内のSafeBindでenclosing returnのerror型と照合している既存共通relationを、
do式のResultContextのexpected error relationへ向け直したものである。衝突時はSafeBind固有の自由形式messageではなく、
同じtyped type-relation failureと二つのoriginを使う。`Result`はこの経路では`Alternative`を実装していなくてもよい。

```surtr
do::<Result> {
  value: Int =? parse_int(source) # Err(error)は同じerrorのままdo結果になる
  return(value)
}
```

### 8.3 Result effectのない Alternative-do

do carrierにResult effectがなければ、failureMatcher/SafeBindを含むdoは同じcarrierの`Alternative`を要求する。Result RHSの`Err`、
pattern不一致、Extractor failureなど、既存SafeBind contractがfailureとして分類したすべての出口を破棄し、
expected `F<R>`で具体化した`Alternative::empty()`へ上書きする。failure payloadやfailure kindをcarrierへ変換する
暗黙関数は導入しない。

成功時はResultContext-preserving doと同じくbindingを後続continuationへ導入する。したがって利用者から見た規則は「このdo carrierで
SafeBindが失敗すれば、そのcarrierのemptyになる」であり、実装から見た規則は「一つのfailure handlerを
resolved empty dispatchへ接続する」である。

```surtr
do::<Option> {
  value: Int =? parse_int(source) # Err(_)は捨ててOption::Noneにする
  [head, ..tail] =? values        # pattern failureもOption::Noneにする
  return((value, head, tail))
}
```

### 8.4 選択と非目標

policy選択はdo carrierが確定するまで`Deferred`にする。SafeBind RHSの型、可視なimpl数、expected error型から
do carrierを逆推論しない。ResultContext は Result effect を先に選び、なければ `Alternative`、それもなければ
capability error とする。`Monad` 単独を failure target にしない。

ResultContext は現在の do-local carrier だけから決定し、外側の do や enclosing function の
Result effect を借用・継承しない。

```surtr
do::<OptionT<Result, _>> {
  do::<List> {
    # このfailure policyはListの能力だけで解決する
    ...
  }
}
```

do外のSafeBindは従来どおりenclosing functionのcanonical Resultまたは検証済み Result-effect carrierへ failureを
接続する。本変更はSafeBindを一般Monad failureへ変更せず、do内だけで failure target を明示的に差し替える。

```text
ResultContext-preserving do:
  SafeBindFailure(error) => Result::Err(error)

Result effectのない Alternative-do:
  SafeBindFailure(_) => Alternative::empty()
```

## 9. Conceptual lowering

残りの文列を返す継続を`next`、do全体の結果を`F<R>`とする。型検査後の概念loweringは次である。

```text
total pattern <- source
  => Monad::bind(source, {|value| { pattern = value; next() }})

partial pattern <- source
  => Monad::bind(source, {|value|
       match value {
         pattern => next(),
         _ => Alternative::empty(),
       }
     })

bare source: F<A>
  => Monad::bind(source, {|_| next()})

pattern =? source  # ResultContext-preserving do
  => SafeBindControl(
       source,
       on_success: {|bindings| next()},
       on_failure: {|failure| Result::Err(failure)},
     )

pattern =? source  # non-Result Alternative-do
  => SafeBindControl(
       source,
       on_success: {|bindings| next()},
       on_failure: {|_| Alternative::empty()},
     )

ordinary statement
  => { statement; next() }

final expression
  => expression
```

この表示は意味を示すものであり、未解決のsurface名を再注入する手順ではない。Scarはcanonical Trait identity、
role付き型リスト、同じcarrier substitutionを使って`Monad::bind`と`Alternative::empty`を具体化し、concrete dispatchを
持つtyped call、closure、match、blockへlowerする。

SafeBind statementは少なくとも次のnormalized情報を保持する。

```text
NormalizedDoSafeBind {
  pattern,
  rhs,
  rhs_projection:
    UnwrapOneResultLayer { canonical_result_identity, error_type }
    | PassThroughNonResultPartial,
  continuation_result_type: F<R>,
  failure_target:
    PreserveResult { canonical_result_identity, expected_error_type }
    | AlternativeEmpty { dispatch: TraitDispatchTarget },
  origins: { do_span, operator_span, pattern_span, rhs_span, result_origin },
}
```

最終的なRust enum名は既存typed IRへ合わせてよい。`SafeBindControl` / `NormalizedDoSafeBind`はdo専用runtime opcodeを
意味せず、既存`TypedInner::SafeBind`へfailure targetを追加しても、Scarで明示branch graphへ展開してもよい。
必要な不変条件は、Forge到達時にmode、result型、Result identityまたはempty dispatchが具体化済みで、元source originを
保持することである。
total non-Resultは通常pattern検査後にcompile errorとなり、`NormalizedDoSafeBind`やForgeへ渡さない。

`<-`とSafeBindのRHSは一度だけ評価する。`pattern <- source` のpatternは、合成closureが受け取った一時値へ適用する。
partial `<-`のExtractorも既存MatchBlock contractに従い、no-matchは共通 FailureEffect route（Result effectならError保持、
なければ Alternative::empty）へ進む。
SafeBindではOption-returning Extractorの`None`をpattern failureへ変換し、8節で選択したfailure targetへ進む。
Extractorの不正な返却型は静的診断であり、no-matchやemptyとして処理しない。
型検査後に未知tag等の内部契約違反を検出した場合も、通常のno-matchに丸めない。

Forgeへは`do`固有の未解決carrierやcandidateを渡さない。typed loweringを既存IRで表せるため、do専用opcode、
runtime trait dictionary、runtime candidate selectionを追加しない。

## 10. AST、scope、phase ownership

### 10.1 Spire

- `do` token、`do` expression、block固有`<-`を構文化する。
- `do::<...>` は既存call-site ReturnTypeArgument parser routeのtoken / span処理を再利用し、項目を
  具象TypeConstructor head、完全・部分適用carrier、または`_`一項に制限する。
- ASTは`Ast::Do`に`call_site_return_type_arguments`と`AstDoStatement`列を持つ。
- `AstDoStatement`は少なくとも`Extract { operator_span, pattern, rhs }`、
  `SafeBind { operator_span, pattern, rhs }`、通常statementを区別する。
- parser段階でlowerせず、`<-` LHSを既存MatchBlock pattern grammarで保持する。
- `=?` LHS / RHSも既存SafeBind grammarとsource spanを保持し、parser段階でfailure targetを決めない。
- do専用Extract / SafeBindにも通常statementと同じ改行または`;` separatorを許し、末尾ではどちらも
  final expression不足として拒否する。
- `do<...>`、外側constructor variable、空block、ReturnTypeArgumentの空listなどidentity不要の違反をparse errorにする。
- `@intrinsic def do...` surfaceをraw display textと構造化validation inputに分ける。validation inputを通常callable
  schemeとして登録しない。

### 10.2 Sigil

- Sindr metadataを起点に`DoBlock` builtin type identityと`IntrinsicId::Do`を登録する。
- `lib/types/special_types.srt`の`DoBlock`宣言と`Bootstrap::do` surfaceをcompiler-owned metadataに対して構造検証する。
- 標準ソースの`do` intrinsic identity、`Monad`、`Alternative`、TypeConstructor、通常callをcanonical identityへ解決する。
- `Resolved::Do`にSigil解決済みのcanonical Monad / Alternative Trait ID、call-site ReturnTypeArguments、各origin span、resolved pattern、resolved RHSを保持する。
- `<-`と`=?`のRHSをpattern bindingより先にresolveし、patternが導入する名前を後続文だけのscopeへ入れる。
- Extractor、constructor、pin、as-patternの既存MatchBlock resolutionを再利用する。
- intrinsic identityを通常callableへ変換せず、`do`構文のownerとして保持する。
- user sourceによる`DoBlock` declarationと`DoBlock` inherent impl / impl targetを
  `ReservedIntrinsicMarkerDeclaration` / `ReservedIntrinsicMarkerImpl`で拒否する。
- N08単独では`Resolved::Do`をScarで暗黙loweringしない。N09のcarrier推論・core loweringが入るまで、
  Scar入口で明示的に未実装として拒否し、workspaceをbuildableなphase境界に保つ。

### 10.3 Scar

- validated `DoIntrinsicContract`のReturnTypeArgument position 0をinstantiateし、一つのdo-local carrier入力を作る。
- 明示ReturnTypeArgument、expected type、各monadic origin、最終式を一つのconstraint setへ集める。
- 通常callには共通`CallableSignature`、role付き型リスト、expected propagationを使う。
- pattern totalityとfailureMatcher/SafeBindの有無を判定し、`DoIntrinsicContract.routes`からMonad常時と共通 FailureEffect
  の obligation・lowering先を同時に具体化する。FailureEffectは `ResultEffect > Alternative > Monad` の順に選び、Monad単独を
  failure targetにしない。
- SafeBind RHSがResultなら外側一段のpayload、それ以外ならRHS全体をexpected scrutineeとして通常MatchBlock pattern
  checkerへ渡す。static pattern errorを先に報告し、partial non-Resultだけをpass-throughし、total non-Resultは
  canonical Monad capability proofによりN06の二reasonへ分類する。Option名による先行拒否とSafeBind constructor patternの`Ok`限定を残さない。
- effective failure targetをenclosing function returnではなくdo結果へ向ける。
- 全constraintを解いた後、resolved dispatchと明示的failure targetを持つtyped IRへlowerする。
- synthetic nodeにも元の`do`、RHS、pattern、expected typeのorigin spanを関連付ける。
- Forge前にpending ReturnTypeArgument、carrier、Trait dispatchがないことを監査する。
- do intrinsic signature以外の通常parameter、return、field、binding annotationに`DoBlock`が現れた場合は
  `ReservedIntrinsicMarkerUsage`を該当type spanに出す。

### 10.4 Forge / Eldr

- concrete `TraitDispatchTarget`と具体化済みclosure / match / blockだけを受け取る。
- carrier推論、impl fallback、具象データ型固有failureの新規生成、runtime dictionary lookupを行わない。
- ResultContext-preserving doのSafeBindにはScarが保存したResult-preserving target、Result effectのないdoには具体化済みempty dispatchを受け取る。
- SafeBindのRHS failureと各pattern failure emitterは、暗黙の`in_function`判定ではなくtyped failure targetへ分岐する。
- 既存のcall、closure、branch、pattern、return opcodeを使い、do専用opcodeを追加しない。

### 10.5 移行順序

1. Sindrに条件付きcapability / same-carrier / SafeBind policy / lowering method identityを含む`DoIntrinsicContract`と、
   reserved `DoBlock` builtin type metadataを追加する。
2. `lib/types/special_types.srt`の`DoBlock` top-level宣言と`lib/bootstrap.srt`の`Bootstrap::do` surfaceを追加する。
3. Spire / Sigilにsurface contractの構造化validationを追加し、raw signature文字列をschemeへ変換しないことを固定する。
4. `DoBlock`のuser declaration、通常type position、impl target / inherent impl拒否を追加する。
5. do parser / resolverを追加し、`<-`とSafeBindを別のdo statementとしてscope付きで保持する。
6. N06で完成した「Result一段分解／partial non-Result pass-through／total non-Result二診断 + 通常MatchBlock pattern検査優先」と
   typed failure targetを再利用する。Option名による先行拒否・Ok限定の撤去、旧fixture移行、通常SafeBindの`@doc`更新は
   N06ゲート通過前に完了していることを確認する。
7. Scarがvalidated contractをinstantiateしてconstraint収集、capability生成、SafeBind failure target選択、typed loweringを行う。
8. diagnostics / fixture / Forge boundary監査を追加し、focused test後にworkspace testを実行する。

## 11. 診断契約

### 11.1 共通failure

do固有の自然言語から原因を復元せず、既存の構造化failureを再利用する。

| failure | phase / error type | 条件 | primary span |
|---|---|---|---|
| `ReturnTypeArgumentArityMismatch` | parse / `ParseError` | `do::<...>`が一項でない | `::<...>`全体 |
| `ReturnTypeArgumentMismatch` | typecheck / `TypeError` | 明示constructor headがRHS、expected type、最終式などのcarrierと矛盾する | 明示項目または後から衝突したorigin |
| `AmbiguousReturnTypeArgument` | typecheck / `TypeError` | boundaryまでcarrier headまたはcaptured argumentが未確定 | `do`またはblocking call |
| `TypeConstructorFamilyMismatch` | typecheck / `TypeError` | 同じMonad familyで異なるconstructor headまたはcaptured / fixed argumentsを要求した | 後から衝突したorigin |
| `MissingTypeConstructorCapability` | typecheck / `TypeError` | monadic originがcanonical Monad family capabilityを提供しない | 該当origin |
| `MissingGenericBound` | typecheck / `TypeError` | rigid carrierにMonad、Result effectのないfailureMatcher/SafeBindを含むdo carrierにAlternative boundがない | do、partial pattern、またはSafeBind |
| `NoApplicableTraitImplementation` | typecheck / `TypeError` | concrete carrierに必要Trait implがない | do、RHS、partial pattern、またはSafeBind |
| `UnresolvedTraitMethodInstantiation` | typecheck / `TypeError` | bind / empty dispatchの型入力がboundaryまで未確定 | call origin |
| `InvalidDoCarrierReturnTypeArgument` | parse / `ParseError` | call-site項目が外側constructor variableである（applied carrierは合法） | 不正な項目 |
| `InvalidIntrinsicSurfaceContract` | resolve / `ResolveError` | stdlibの`Bootstrap::do` surfaceがSindr contractと一致しない | intrinsic declaration |
| `ReservedIntrinsicMarkerDeclaration` | resolve / `ResolveError` | user sourceが`DoBlock`を宣言した | declaration head |
| `ReservedIntrinsicMarkerImpl` | resolve / `ResolveError` | `DoBlock`をimpl targetまたはinherent impl receiverにした | impl target |
| `ReservedIntrinsicMarkerUsage` | typecheck / `TypeError` | `DoBlock`をdo intrinsic signature以外のtype positionで使った | `DoBlock` type span |
| `InvalidResultEffectAnnotation` | typecheck / `TypeError` | annotation assertion、canonical Trait identity、またはbase relationが成立しない | `@result_effect` annotation span |

`do`というcontext名をheadlineやnoteに追加してよいが、通常callと別の型推論規則やdata type固有kindを作らない。
明示ReturnTypeArgumentを含む衝突は`ReturnTypeArgumentMismatch`、RHS同士やRHSと最終式など推論origin同士の
same-family carrier衝突は`TypeConstructorFamilyMismatch`にする。
異なるfamilyの値、またはMonad能力を持たない値を、same-family carrier conflictへ偽装しない。rigid variableは
`MissingGenericBound`、concrete typeは`NoApplicableTraitImplementation`、capability view自体がMonad familyでない
originは`MissingTypeConstructorCapability`として区別する。

### 11.2 carrier衝突

衝突した二つのsource originへ完全な具象型をAriadne labelとして表示する。explicit
ReturnTypeArgumentのlabelは指定した型だけでよいが、相手のRHS、通常call、expected resultには完全な型を表示する。

```text
message: values require different type-constructor carriers
label 1: this RHS has `Either<String, Int>`
label 2: this RHS has `Either<Error, Boolean>`
note: one do block must use one concrete carrier in the Monad family
help: use the same fixed carrier arguments, or split the computations
```

`message`は主原因、`labels`はsource spanに対応する二つの事実、`notes`はsame-family / same-carrier規則、
`help`はReturnTypeArgument、型注釈、fixed argumentの修正、block分割に置く。規則や書換えをlabelへ入れない。

### 11.3 ambiguity

```text
message: return type argument for `do` could not be determined
label: this block leaves its Monad carrier unresolved
note: implementations are not used as default carrier choices
help: write `do::<Carrier> { ... }` or add an expected result type
```

headだけが決まりcaptured argumentが残る場合も同じfailure reasonを使い、どのcaptured positionが未確定かを
typed fieldとnoteで示す。impl候補の登録順や内部inference variable番号は表示しない。

### 11.4 capability

Result effectもAlternativeもない failureMatcher による不足は pattern/`=?` spanをprimaryにし、必要ならdo carrierを
related labelにする。
compiler-generated wildcardや`Alternative::empty`のsynthetic spanをuser-facing primaryにしない。

```text
message: this do pattern requires `Alternative`
label: this pattern can fail to match
note: partial `<-` adds a failure branch to the same Monad carrier
help: use a total pattern, or use a carrier that implements `Alternative`
```

具象carrierが何であるかによりheadlineを分岐しない。

Result effectのないdo carrierの合法なSafeBindによる不足は、同じtyped Trait obligation failureを次のcontextでrenderする。

```text
message: this do SafeBind requires `Alternative`
label: this SafeBind can exit before the remaining do statements
note: a do block without Result effect replaces SafeBind failure with `Alternative::empty` in the same carrier
help: use a carrier that implements `Alternative`, or use a ResultContext-preserving do block to preserve SafeBind errors
```

### 11.5 SafeBind

RHSのcanonical型がResultかどうかとLHS totalityは、LHSへ渡す検査対象とSafeBind固有診断を選ぶpolicy dataとして保持する。
Resultならpayload型、non-ResultならRHS型そのものを共通pattern checkerへ渡し、static pattern検査を最初に行う。

```text
message: pattern type mismatch
label 1: this binding is annotated as `Int`
label 2: this expression has type `Option<Int>`
```

これは`num: Int =? Option::Some(10)`の診断例である。`Alternative::empty`へlowerするruntime no-matchでも、
SafeBind固有reasonでもない。LHS patternと検査対象型の通常のstatic pattern type relation failureとして、
failure target選択より先に報告する。Result-only分解のnote / helpを追加しない。

通常pattern検査を通過したtotal non-Resultだけを、N06の二つのstructured reasonへ分類する。

```text
message: Int is not a SafeBind target; it is not a Monad, and only a Result RHS can be decomposed by `=?`.
```

```text
message: Option<Int> is not a SafeBind target; `=?` propagates Result-context failures, not values from another Monad. Only a Result RHS can be decomposed by `=?`.
```

前者はcanonical Monad capabilityがないRHS、後者はResult以外のMonad RHSである。型名はclosed typed dataからrenderし、
Optionという表示名でreasonを決めない。どちらにも`Option::to_result`、`from::<Result>`などの変換helpを付けない。
Monad proofが`Deferred`なら二reasonのどちらかへ早期分類しない。

ResultContext-preserving doでRHS errorまたはpattern-generated errorがdo結果の既存Result error関係に適合しない場合は、共通type relation
failureを使い、failure originとdo result originの二地点をlabelする。

```text
message: SafeBind failure does not fit this Result do block
label 1: this SafeBind failure originates here
label 2: this do block requires a compatible Result failure
note: ResultContext-preserving do preserves SafeBind errors instead of replacing them with `Alternative::empty`
help: make the Result error types agree or convert the SafeBind error explicitly
```

pattern不一致から生じるerror型が衝突する場合は、label 1をpattern spanに置き、既存SafeBind failure kindもtyped dataに
保持する。rendererがpattern textやgenerated error messageを再解析して型を復元してはならない。

### 11.6 compiler-owned contractと`DoBlock`

標準ソースのdo declarationがSindr contractと異なる場合は、Sigilが
`InvalidIntrinsicSurfaceContract`を宣言spanへ出す。

```text
message: the `do` intrinsic declaration does not match its compiler-owned contract
label: this declaration has a different owner or type structure
note: `do` must have one Monad return type argument, one DoBlock parameter, and a Monad result
help: restore the canonical `Bootstrap::do` declaration in `lib/bootstrap.srt`
```

user sourceによる`DoBlock`宣言またはimplはSigil、それ以外の通常type positionでの利用はScarが、用途に対応する
reserved marker reasonとして拒否する。

```text
message: `DoBlock` is reserved for the compiler-owned `do` signature
label: `DoBlock` cannot be used in this type position
note: `DoBlock` is not an ordinary value type
help: remove `DoBlock`; write a `do { ... }` expression to sequence Monad values
```

### 11.7 JSON

JSON利用者が必要とする情報はtyped fieldとして保持する。少なくとも次をfailureに応じて出力できるようにする。

```text
return_type_argument_ordinal
family_id
required_trait
explicit_constructor
left_type
right_type
left_origin
right_origin
captured_argument_ordinal
pattern_totality
safe_bind_mode
safe_bind_rhs_projection
safe_bind_failure_origin
safe_bind_failure_kind
```

`safe_bind_mode`は`preserve_result`または`override_with_empty`のclosed enumとし、Result判定前の未確定状態を
user-facing JSONへ出さない。`safe_bind_failure_kind`はpattern由来など既存SafeBind failureが静的に特定できる場合だけ
出力し、Alternative routeでruntime failure payloadを観測するためには使わない。
`safe_bind_rhs_projection`は`unwrap_result_once`または`pass_through_non_result_partial`のclosed enumとし、後者では
`pattern_input_type`にRHSの完全な型を保持する。

`family_id`は診断正本と同じcanonical Trait ID集合由来のstable semantic identityを使い、process-local連番や
単独family root名をserializeしない。ReturnTypeArgument、carrier、Trait obligationのdata variantと必須fieldは
[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)を再利用し、do専用の自由形式mapを作らない。

`kind`、`phase`、`primary_span`、`expected`、`got`、`hint`の既存意味を変更しない。AriadneとJSONは同じfailure
objectを参照し、rendererがmessage文字列を解析してtyped fieldを復元してはならない。

## 12. `match` / `if` 診断との関係

user source内の`match`または`if`が、それ自体のarm / branch間で一つの式型を作れない場合は、既存の
`match` / `if`固有診断を優先する。その式が一つの型には決まるがdoのcarrierと異なる場合だけ、doをcontextにした
共通carrier mismatchを出す。

partial `<-` のloweringが生成する`match`は、source pattern armとwildcard failure armを持つためexhaustiveである。
このsynthetic matchからuser-facing exhaustiveness errorを出さない。pattern自体の型不一致、Extractor contract、
pin、constructor arityなどは既存MatchBlock診断を使い、元の`<-` pattern spanを示す。

SafeBindのnormalized control flowもsuccess / failureをcompilerが閉じるため、synthetic branchのexhaustiveness診断を
出さない。SafeBind pattern自体の診断は元の`=?` LHS、Result errorとfailure targetのrelation診断は元のRHSを指す。

`guard`は通常関数なので、その実装が内部で`if`を使っていてもdo checkerは観測しない。利用側では
`guard` callのReturnTypeArgument / expected return / Trait obligationだけを扱う。

## 13. テストマトリクス

### 13.1 Spire / Sigil

- `do { ... }`、`do::<_> { ... }`、`do::<Option> { ... }`
- `do<Container> { ... }`の拒否と`do::<Container>`へのrewrite help
- ReturnTypeArgumentの空list、一項、過剰項目
- `do::<Either<String, _>>`、`do::<Either<String, Int>>`のapplied carrierが通常のRTAとして受理され、
  `do::<$F>`だけが`InvalidDoCarrierReturnTypeArgument`となり全source spanを保持すること
- 空block、最終monadic expressionなし、block外`<-`の拒否
- do blockの先頭／中間とnested doに現れる`=?`をSafeBind statementとして保持すること
- SafeBindの後続がなく最終monadic expressionを欠く場合は、SafeBind拒否ではなくdo block終端の診断になること
- `DoBlock` identityがSindr builtin metadataから登録され、stdlib surface declarationから新規作成されないこと
- `DoBlock`のuser declaration、通常parameter / return / field / binding、impl target / inherent implの拒否
- `Bootstrap::do` surfaceのowner、ReturnTypeArgument、parameter、return、repeated payload relationの構造検証
- `DoIntrinsicContract`がMonad常時、Result effectのないfailureMatcher/SafeBindを含むdo carrierでのAlternative、position 0との
  same-carrier関係、Result一段分解／partial non-Result pass-through／total non-Result二診断 input policy、Result-preserving / Alternative-empty
  failure policy、`Monad::bind` / `Alternative::empty`のcanonical lowering identityを保持すること
- display textやraw signature文字列の違いがcallable schemeまたはcall-site inferenceを作らないこと
- `Facet::bulk_update`の`<-`が回帰しないこと
- `<-`と`=?`のRHSがpattern bindingより先にresolveされること
- pattern bindingが後続文でだけ利用できること
- constructor / Extractor / as-pattern / pin identityの保持

### 13.2 carrier inference

- 明示ReturnTypeArgumentだけでcarrierを決定
- expected resultまたはblock型注釈だけでcarrierを決定
- `<-` RHSだけでcarrierを決定
- bare monadic expressionだけでcarrierを決定
- 最終式だけでcarrierを決定
- 通常callの明示ReturnTypeArgumentだけでcarrierを決定
- `do { ... }`と`do::<_> { ... }`が同じconstraintを生成
- RHSのsource順を入れ替えても成功／failure reasonが変わらないこと
- explicit、expected、RHS、通常call、最終式の任意の二originの一致／衝突
- explicit constructor headと他originの衝突が`ReturnTypeArgumentMismatch`になること
- 推論origin同士のsame-family carrier衝突が`TypeConstructorFamilyMismatch`になること
- headは明示されたがcaptured argumentが未確定のambiguity
- `do::<Either>`のcaptured / fixed argumentをRHS、expected result、block型注釈、最終式、通常callから決定
- partial / full applied carrierを明示したdo ReturnTypeArgumentの成功と、外側constructor variableだけを明示した場合の拒否
- generic contextの`do { ... }` / `do::<_> { ... }`がRHSまたはexpected typeのrigid constructor variableへ統一
- implが一つだけでも未確定carrierを逆決定しないこと
- impl登録順を反転しても結果と診断が同じこと
- SafeBind RHSだけではdo carrierをResultにもAlternative実装型にも決定しないこと
- SafeBind policyがcarrier確定までDeferredになり、policy選択後もRHS型をcarrier constraintへ混ぜないこと

### 13.3 familyとpayload

- Functor / Applicative / Monad / Alternative originがcontract所有のdo-local carrierを共有
- 複数rootを持つ同じfamilyでもcarrierを分離しないこと
- 異なるfamilyを一つのdoへ混ぜた失敗
- mapped payloadが文ごとに変化する成功
- captured / fixed argument一致の成功と不一致の二地点診断
- 複数mapped slotをslot ID / positionごとに処理
- user-defined TypeCtorTraitとuser-defined carrier

### 13.4 capabilityとpattern

- 最終式だけのdoでもMonadを要求
- total variable / annotation / wildcard / tuple / as-patternはMonadだけで成功
- literal、pin、list/string分解、constructor、Extractor、or-patternは共通 FailureEffect routeを使う
- Result effectもAlternativeも持たないcarrierでtotal patternはMonadだけで成功、partial patternはcapability error
- Result effectのないfailure branchがresolved Alternative dispatchを使い、Result effectがあればErrorを保持すること
- partial `<-`で`Result`、`Option`、`List`、`Either`固有failureをdo checkerが生成しないこと
- Result effectのないdo carrierの合法なfailureMatcher/SafeBind failureが具象型固有値ではなくresolved empty dispatchへ進むこと
- `guard`名をdo checkerが特別扱いしないこと
- `pure` / `return`名をdo checkerが特別扱いしないこと
- 各signature位置のcapability viewを超えないこと
- Identity / Reader / Stateのtotal bindが通常のMonad dispatchで成功すること
- Identity / Reader / Stateのpartial `<-`と、Result effectのない合法なfailureMatcher/SafeBindを含むdoが、標準`Alternative`不在により拒否されること
- OptionT等のTransformerで明示`lift`後の値を扱え、base Monad値の直接混在はcarrier mismatchになること
- StateT等で外側`Alternative`も有効な Result effect もない構成のpartial `<-` / 合法なSafeBindを拒否すること

### 13.5 SafeBind / match / if

- Result RHSだけを外側一段自動分解し、nested Resultを再帰分解しないこと
- `Ok(x) =? Ok(Err(error))`は内側errorの伝播ではなくpattern no-matchになり、ResultContext-preserving doではPatternMismatch、Result effectのないAlternative-doではemptyとなること
- `Err(e) =? Ok(Err(error))`は成功し、二文のSafeBindで明示二段伝播できること
- non-Result RHSはOption、List、String、user-defined型を含めて値／型全体を通常MatchBlock pattern検査へ渡し、partial patternだけをpass-throughすること
- pattern型関係が成立するtotal non-Resultを、非Monad / Result以外のMonadの二reasonで拒否すること
- `num: Int =? Option::Some(10)`を通常のpattern TypeErrorにし、SafeBind固有reasonやResult-only説明を付けないこと
- `value =? Option::Some(10)`をResult以外のMonad、`value =? 10`を非MonadのSafeBind reasonにすること
- `Option::Some(num) =? Option::Some(Option::Some(1))`で`num: Option<Int>`になること
- `Option::Some(num: Int)`なら静的pattern type mismatchになり、empty routeへ進まないこと
- `Option::Some(Option::Some(num))`なら`num: Int`になり、runtime no-matchだけがempty routeへ進むこと
- Option名による先行RHS rejection、total non-Result pass-through、SafeBind constructor patternの`Ok`限定が残っていないこと
- ResultContext-preserving doでSafeBind RHSの`Err(error)`が同じerrorを保持してdo結果から返り、後続を実行しないこと
- ResultContext-preserving doでliteral、list/string分解、Extractorなどのpattern failure kind / detailが既存SafeBindと一致すること
- ResultContext-preserving doのSafeBind成功時にbindingが後続文だけで利用できること
- ResultContext-preserving doがAlternative implなしでSafeBindを受理すること
- ResultContext-preserving doのRHS error / pattern-generated errorを現行のResult error relationで検査し、独立したcaptured error型を新設しないこと
- Result effectのないAlternative-doで合法なSafeBindのRHS failure、pattern不一致、Extractor failureがすべて同じcarrierのemptyになること
- Result effectのないAlternative-doで合法なSafeBindのfailure payloadを観測せず、後続を実行しないこと
- user-defined Monad + Alternative carrierでも同じempty routeを使うこと
- non-ResultでAlternativeを持たないrigid / concrete carrierが`MissingGenericBound` / `NoApplicableTraitImplementation`になること
- Result effect policyがAlternative routeより先に選ばれ、impl追加で意味が変わらないこと
- SafeBindだけからcarrierをResultへdefaultせず、ambiguityを保持すること
- do外のSafeBindが既存のenclosing-function failure targetと診断を保つこと
- user `match` / `if`の内部branch mismatchが固有診断を保つこと
- user `match` / `if`全体とdo carrierの不一致は共通carrier mismatchになること
- partial `<-`のsynthetic matchがexhaustiveness errorを出さないこと
- MatchBlock / Extractor errorが元pattern spanを指すこと

### 13.6 lowering / runtime

- `<-` RHSを一度だけ評価
- bare monadic expressionがpayloadを捨てて後続を実行
- mapped payloadが変化する連続bind
- Alternative failure時に後続を評価しないこと
- SafeBind RHSを一度だけ評価すること
- Result-preserving SafeBindとAlternative-empty SafeBindがsynthetic closureのreturn targetへ依存しないこと
- typed SafeBind failure targetがResult effect identity / expected error型またはconcrete empty dispatchとsource originsを保持すること
- `Option`、`List`、`Either`、user-defined carrierの成功経路
- concrete Trait dispatchだけがForgeへ渡ること
- do専用opcodeとruntime candidate lookupがないこと

### 13.7 diagnostics

- explicit ReturnTypeArgumentとRHSの二地点label
- 二つのRHSまたはRHSとexpected typeの二地点label
- ambiguityのReturnTypeArgument help
- partial patternのpattern labelとResult effect / Alternative capability note
- Result effectのないdo carrierのSafeBindに対する`=?` labelとAlternative note
- total non-Result RHS二reasonのResult-only説明と、変換helpがないこと
- 通常pattern TypeErrorにSafeBind固有説明が混入しないこと
- ResultContext-preserving SafeBind error mismatchのRHS / patternとdo resultの二地点label
- `safe_bind_mode`、`safe_bind_rhs_projection`、pattern input型、failure origin、既知failure kindのJSON field
- AriadneとJSONが同じtyped failureを参照
- 内部variable ID、impl登録順、synthetic wildcard spanを表示しないこと

## 14. 対象外

- TypeCtorTraitの型形状指定やconstructor slot mapping規則の変更
- Trait impl coherence、applicability、parent coverageの量化変更
- arbitrary higher-kinded type variableと一般kind system
- runtime Trait object、dictionary passing、dynamic dispatch
- impl specialization、priority dispatch、negative bound
- SafeBindを一般Monad failureへ変更すること
- 明示 annotation と canonical base の検証を経ない、特定データ型名・field・representation向けの新しいdo failure規則を追加すること
- do専用opcodeまたはruntime carrier selection
- Boolean expressionを暗黙に`guard` callへ変換すること

## 15. 受け入れ基準

1. 標準ソースの`do`は`@intrinsic`宣言と利用者向け`@doc`を持つ。
2. canonical surfaceは`do::<Container> { ... }`であり、`do<Container>`を拒否する。
3. Sindrの`DoIntrinsicContract`がReturnTypeArgument position 0、`DoBlock` parameter、Monad result、常時Monad、
   Result effectのないfailureMatcher/SafeBindを含むdo carrierでのAlternative、position 0とのsame-carrier関係、Result一段分解／
   partial non-Result pass-through／total non-Result二診断 input policy、SafeBind failure policy、`Monad::bind` / `Alternative::empty` lowering identityを
   canonical identityと構造で保持し、stdlib surfaceをそのcontractに対して検証する。
4. `do { ... }`と`do::<_> { ... }`が同じconstraintを生成し、明示constructor headはposition 0を固定する。
5. call-siteの明示項目は具象constructor head、完全・部分型application、または`_`を受理し、外側constructor variableだけを拒否する。
   applied carrierのcaptured / fixed argumentsと他constraintは厳密一致させ、明示headまたは型applicationとの衝突時は`ReturnTypeArgumentMismatch`にする。
6. 明示head、expected type、型注釈、RHS、通常call、最終式からcarrierを推論し、captured / fixed argumentsは
   明示head以外のblock constraintだけから得る。
7. 未確定carrierをimpl一覧から逆決定せず、Deferredをboundaryのambiguityまで保持する。
8. 一つのdoはcontractが所有する一つのdo-local carrier入力と一つの具象carrierだけを使う。
9. mapped payloadは文ごとに変化でき、captured / fixed argumentsは厳密一致する。
10. Monadは常時、AlternativeはResult effectのないfailureMatcher/SafeBindを含むdo carrierにだけintrinsic規則として追加される。ResultContextは `ResultEffect > Alternative > Monad` とする。partial `<-`もfailureMatcherとしてこの順序で解決する。
11. `guard`、`pure`、`return`は通常callであり、名前固有のchecker分岐がない。
12. failureMatcher/partial `<-` failureは共通 FailureEffect routeを使い、Result effectがあれば Error を保持し、なければ
    resolved Alternative dispatch、どちらもなければ capability error とする。具象データ型固有routeはない。
13. do block内のSafeBindを受理し、canonical Resultまたは検証済み Result effect carrierでは既存Err / pattern failureを保持し、
    Result effectのないcarrierでは同じcarrierの resolved `Alternative::empty`へ上書きする。SafeBind RHS自体はcarrier推論元にしない。
14. SafeBindはResult RHSだけを一段自動分解し、non-Result RHSは型と値を変更せず通常MatchBlock pattern検査へ渡す。
    static pattern errorを優先し、partial patternだけをpass-throughし、total patternは非Monad / Result以外のMonadで拒否する。
    AlternativeはRHS分解能力を追加せず、Option名による先行拒否とconstructor patternの`Ok`限定を持たない。
15. `DoBlock`はSindr metadata起点のreserved markerで、canonical do signature以外の宣言、type position、implを拒否する。
16. Scarはraw intrinsic signatureではなくvalidated contractをinstantiateし、concrete dispatchと明示的SafeBind failure targetを
    持つtyped IRへlowerする。
17. same Monad family内のcarrier衝突は`TypeConstructorFamilyMismatch`、別familyまたは能力不足は
   `MissingTypeConstructorCapability` / `MissingGenericBound` / `NoApplicableTraitImplementation`として区別する。
18. carrier衝突は関係する二地点をlabelし、JSONは同じfailureのtyped fieldを保持する。
19. user sourceの`match` / `if`とsynthetic matchの診断ownershipが分離される。
20. 標準型とuser-defined carrierのunit / fixture / runtime testが同じ汎用routeを固定する。
21. ReturnTypeArgument、Trait dispatch、diagnosticsの修正フェーズ完了前にdo実装を開始しない。
