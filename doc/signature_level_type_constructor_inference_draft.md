# シグネチャレベル TypeConstructor 推論・検査ドラフト

## 状態

提案。これは現行実装の説明ではなく、TypeCtorTrait を使う関数、非 intrinsic
ビルトイン、および `do` 構文の型検査を共通化するための設計案である。

この文書は `constructor_return_witness_inference_draft.md` の拡張ではない。同文書の
`if` と constructor helper に限定した期待型伝播案を採用せず、callable signature
全体を一つの制約系として検査する。

正本へ反映するときは、少なくとも次の既存記述を同時に整合させる。

- `docs/dev/Trait_system_spec.md` の position-keyed parameter witness と fresh result witness
- `doc/contextual_type_syntax_impact_analysis.md` の引数位置ごとの独立 witness
- `doc/do構文ドラフト.md` の `Result` 固有 failure route
- `docs/dev/diagnostics.md` の constructor witness 関連診断

## 目的

TypeCtorTrait を含む関数呼び出しを、データ型名や関数名に依存せず、宣言済みの
シグネチャだけから検査・推論する。

次を同じ経路へ載せる。

- 通常関数
- trait method と auto-import helper
- 非 intrinsic の builtin
- operator などが lower された後の trait call
- 引数、明示 function slot、戻り値期待型を使う call-site inference

`do` は `Monad` 必須・`Alternative` 条件付きという契約を通常の関数シグネチャで
表現できないため、唯一の構文固有例外として intrinsic にする。ただし `do` が生成する
型制約は通常 call と同じ solver へ渡し、`Option`、`List`、`Result`、`Either` などの
具象型を特別扱いしない。

## 基本原則

1. TypeConstructor の具象化は declaration 名や body の特定 AST 形状から決めない。
2. 引数、明示 slot、戻り値期待型、`do` の各 RHS はすべて対等な制約源である。
3. 制約は収集してから解き、最初に見つかった impl や最初に検査した branch を既定値にしない。
4. 同じ TypeCtorTrait root は一つの関数定義内で同じ具象 carrier を使う。
5. capability は carrier identity と分離し、シグネチャの各位置が宣言した Trait までに制限する。
6. Forge へ渡す前に carrier、trait dispatch、通常型変数をすべて静的に確定する。
7. 型推論失敗は共通の構造化診断へ集約し、型名・関数名固有のエラー分岐を作らない。

runtime trait object、dictionary passing、動的 dispatch は導入しない。

## 用語

### TypeCtorTrait root

TypeCtorTrait の継承 closure の根を、その family の root とする。`Applicative` や
`Monad` の root が `Functor` である場合、`Functor`、`Applicative`、`Monad` は同じ
family に属する。Trait 名が違うことを理由に carrier を分離しない。

root identity は表示名ではなく Sigil で解決済みの Trait identity から求める。

### carrier

TypeCtorTrait application を実装する具象型コンストラクタと、Trait の mapped slot
以外に固定される型引数を合わせた identity である。

たとえば `Either<$L, $R>` の `$R` が `Functor.$A` に map される場合、次の二つは
同じ carrier として扱える。

```surtr
Either<String, Int>
Either<String, Boolean>
```

次の二つは固定される `$L` が違うため、同じ carrier ではない。

```surtr
Either<String, Int>
Either<Error, Boolean>
```

### payload

TypeCtorTrait の mapped slot に対応する型である。payload は通常の型変数として扱い、
同じ carrier の中でもシグネチャの関係に従って変化できる。

### capability view

ある値位置で利用できる TypeCtorTrait の能力である。同じ carrier を共有していても、
その位置に宣言された Trait を越える method は呼べない。

```surtr
def f(left: Functor, right: Monad) -> Unit {
  bind(left, mapper)  // error: left の view は Functor
  bind(right, mapper) // ok
}
```

実引数の具象型が偶然 `Monad` を実装していても、`left` の静的 capability を
`Monad` へ昇格させない。

### function slot

値引数から導入できない型入力を呼び出し側が指定する既存の dispatch slot である。
定義側で必要かどうかは通常 Trait と同じ現行規則に従う。呼び出し側の明示指定は
任意であり、期待型などから一意に推論できるなら省略できる。

## 関数シグネチャの surface 規則

### TypeCtorTrait の直接表記

ある constructor 型変数に必要な制約が TypeCtorTrait だけなら、引数、戻り値、
function slot に Trait 名を直接書ける。

```surtr
def map_value(
  value: Functor<$A>,
  mapper: ($A -> $B)
) -> Functor<$B>
```

これは hidden constructor variable と `where $F: Functor` を持つシグネチャへ
正規化する。直接表記は通常 trait object や runtime existential を生成しない。

同じ root の直接表記は同じ hidden constructor variable を参照する。ただし各位置の
capability view は表記された Trait のまま保つ。

```surtr
def combine(left: Functor, right: Monad) -> Monad
```

`left`、`right`、戻り値は同じ carrier を使う。`left` から利用できるのは `Functor`
能力だけである。

異なる root の直接表記は別々の constructor variable を導入する。

```surtr
def pair(left: Monad, right: Monad2) -> Unit

pair(Ok(1), Option::None) // Monad と Monad2 の root が異なれば許可できる
```

### 複数 capability と名前付き constructor variable

一つの値または結果に複数の Trait 能力を要求する場合は、名前付き constructor variable
を使い、すべての制約を関数定義の `where` に列挙する。

```surtr
def choose_and_bind(value: $F<$A>) -> $F<$B>
where
  $F: Monad + Alternative
{
  # ...
}
```

次のように function slot 内へ制約を書いてはならない。

```surtr
def make::<$F: Monad>() -> $F<Unit> // error
```

直接表記された異なる位置が同じ root に属し、それぞれ別の capability view を持つことは、
一つの位置へ複数 capability を与えることとは区別する。前者は直接表記のままでよい。

型入力の導入チャネルは現行規則どおり一つにする。上の `$F` は値引数から導入されるため、
同じ `$F` を function slot にも重ねて書かない。値引数から導入できない return-only の
`$F` だけが function slot を必要とする。

```surtr
def stop::<$F>() -> $F<Unit>
where
  $F: Monad + Alternative
{
  # ...
}
```

### constructor variable application

`$F<$A>` のような型変数 application は、同じ関数定義の `where` により `$F` が
TypeCtorTrait constructor であると証明される場合だけ受理する。

```surtr
def valid(value: $F<$A>) -> $F<$A>
where
  $F: Monad
{
  value
}
```

```surtr
def invalid(value: $F<$A>) -> $F<$A> { value }
// error: $F が TypeCtorTrait constructor である制約がない
```

通常型変数を任意に高階適用できるようにはしない。Trait identity の確定が必要な判定は
Scar が declaration signature を正規化する時点で行う。

### function slot の必要条件

TypeCtorTrait constructor が値引数から導入されず、戻り値からだけ要求される場合、
定義は現行の return-only 型入力規則に従って function slot を持たなければならない。

`guard` はこのケースなので slot ありで定義する。

```surtr
def guard::<Alternative>(condition: Boolean) -> Alternative<Unit> {
  if(condition, pure(()), empty())
}
```

内部的には、概念上次の契約へ正規化される。

```text
function slot: $F
return:        $F<Unit>
constraint:    $F: Alternative
```

呼び出し側は具象型の head だけを slot に指定する。

```surtr
guard::<Option>(True)
guard::<Either>(True)
```

`guard::<Either>` の固定引数は slot には書かない。期待型や他の制約から取得する。
固定引数が未確定なら、具象 head が指定済みでも ambiguity error とする。

定義に slot があっても、呼び出し側は期待型から推論できる場合に省略できる。

```surtr
value: Option<Unit> = guard(True)
```

明示 slot も期待型もなく carrier を一意に決められない呼び出しは error とする。登録順の
最初の `Alternative` impl を選んではならない。

### payload の省略

関数が payload を観測せず、他の型との関係も表現しない場合は TypeCtorTrait の型引数を
省略できる。

```surtr
def discard(value: Monad) -> Unit {
  ()
}
```

省略位置は名前付き型変数を導入しない。同じ root の別位置と carrier は共有するが、
mapped payload 同士を同一型とはみなさない。payload の同一性や変換関係が必要なら、
通常どおり `$A`、`$B` を書く。

```surtr
def keep(value: Monad<$A>) -> Monad<$A>
def map(value: Functor<$A>, mapper: ($A -> $B)) -> Functor<$B>
```

## 同一 root の統一規則

一つの関数定義を正規化したとき、同じ TypeCtorTrait root を参照する全 occurrence は
一つの constructor variable を共有する。

共有対象は次を含む。

- direct parameter type
- direct return type
- TypeCtorTrait を表す function slot
- 名前付き `$F` application
- 同じ callable scheme が body 内の generic call へ渡す proof requirement

call-site では次を統一する。

1. 具象 type constructor head
2. mapped slot 以外の固定型引数
3. nominal owner identity

mapped payload は通常型の unification 規則に従う。同じ識別子が再出現した場合だけ
同一型を要求し、別識別子または省略位置は独立してよい。

異なる root は別の constructor variable なので、異なる具象 carrier を渡せる。同じ具象
carrier を渡すことも禁止しない。

## 共通 callable scheme

通常関数、trait method、helper、非 intrinsic builtin は、Scar の call 検査前に同じ
`CallableScheme` 相当へ正規化する。

```text
CallableScheme {
  callable_identity,
  runtime_target,
  ordinary_type_variables,
  constructor_variables,
  function_slots,
  parameters,
  return_type,
  obligations,
}

ConstructorVariable {
  root_trait_id,
  required_capabilities,
  fixed_arguments,
  explicit_slot_ordinal,
  declaration_origins,
}

ConstructorOccurrence {
  constructor_variable,
  capability_view,
  mapped_payloads,
  source_span,
}
```

これは概念モデルであり、型名や field 構成をそのまま実装へ強制するものではない。
必要な不変条件は次である。

- builtin と user function が同じ argument/return inference API を使う
- runtime builtin ID や opcode 選択は `runtime_target` として型推論から分離する
- direct Trait 表記と名前付き `$F` 表記が正規化後に同じ constructor constraint を持つ
- capability view を constructor identity から分離する
- constraint origin と source span を診断まで保持する

`Ty::BuiltinFunc` のような runtime 区分が残ってもよいが、それを理由に別の型推論分岐へ
入れてはならない。

## call-site 制約収集

call は次の順序に依存せず、利用可能な制約を一度収集してから解く。

### 制約源

- 呼び出し側の明示 function slot
- 各値引数の具象型
- call expression に与えられた expected return type
- closure parameter/return の既知 shape
- callable scheme が要求する trait obligation

明示 slot は具象型 head だけを与える。値引数と期待型は完全な型を与えられるため、
固定型引数と payload の推論にも利用する。

### expected type の伝播

expected type は AST ノード名ではなく型 shape に従って再帰的に伝播する。少なくとも
次を共通経路で扱う。

- block の最終式
- `if` の全 branch
- `match` の全 arm
- tuple の各要素
- list や map の要素
- struct / record field
- enum / constructor payload
- 既知シグネチャを持つ closure body
- 通常関数、trait helper、builtin の戻り値

これは TypeCtorTrait の直接表記を nested type や local annotation に許可する変更ではない。
許可済みの具象型 shape と通常型変数の内側へ expected type を伝える規則である。

`Option::Some(pure(...))` のような generic enum payload だけを例外扱いせず、constructor
signature から内側へ expected type を渡す。`pure`、`empty`、`return` などの関数名を
見て expected type を特別に注入してはならない。

### 解決結果

solver は次のいずれかを返す。

```text
Solved
  concrete carrier と全 dispatch が確定

Deferred
  他の inference variable または expected type を待つ

Failed
  互いに矛盾する制約、能力不足、または型不一致
```

declaration boundary、specialization boundary、program boundary では `Deferred` を監査する。
呼び出しに必要な入力が尽きても constructor が未確定なら ambiguity error とし、成功へ
潰さない。

## 関数本体の検査

関数本体は宣言戻り値を expected type として、通常の bidirectional checker で検査する。
末尾が receiverless trait call、`if`、`match` のどれかによって経路を変えない。

slot-bearing TypeCtorTrait return は、関数定義時に `Option` や `List` のような具象型を
一つ選ぶ witness ではない。function slot が表す constructor variable と、その `where`
capability の下で本体を検査する。

そのため `guard` 本体の `pure(())` と `empty()` は `$F` を expected carrier として受け、
`$F: Alternative` から必要な親 capability を証明する。関数定義時や標準ライブラリの
ロード順に依存して具象 impl を選ばない。

本体が slot contract と無関係な具象 carrier に固定される場合は、generic contract を
満たさない型不一致として declaration 側で拒否する。

## 非 intrinsic builtin

非 intrinsic builtin の surface declaration も `CallableScheme` へ正規化し、通常関数と
同じ call checker を使う。

- argument inference を builtin 名で分岐しない
- expected return の注入を builtin 名で分岐しない
- TypeCtorTrait impl を builtin 固有の候補順で選ばない
- trait obligation と generic propagation を通常関数と同じ形式で保持する

runtime の builtin ID、引数の値域検査、effect、VM opcode など、型シグネチャで表せない
runtime policy は型推論後の別責任として残せる。これらを constructor 推論の根拠には
使わない。

`@intrinsic` は通常 callable ではないため、この共通化の対象外である。

本ドラフトが扱う `where` は、通常関数に置かれる通常 Trait capability constraint
だけである。trait definition／impl の `where` が持つ型形状指定や slot mapping を、
この callable normalization へ取り込まない。

## `do` intrinsic

### source declaration

`do` は標準ソースに `@intrinsic` 宣言と利用者向け `@doc` を持つ。宣言は signature help
と構文の所有者を示すが、通常の callable scheme として解釈しない。

通常の関数 `where` では次の条件付き capability を表現できないためである。

- `Monad` は常に必要
- partial `<-` など failure branch を生成する構文がある場合だけ `Alternative` が必要

この条件付き規則は `do` intrinsic の唯一の特別契約として文書化する。

### carrier inference

一つの `do` block は一つの constructor variable を持つ。次のすべてをその制約源とする。

- `do<Option>` などの明示 head
- block 全体へ与えられた expected type
- 各 `<-` RHS
- payload を捨てる bare monadic expression
- block の最終 monadic expression
- `guard`、`pure`、`return` など通常 call の推論結果

最後の項目は関数名による特例ではない。それぞれの通常シグネチャを検査した結果が、
do block の expected carrier と統一される。

すべての制約を集めてから解くため、最初の RHS と後続 RHS の順序を入れ替えても結果は
変わらない。

### 同一 carrier

値取り出し、bare monadic expression、`guard`、`pure`、`return`、最終結果は同じ具象
carrier を使う。mapped payload は文ごとに変化できる。

```surtr
do<Either> {
  number <- left_source   // Either<String, Int>
  flag <- right_source    // Either<String, Boolean>
  return((number, flag))
}
```

これは固定引数が同じなので許可できる。次は拒否する。

```surtr
do<Either> {
  number <- left_source   // Either<String, Int>
  flag <- right_source    // Either<Error, Boolean>
  return((number, flag))
}
```

`do<Either>` の slot には `Either` だけを書き、`String` は RHS または expected result から
推論する。固定引数を決める制約がなければ ambiguity error とする。

### capability

各 `<-` と bare monadic expression の sequencing は `Monad` を要求する。失敗しうる pattern
の `<-` が fallback branch を必要とするときだけ、同じ constructor variable に
`Alternative` obligation を追加する。

`guard(condition)` は通常の `guard` 関数呼び出しであり、その signature 自身が
`Alternative` を要求する。do checker が `guard` という名前を認識して capability を
追加してはならない。

`pure` と `return` も通常 call であり、do checker は名前を認識しない。周囲から渡された
expected carrier と通常シグネチャによって同じ constructor に具体化する。

### データ型固有 route の禁止

`do` intrinsic は `Result` 固有の no-match error、`Option` 固有の none、`List` 固有の
空リストを直接生成しない。failure は resolved `Alternative` dispatch から得る。

既存の `=?` は独立した intrinsic としてその契約を維持し、本ドラフトでは一般 Monad の
failure へ統合しない。`do` の carrier inference が `=?` の `Result` 固有規則を根拠に
carrier を選ぶ経路も設けない。

## 診断設計

### 原則

constructor 推論の失敗は、関数別・builtin 別・データ型別の文章をその場で組み立てず、
構造化した failure reason と origin から生成する。

同じ原因は通常関数、trait helper、builtin、`do` で同じ kind を使う。callable 名や
`do` block は context として表示してよいが、別の推論規則を意味しない。

### 共通 failure reason

最低限、次を独立した構造として扱う。

| reason | 主な原因 |
|---|---|
| `MissingTypeConstructorConstraint` | `$F<$A>` に TypeCtorTrait 制約がない |
| `InlineFunctionSlotConstraintNotAllowed` | `::<$F: Monad>` と書いた |
| `MissingFunctionTypeSlot` | 値引数から導入できない return-only constructor に slot がない |
| `InvalidDirectTypeCtorConstraint` | direct Trait 表記では表せない複数 capability を要求した |
| `AmbiguousTypeConstructor` | carrier head または固定引数を一意に決められない |
| `TypeConstructorMismatch` | 同じ root に異なる具象 carrier が与えられた |
| `MissingTypeConstructorCapability` | occurrence の capability view に必要 Trait がない |
| `ExplicitTypeConstructorMismatch` | 明示 slot と引数／期待型が矛盾した |
| `TypePayloadMismatch` | named payload の通常型制約が矛盾した |
| `UnresolvedTypeConstructorObligation` | boundary まで dispatch が deferred のまま残った |

最終的な `kind` 名は既存 `TypeError` / `DiagnosticSpec` の分類方針に合わせてよいが、
renderer が message 文字列を解析して reason を復元する設計にはしない。

### Ariadne

二つの型が衝突した場合は、それぞれの origin span に型を caption する。

```text
TypeConstructorMismatch: do block requires one monad container

left RHS:  Either<String, Int>
right RHS: Either<Error, Boolean>

note: values in one do block must use the same concrete carrier
help: use the same fixed Either parameter, or split the computations
```

function call の明示 slot は型名だけを表示すればよい。実際の矛盾は slot、値引数、
expected result のうち関係する二地点へ完全な型を表示する。

`docs/dev/diagnostics.md` に従い、配置を次で統一する。

- `message`: 短い主原因
- `labels`: 衝突した型、slot、引数、期待型など source span に対応する事実
- `notes`: 同一 root／同一 carrier、位置別 capability などの言語規則
- `help`: `::<Option>`、戻り値注釈、名前付き `$F` と `where` への書換え

### JSON

外部利用が必要な情報は自然言語から抽出せず、必要に応じて typed field を追加する。

```text
constructor_root
required_capability
explicit_constructor
left_type
right_type
left_origin
right_origin
slot_ordinal
```

内部 inference variable 番号や impl 登録順は user-facing output に含めない。

### 固有エラーメッセージの整理

既存の次の種類の分岐は共通 reason へ移行する。

- `pure`、`empty`、`return` など receiverless helper 固有の witness 未確定エラー
- `if`、`match`、constructor payload ごとの expected propagation 失敗メッセージ
- `Option`、`List`、`Result`、`Either` など型名を条件にした carrier mismatch
- builtin 名ごとの generic argument / return witness エラー
- `do` の最初の RHS と後続 RHS で別々に構築された mismatch 文言

runtime の値域違反、effect、intrinsic 構文違反など、シグネチャ型推論では表せない
診断は残す。型推論エラーと runtime policy error を同じ kind にまとめない。

## phase ownership

### Spire

- function slot と call-site type application の構文を保持する
- direct TypeCtorTrait application を signature の許可位置で構文化する
- `::<$F: Trait>` のような inline constraint を parse error にする
- `do` と `<-` を block 固有構文として保持する
- `@intrinsic def do...` を docs/signature 表示用宣言として保持する

Trait identity が必要な判定を名前文字列だけで行わない。

### Sigil

- Trait、root family、型 owner、callable identity を canonical identity へ解決する
- direct Trait occurrence と名前付き `$F` の declaration origin/span を保持する
- builtin と通常関数を、callable identity の違いを保ったまま同じ signature input へ渡す
- `do` intrinsic の構文 identity を解決する

### Scar

- 全 callable signature を共通 scheme へ正規化する
- root ごとの constructor variable と位置ごとの capability view を構築する
- argument、slot、expected return を一つの constraint set として解く
- expected type を型 shape に従って再帰伝播する
- `do` intrinsic から Monad／条件付き Alternative obligation を生成する
- origin を保持した構造化 failure を diagnostics へ渡す
- Forge 前に unresolved constructor と pending dispatch を拒否する

### Forge / Eldr

- concrete type と concrete trait dispatch だけを受け取る
- TypeConstructor inference、fallback impl 選択、runtime dictionary lookup を行わない
- builtin の runtime target と opcode contract は既存どおり保持する

## テストマトリクス

### declaration well-formedness

- single TypeCtorTrait constraint の direct parameter / return / slot
- named `$F` と一つの TypeCtorTrait bound
- named `$F` と複数の Trait bound
- inline slot bound の拒否
- TypeCtorTrait bound のない `$F<$A>` の拒否
- return-only constructor に必要な function slot の有無
- payload 省略の成功と、関係を必要とする箇所での named payload

### call-site inference

- 値引数だけから carrier を決定
- expected return だけから carrier を決定
- 明示 slot だけから carrier を決定
- 値引数、期待型、明示 slot の複数根拠が一致
- 上記の任意の二根拠が不一致
- 根拠不足による ambiguity
- 明示 head はあるが固定型引数が不足する ambiguity
- impl 登録順を変えても結果と診断が変わらない

### root と capability

- `Functor` と `Monad` が同じ root として同じ carrier を要求
- 同じ root に異なる carrier を渡す失敗
- 異なる root に異なる carrier を渡す成功
- `Functor` parameter から `bind` を呼ぶ失敗
- `Monad` parameter から `bind` を呼ぶ成功
- 名前付き `$F: Monad + Alternative` から両能力を使う成功

### payload と固定引数

- 同じ carrier で mapped payload が変化する成功
- named `$A` の再出現による payload 同一性
- omitted payload 同士を不要に同一視しないこと
- `Either<String, Int>` と `Either<String, Boolean>` の成功
- `Either<String, Int>` と `Either<Error, Boolean>` の二地点診断

### expected propagation

- block tail
- `if` の両 branch
- `match` の全 arm
- tuple / list / map 要素
- struct / record field
- generic enum / constructor payload
- 既知 closure signature
- user function / trait helper / builtin return

各 shape で TypeCtorTrait helper 名を変えても推論経路が変わらないことを確認する。

### builtin parity

- user function と同型の builtin が同じ成功結果になる
- argument mismatch、expected mismatch、ambiguity が同じ failure reason になる
- runtime builtin ID と static dispatch が型推論後も維持される
- intrinsic が共通 callable route に誤登録されない

### `do`

- 明示 `do<Option>`
- expected result だけで carrier を決定
- 最初／途中／最後の `<-` RHS だけで carrier を決定
- RHS の順序を入れ替えても結果が同じ
- bare monadic expression、`pure`、`return`、`guard` の carrier 一致
- 異なる carrier の二地点診断
- `Either` の固定引数一致／不一致
- total pattern は Monad だけで成功
- partial pattern は Alternative を追加要求
- Alternative を持たない carrier の capability error
- `guard` 名を do checker が特別扱いしていないこと
- `Result` 固有 failure を do checker が生成しないこと

標準型だけでなく、ユーザー定義 TypeCtorTrait 実装を使う fixture を含め、型名の
hardcode がないことを固定する。

## 対象外

今回変更しないものは次のとおり。

- trait definition `where` の `Self: Type<...>` 型形状指定
- TypeCtorTrait impl `where` の `TypeCtorTrait.$Slot` mapping
- trait definition / impl の where 三分類と phase ownership
- mapped slot の決定規則
- trait impl coherence と parent coverage の意味
- 通常 trait の full obligation identity
- runtime trait object、dictionary passing、動的 dispatch
- `=?` を一般 Monad failure にする変更
- `Result` など特定データ型向けの新しい do failure 規則
- arbitrary higher-kinded type variable と一般 kind system

## 受け入れ基準

1. 通常関数、trait helper、非 intrinsic builtin が同じ callable scheme と call checker を使う。
2. direct TypeCtorTrait 表記と名前付き `$F` + function `where` が同じ内部制約へ正規化される。
3. 同じ root の全 occurrence が同じ具象 carrier を要求し、位置別 capability は混ざらない。
4. 引数、明示 slot、期待型のどこからでも carrier を推論でき、結果が検査順に依存しない。
5. payload を観測しない signature では型引数を省略できる。
6. `guard` は return-only constructor slot を持ち、caller は明示 head または期待型で具体化できる。
7. `do` は intrinsic として Monad／条件付き Alternative 制約を生成し、全 monadic value を同じ carrier に統一する。
8. `do`、helper、builtin の型推論に具象データ型名の分岐がない。
9. carrier conflict は両方の source span と完全な型を示す。
10. ambiguity は fallback impl を選ばず typecheck error になる。
11. Forge に unresolved constructor、pending dispatch、abstract runtime carrier を渡さない。
12. trait definition／impl の型形状指定と slot mapping は変更されない。
