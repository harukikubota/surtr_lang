# MonadT を通常のユーザ定義型として表すための言語拡張仕様

## 1. 状態・読み方

- 配置: `/doc`。N03 の nominal constructor parameter は実装済み。N04 の parameterized TypeCtorTrait / MonadT 契約は未実装の入力。
- 基準 commit: `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 通常 Monad インスタンス追加とは別タスクで実装する。
- 本書では「確定要件」「初回実装で採用する最小インターフェース」「実装前の確認項目」を区別する。
- 確認項目を実装担当が暗黙に拡張・一般化して埋めない。

入力は本会話、および「Surtr Monad Transformer 検討メモ」§1–3・§7・§11–12。「MonadT / Alternative / do 構文 検討メモ」の実装型固有意味論は別紙 `monadt_standard_types_spec.md` に置く。

既存契約は[`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)のReturnTypeArgument、role付き型リスト、Trait applicability、dispatch規則である。旧入力の実装済み部分は同正本へ移管済みであり、旧ドラフトは現行仕様に書き換えない。

ただし、本書で確定する次の未実装契約については、本書が既存の計画・正本にある反対の記述を置き換える実装入力となる。

- Trait-head と nominal declaration の `$P: Bound` を受理せず、binder と `where` constraint を分離する。
- TypeCtorTrait に Trait-head type parameter を許可し、constructor slot とは別metadataとして保持する。
- TypeCtorTrait の call-site ReturnTypeArgument に完全な型applicationと `_` を含む型applicationを許可し、`do` も同じ規則を使う。

実装開始前に `docs/dev/Trait_system_spec.md`、`doc/do_intrinsic_spec.md`、関連する実装計画をこの契約へ整合させる。衝突したまま旧規則と新規則を併存させたり、いずれかへfallbackしたりしない。

## 2. 確定要件

1. `Monad` / `MonadT` はユーザ定義型にも開く。標準型名の allowlist で利用可否を決めない。
2. MonadT 実装型は通常の nominal data type であり、引数・戻り値・property・field・collection に保持できる。
3. 現行の `defmod`、`deftrait`、`impl Trait for Type`、inherent impl の関数定義と型入力規則を維持する。
4. `::<...>` は ReturnTypeArgument であり、引数由来の型変数を任意に指定する一般的な generic parameter list にしない。
5. 新しい型解決機構は、通常型に constructor parameter を保持し、宣言通りに適用するための最小範囲に限定する。
6. 実行されるデータ値は具象型。REPL の次の入力まで未確定の carrier を持ち越さない。
7. dyn Trait、runtime Trait dictionary、実行時 impl 探索、暗黙 lift、暗黙 delegation を導入しない。
8. `MonadT` を引数型として受け取る新しい抽象 API は作らない。`OptionT<Result, Int>` 等の通常型を引数に取ることは許可する。
9. `IdentityT` は対象外。型 constructor としての Transformer 自体を抽象化する新APIも対象外。

## 3. 何を追加し、何を追加しないか

| 区分 | 内容 |
|---|---|
| 追加 | nominal declaration の型parameterと、`where` に置く declaration constraintを分離する |
| 追加 | TypeCtorTrait で形状が判明した constructor parameter を nominal field の型式で適用する |
| 追加 | TypeCtorTrait の Trait-head parameter を captured parameter として保持する |
| 再利用 | 型変数導入元、既存 constructor slot、canonical type、role付き型リスト、substitution、solver |
| 再利用 | `Self` の mapped-slot 置換、coherence、applicability、parent coverage、static dispatch |
| 追加しない | 型 lambda、任意の高階型関数の推論、associated type、新しい parameterized `where` RHS |
| 追加しない | property 名・field 構造から MonadT impl や base carrier を自動発見する仕組み |
| 追加しない | MonadT 実装型だけの Facet/パターン/REPL特例 |

これは限定された constructor polymorphism の拡張である。「型機能の拡張が一切ない」とは扱わない。ただし、既存 TypeCtorTrait の適用と構造的照合で必要な能力に閉じる。

## 4. nominal 型定義の最小表記

Trait-head type parameter と Trait constraint を分離する方針を nominal declaration にも適用し、正規形は次とする。

```surtr
defstruct OptionT<$M, $A>
where
  $M: Monad
{
  inner: $M<Option<$A>>
}
```

`$M` / `$A` は nominal head で導入される。`where $M: Monad` は導入済み `$M` の能力を宣言する。`where` を未知の型変数の導入元にしない。

`$M` は、この宣言で TypeCtorTrait の constructor shape を要求され、`$M<T>` として適用される parameter。`$A` は通常の payload 型parameter。大文字の綴りや `$M` という名前で分類しない。

`Monad` の unary shape は親の Functor から得る。`$M<Option<$A>>` は「`M` の mapped slot に `Option<A>` を入れて得られる通常型」であり、Monad 値を runtime に持ち上げる操作ではない。

型parameterに constraint を併記する `defstruct OptionT<$M: Monad, $A>` は受理しない。同一意味の複数surfaceを作らず、capability constraint は `where` に統一する。

`defrecord` / `defenum` の同等な generic parameter を扱う場合も、同じ metadata と well-formedness 検査を使う。`deferror` / Error の運搬制限を、この拡張に便乗して変更しない。

## 5. 型の適用と格納

```text
OptionT<Result, Int>
  nominal head: OptionT
  parameter M: concrete unary Monad carrier Result
  parameter A: concrete value type Int
  field inner: Result<Option<Int>>
```

型引数として渡された `Result` は compile-time の constructor であり、runtime field は `Result<Option<Int>>` の通常値を持つ。runtime に `$M` オブジェクトや Trait dictionary を格納しない。

```surtr
defstruct Cache<$M, $A>
where
  $M: Monad
{
  current: OptionT<$M, $A>
  history: List<OptionT<$M, $A>>
}
```

宣言中の型変数は compile-time の型入力であり、実行時 instance は例えば `Cache<Result, Int>` になる。

次の使い分けを維持する。

| 表記・位置 | 扱い |
|---|---|
| 宣言内の `$M<T>` | 宣言済み constructor parameter を型式内で適用 |
| `OptionT<Result, Int>` | 具象の nominal data type |
| field 型の `MonadT` / `Monad` | dyn値を意味する表記としては受理しない |
| `x = $M` / `call($M)` | constructor を値として扱わない |
| `$M` 単独を runtime field 型にする | 未適用constructorは値型ではないため拒否 |
| 新たな `$M` を式中の注釈だけで導入 | 拒否。既存の型変数scope規則を維持 |

通常の型注釈位置にある `_` は constructor parameter の inference hole にしない。既存どおり `Hole` 型として扱い、その型に関する通常の型操作を行わない。したがって、次の表記を constructor inference として受理しない。

```surtr
value: OptionT<_, Int>                  # NG
arg: OptionT<_, Int>                    # NG
def f() -> OptionT<_, Int>              # NG
value: OptionT<Either<String, _>, Int>  # NG
```

通常型注釈で `OptionT` の `$M` を指定する場合、carrier は明示的かつ完全に型形成可能でなければならない。

```surtr
value: OptionT<Result, Int>
```

call-site ReturnTypeArgument 内の `_` は、そのRTA位置の推論を他の型制約へ委ねる既存の inference hole であり、通常型注釈の `Hole` とは別の構文意味を持つ。この一般則は通常関数や `TryFrom` などのTrait methodにも適用する。`Enum<_, ...>::Variant` の既存 inference hole は別の専用surfaceとして維持し、相互fallbackを作らない。

## 6. well-formedness と型変更

nominal 型の明示された `where` constraint は、その型の適用が合法であるための条件である。型の runtime component や refinement payload にはしない。

型を使用・具体化・再構築するとき、宣言parameterを実際の型引数へ置換し、必要な obligation を既存 solver で検査する。

```text
WF(OptionT<M,A>) requires:
  M が要求された constructor shape を持つ
  M の Monad capability が証明できる
  A と M<Option<A>> が既存の型利用規則を満たす
```

対象は parameter、return、local annotation、field、nested collection、constructor、callable instantiation、Facet の型変更可能な再構築を含む。

例えば `OptionT<M,A> -> OptionT<N,B>` では、出力側について `N: Monad` を証明する。`M` が合法だったことだけで出力を受理しない。

また、複数 field が同じ型parameterを共有する構造体では、一つの field を変えた結果の全field型が整合する必要がある。Facet が局所の値型だけを検査して、矛盾した enclosing type を作ってはならない。

### 6.1 generic な宣言境界

rigid generic の能力は明示された `where` constraint / parent closure から証明する。型を使った事実だけで、新しい generic bound を周囲へ暗黙導入しない。

```surtr
# 有効な形。
def keep(value: OptionT<$M, $A>) -> OptionT<$M, $A>
where
  $M: Monad
{
  value
}
```

この `$M: Monad` は nominal type の合法性検査に使われる。既存 `UnusedTraitConstraint` を維持しつつ、新たな nominal well-formedness obligation による使用も正しく記録する。型形成に必要な bound を「body が bind を呼ばないから未使用」と誤判定しない。

### 6.2 Functor の全payload置換

Functor の mapper が返せる `$B` を、impl 側だけの追加制約で狭めない。nominal 型が payload に追加boundを要求するなら、Trait の元契約が要求する出力型全体を作れるかを method contract と parent coverage で検証する。

`fmap` 実装の都合で暗黙の `$B: Show` 等を足す解決は行わない。標準 Transformer の payload にはこのような追加boundを設けない。

## 7. MonadT の最小契約

```surtr
deftrait MonadT<$M>
where
  $M: Monad
  Self: Monad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}
```

これは初回実装で採用する最小インターフェースである。

`$M` は Trait-head type parameter であり、Trait identity の一部として導入する。`$M: Monad` は既存 `where` による Trait constraint とする。Trait-head は型変数の導入だけを担当し、次の表記は受理しない。

```surtr
deftrait MonadT<$M: Monad>  # NG
deftrait MonadT<Monad>      # NG
```

これらの禁止構文に対する diagnostic の help は `where $M: Monad` に一意化し、declaration bound や direct TypeCtorTrait binder という別surfaceを提案しない。

direct TypeCtorTrait 名を callable signature に書く既存表記は、fresh constructor variable と単一の capability constraint へ一意に正規化できる限定 shorthand である。

```surtr
def pure::<Applicative>(value: $A) -> Applicative<$A>
```

これは概念上の `$F: Applicative` と同じ制約へ正規化するが、Trait-head type parameter、nominal declaration binder、impl Trait argument の generic binder へこの shorthand を拡張しない。

| 型入力 | 役割 |
|---|---|
| `Self` | Transformer 適用後の carrier |
| `$M` | base carrier を表す captured Trait parameter |
| `$A` | 両者の mapped payload |

`MonadT` 自身は新しい mapped slot を宣言せず、`Self` の shape は Monad family から継承する。`$M` が同じ Monad family に属していても、`Self` と `$M` を等置しない。

Trait-head parameter と constructor slot は別metadataとして保持する。`MonadT<Result>` の `Result` を `Self: Type<...>` の slot に混ぜない。

TypeCtorTrait の Trait-head parameter を許可する一般規則で実現し、`MonadT` という表示名だけを特例にしない。ただし、parameterized TypeCtorTrait を field や通常値の direct annotation として使う新surfaceは今回追加しない。

## 8. impl の対応付け

```surtr
impl MonadT<$M> for OptionT<$M, $Payload>
where
  $M: Monad
{
  # lift の実装
}
```

必要なのは Trait argument、impl target、親 Monad の slot mapping、method contract を同じ canonical substitution で照合すること。

`$Payload` という名前と Trait method の `$A` を文字列で対応付けない。`Self<X>` は親から確定した mapped slot へ `X` を代入し、captured `$M` を保存する。

impl method の ReturnTypeArgument は、元TraitのRTAへ impl target と Trait arguments を代入した契約を使う。具体化済み target 内に `$M` が現れることを理由に、独自の重複検査・RTA省略例外を追加しない。抽象宣言と specialized impl signature を同じrole付き型リストで比較する。

### 8.1 新しい MonadT 固有の「出現条件」を作らない

`MonadT<Base>` の `Base` は、通常の impl binder/scope規則で導入・解決される必要がある。

```text
impl MonadT<M> for OptionT<N,A>
```

で `M` がどこからも合法に導入されていなければ、通常の未導入型変数のエラーにする。一方、合法な具体Trait argumentを持つ impl を「baseはtargetの特定fieldや特定位置に必ず現れるはず」という MonadT 専用規則で拒否しない。

標準 OptionT/StateT は同じ base 変数を Trait argument と target に書く。これは標準実装の契約であり、型レイアウトを検査する compiler 特例ではない。

### 8.2 representation を探索しない

OptionT の `inner: M<Option<A>>`、ReaderT の `R -> M<A>`、StateT の `S -> M<(A,S)>` は各通常型の宣言で検査する。

MonadT impl の検査では、field 名 `inner` / `run_state`、field 数、body の特定形状を正しさの根拠にしない。`lift` が `M<A> -> Self<A>` を型正しく実装するかを通常の method body 検査で確認する。

型の合法性と法則の成立は別である。Monad / lift の法則は標準実装のテスト契約とし、ユーザの任意実装をコンパイラが証明したとは扱わない。

## 9. 呼び出し・推論・ReturnTypeArgument

call-site ReturnTypeArgument の各項目は、具象型を明示するか、`_` によってその位置の推論をvalue argument、expected return、型注釈、その他のsignature制約へ委ねるかをユーザが選択する。ReturnTypeArgument list全体の省略は、全項目を `_` にした場合と同じ制約を生成する。この規則は通常関数、Trait method、inherent methodに共通である。

```surtr
converted = try_from::<Int>("1")
converted: Result<Int> = try_from::<_>("1")
converted: Result<Int> = try_from("1")
```

上の三例では、1行目は `TryFrom::try_from` のRTA slotを明示し、2・3行目は同じslotの解決をexpected returnの `Int` へ委ねる。`_` は TypeCtorTrait 専用ではなく、RTAを宣言する通常Trait methodにも同じ意味で適用する。

TypeCtorTrait を要求する call-site ReturnTypeArgument は、完全な carrier 型、constructor head、または `_` を含む型applicationを受理する。

| 表記 | 意味 |
|---|---|
| 完全な型application | 全型引数を固定constraintとする |
| `_` を含む型application | `_` の位置だけ fresh inference variable とする |
| constructor head のみ | 不足する constructor argumentをすべて推論対象にする |

最終的には mapped slot と captured argument を含む完全な carrier identity が確定しなければならない。推論源は TypeCtorTrait の slot mapping、callable signature、value argument、expected return、その他既存の型制約に限定し、impl 数、登録順、標準型名、field 構造から不足型を補完しない。

`Either<L, R>` で `R` が mapped slot、`L` が captured argument の場合は次のように解決する。

```surtr
v = pure::<Either<String, _>>(10)       # OK: Either<String, Int>
pure::<Either<String, Int>>(10)         # OK
pure::<Either<String, String>>(10)      # NG
v = pure::<Option>(10)                  # OK: Option<Int>
v = pure::<Either>(10)                  # NG: Either<?, Int>
v: Either<String, Int> = pure::<Either>(10) # OK
```

この位置の `_` も通常のRTA inference holeであり、`Hole` 型ではない。推論終了時に解決できなければ ambiguity とする。TypeCtorTrait に固有なのは、完全なcarrier型やconstructor head、`_` を内包する型applicationをRTA slotの入力として扱い、mapped slotとcaptured argumentを解決する規則である。

```surtr
source: Result<Int> = Ok(1)
value: OptionT<Result, Int> = MonadT::lift(source)
```

base と payload は引数の具体型から、出力carrierは期待型から得る。`lift` に対応しない第2・第3のRTAを追加して base/payload を重複指定させない。

結果carrierを `OptionT`、`ReaderT`、`StateT` の登録順や impl 数から選ばない。期待型も他の型入力もなく出力が曖昧なら、既存 ambiguity 診断を出す。

`pure` / `return` は値引数から mapped payload を導出できる。`guard` が `Alternative<Unit>` を返す場合は signature 自身から mapped payload が `Unit` と確定する。`do` の carrier ReturnTypeArgument も独自surfaceを持たず、同じ resolver を使う。

```surtr
do::<Option> { ... }
do::<Either<String, _>> { ... }
do::<_> { ... }
do { ... }
```

applied carrier を `do` だけで禁止しない。captured argument を含む完全な carrier identity が確定しなければ ambiguity とする。

### 9.1 `Alternative::empty`

`empty` は完全な carrier application から mapped slot を取得できるため、carrier と payload を別々の ReturnTypeArgument として要求しない。

```surtr
deftrait Alternative
where
  Self: Applicative
{
  def empty::<Self>() -> Self
}

r = Alternative::empty::<Option<Int>>()
r: Option<Int> = Alternative::empty()
r = Alternative::empty::<Option<_>>() # NG: 他の推論源がない
```

`Alternative` は通常のユーザ実装可能 Trait である。コンパイラは `empty` の生成方法を標準型や runtime representation から構築できる型に限定しない。例えば captured 型 `$L: Default` を持つ `Either<$L, $A>` が `Default::default()` から Left 値を生成する実装も、通常の型検査、coherence、Trait contract を満たす限り許可する。標準ライブラリが `Either` / `Result` に実装を提供するかは標準APIの意味論判断であり、言語機能上の実装可能性とは分離する。

通常helperの例:

```surtr
def some::<$M>(value: $A) -> OptionT<$M, $A>
where
  $M: Monad
```

ここでは `$M` だけが戻り値由来。`value` から導入される `$A` をRTAへ重複させない。

```surtr
def map_t(
  self: OptionT<$M, $A>,
  mapper: ($M<Option<$A>> -> $N<Option<$B>>)
) -> OptionT<$N, $B>
where
  $M: Monad
  $N: Monad
```

`$N` と `$B` は mapper の関数型の戻り値側に現れるが、mapper 自体が value parameter なので引数由来の型入力である。これらを「戻り値だけ」と扱ってRTAを要求しない。

`where $T: MonadT<$M>` という parameterized bound syntax は、本件のために解禁しない。full TraitRef を保持する既存の impl head / method dispatch と、bare capability の規則を維持する。

## 10. Facet と REPL

public field を持つ標準 Transformer は通常の Facet path で操作できる。Transformer全体だけでなく、公開されている representation field も通常の型として扱う。

ただし、field を省略してコンテナ層を暗黙透過する操作、MonadT を発見して自動 lift/run する操作は作らない。Facet自身の既存 Result 文脈・variant・readonly規則も変えない。

REPL の成功入力では、出力carrier・base・captured型・payload型・選択済みcallableを具体化してから値を保存する。一般化可能な callable のcompile-time schemeと、未具体化のTransformerデータ値を区別する。

失敗した型推論では、bound・carrier・pending obligationを含めcheckpointを復元する。以前保存したTransformer値の型を後続入力で変えない。

## 11. インターフェース確定ゲート

MT-I01 は、以下の確定内容により **CLOSED** とする。

1. captured argument を持つ carrier は TypeCtorTrait RTA 内の型applicationで明示できる。
2. RTA 内の `_` は inference variable として受理する。
3. constructor head だけの指定では、全constructor argumentを推論対象にする。
4. mapped slot と captured argument を含む完全な carrier identity が最終的に必要である。
5. 推論根拠が不足すれば ambiguity とする。
6. 型lambdaは導入しない。
7. impl一覧、登録順、field探索から不足型を推測しない。
8. 通常型注釈の `_` は対象外であり、`Hole` 型の既存意味を維持する。
9. `do` も独自carrier表記を持たず、同じ TypeCtorTrait RTA 規則を使う。

残る確認項目は次のとおり。初回の最小対応を超える部分を自動実装しない。

| ID | 確認事項 | 初回の扱い |
|---|---|---|
| MT-I02 | specialized `lift` のRTA表示と型リストの対応 | 抽象contractから構造置換し、既存impl signature検査で固定。表示のための型解決fallbackは作らない |
| MT-I03 | nominal宣言boundがProofEnvironmentと未使用制約検査へ渡る位置 | declaration/body/instantiation境界のテストで固定し、利用箇所からの暗黙bound導入はしない |
| MT-I04 | parameterized TypeCtorTraitのdirect signature表記 | 今回は追加しない。通常型の引数とqualified method呼出しに限定 |

## 12. 受け入れ条件

| ID | 検証 |
|---|---|
| MT-L01 | nominal boundのある通常ユーザ型を構文・型検査できる |
| MT-L02 | `$M<T>` のconstructor shapeとarityを既存metadataから検査する |
| MT-L03 | 適用先のnested field/property型まで型置換が到達する |
| MT-L04 | 入出力・Facet再構築・nominal instanceのdestination boundを検査する |
| MT-L05 | rigid genericの不足boundを拒否し、型利用だけで仮定を追加しない |
| MT-L06 | 型形成に必要な明示boundを正しく使用済みとして扱う |
| MT-L07 | TypeCtorTraitのTrait argumentとmapped slotを区別する |
| MT-L08 | `Self` とbaseが同じfamilyでも独立carrierとして扱える |
| MT-L09 | MonadT型名に依存しない通常のimpl matching・coherenceを使う |
| MT-L10 | `lift` の出力carrierを期待型から解き、根拠なしの候補選択を拒否する |
| MT-L11 | RTAの個数・順序・型関係を抽象contractとimplで一致させる |
| MT-L12 | 標準型以外のbase MonadとTransformerの組を定義できる |
| MT-L13 | 公開representation fieldをFacetで読み取り・通常の合法な更新ができる |
| MT-L14 | REPLに具象データ値だけを保存し、エラー後の継続とcheckpointを保つ |
| MT-L15 | dyn値・runtime dictionary・新しいtype lambda・field探索を導入しない |
| MT-L16 | 標準Transformerとは別に、最小ユーザ定義型で言語機能の完了を検証する |
| MT-L17 | Trait-head binder と Trait constraint を分離し、`deftrait MonadT<$M: Monad>` / `deftrait MonadT<Monad>` を拒否して、diagnostic の help を `where $M: Monad` に一意化する |
| MT-L18 | `pure::<Either<String, _>>(10)` を `Either<String, Int>` に解決する |
| MT-L19 | 通常関数・Trait methodを含むRTA内の `_` をそのslotの推論委譲として扱い、通常型注釈の `Hole` と区別する |
| MT-L20 | `OptionT<_, Int>` を constructor inference として受理しない |
| MT-L21 | constructor headのみのRTAは全captured/mapped argumentが導出可能な場合だけ成功する |
| MT-L22 | `do::<Either<String, _>>` が通常のTypeCtorTrait RTA解決経路を使用する |
| MT-L23 | `Alternative::empty::<Option<Int>>()` からmapped slotを取得し、payload用の追加RTAを要求しない |
| MT-L24 | user-defined `Alternative` impl の `empty` 生成方法を標準型・representationで制限しない |

## 13. 実装後の移管

実装開始前に、§1で列挙した置換対象を `docs/dev/Trait_system_spec.md`、`doc/do_intrinsic_spec.md`、関連する実装計画へ反映し、相反する旧契約を削除する。実装後は言語機能の確定契約を利用者向け型注釈・Trait・struct文書へ移管する。標準TransformerのAPIは `monadt_standard_types_spec.md` と個別 `@doc` の担当であり、本書に複製しない。

N03 の MT-I01/03 と MT-L01–06・MT-L13–15 該当部分は、2026-09-09 に
`docs/dev/Trait_system_spec.md`、`docs/site/{language-reference,structs,type-annotations,facet}.md`へ移管した。
MT-I02/04 と MT-L07–12・MT-L16 は N04 以降の入力として本書に残す。
