# 型コンストラクタ trait と代数 API の改修案

> impl coherence、generic overlap、call-site 型推論の規範は [`trait_coherence_and_callsite_inference_revision.md`](./trait_coherence_and_callsite_inference_revision.md) に従う。本書の「specialization」は concrete constructor witness の確定を意味し、overlapping impl の優先順位を意味しない。

## 目的

既存の `Type<$A>` constructor slot、trait 継承、compile-time trait dispatch を使い、
runtime trait object を増やさずに container-oriented API を型注釈位置へ広げる。

この改修で導入・整備する対象は次である。

- `Applicative<$A>` のような constructor-trait application
- `Alternative` と `<|>`
- `Bifunctor` など、binary constructor trait を表現できる型検査基盤
- runtime representation を持たない関数シグネチャ alias
- `Monoid<$A>` と List 固有の代数 helper
- 既存 `Functor` / `Applicative` / `Monad` の FunParams 宣言を入力位置規則へ適合させる改修

これは HKT、`dyn Trait`、runtime dictionary、trait object を導入する改修ではない。hidden
constructor witness は型検査と specialization のためだけに使い、codegen 前に必ず具体化する。

## 今回見送る対象

- `Foldable` / `Bifoldable` trait
  - 現在の標準型で自然な対象は `List` と `Option` にほぼ限られる。
  - `NonEmptyList`、`Tree` などの concrete target が導入された時点で別提案とする。
  - 畳み込みは trait ではなく、今回導入する List 固有 helper として提供する。
- `Profunctor`
  - 関数合成は既存の `Compose` と通常の関数型推論で表現でき、`dimap` を標準 API として持つ
    実益が現時点でない。

## 非目標

- `dyn Trait` / trait object / runtime dictionary
- surface 上の `$F<$A>` のような型コンストラクタ変数
- 通常型 alias、NewType、alias の runtime representation
- trait parameter と constructor slot を併せ持つ trait
- concrete constructor を失った値を field や container に保持すること
- `Traversable`、`Bitraversable`、`MonadTrans` などの API 導入

## trait の二分類

### 通常 trait

通常 trait は trait parameter を持てる。`TryFrom<$To>` の `$To` は変換先を表す
通常の trait parameter であり、実装対象 `Self` は変換元型である。

```surtr
deftrait TryFrom<$To> {
  def try_from::<$To>(self: Self) -> Result<$To, Error>
}
```

### constructor trait

direct に `Self: Type<...>` を持つ、またはその constraint を親 trait から継承する trait を
constructor trait と呼ぶ。constructor trait の trait parameter arity は常に 0 とする。

```surtr
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

deftrait Applicative
where
  Self: Functor
{
  def pure(value: $A) -> Self<$A>
  def ap(mapper: Self<($A -> $B)>, value: Self<$A>) -> Self<$B>
}
```

次は error とする。

```surtr
deftrait Bad<$A>
where
  Self: Type<$A>
{
  # ...
}
```

この規則により、`TryFrom<$To>` は trait parameter application、
`Applicative<$A>` は constructor slot application として一意に区別できる。

## constructor-trait application

unary constructor trait の `Trait<$A>` は、型検査中だけ次の意味へ下げる。

```text
Trait<$A>
=> F<$A> where F: Trait
```

`F` は surface に出ない hidden constructor witness である。call-site の具象値または期待型から
必ず一意に確定し、未解決のまま codegen へ渡してはならない。

```surtr
def map(
  value: Applicative<$A>,
  mapper: ($A -> $B)
) -> Applicative<$B>
```

上記は `F<$A> -> F<$B>` を表す。`Option<Int>` を渡した呼び出しは `Option<String>` へ、
`Result<Int>` を渡した呼び出しは `Result<String>` へ静的に確定する。

### root と witness の共有

- `Self: Type<...>` を直接宣言する trait を constructor root とする。標準 unary lineage の root は
  `Functor` である。
- hidden witness の identity は、同一 callable signature 内の constructor root ごとに生成する。
- 同じ root を継承する trait application は同じ witness を共有する。たとえば `MyTrait` が
  `Applicative` を継承するとき、`MyTrait<$A>` と `Applicative<$A>` は同じ witness を使う。
- 異なる root は独立した witness として解決する。同じ concrete type が両 root を実装していても、
  witness を同一視しない。
- 異なる root の trait method は相互に利用できない。ある値に対する dispatch は、その値の witness が
  持つ root と trait bound だけから解決する。
- binary root では同じ規則を arity 2 に拡張する。`Bifunctor<$A, $B>` は `F<$A, $B>` を表す。

### bare trait annotation

要素型を API に露出しない parameter / local binding では、constructor trait の bare 名を constraint
shorthand として許可する。

```surtr
def audit(value: MyTrait) -> Unit {
  # call-site の value から concrete F<$A> を確定する
}
```

これは `dyn MyTrait` ではない。hidden slot と witness は静的に確定し、値の runtime representation は
元の concrete type のままである。

要素型・戻り値との対応を表す callable signature では明示 application を使う。

```surtr
def transform(value: MyTrait<$A>, mapper: ($A -> $B)) -> MyTrait<$B>
```

bare constructor trait は direct parameter / local annotation にだけ許可する。`List<MyTrait>`、tuple、
field、closure signature 内など、別の型の構成要素に bare trait を置くことは error とする。field は
`List<Result<$A>>` のように named concrete constructor を明記しなければならない。

`Trait<$A>` のような application は callable signature shape として展開する。対応する入力値または
call-site の期待型から witness を確定できる場合だけ許可し、bare trait のように値として保存してはならない。

### `pure` / `empty` の未確定利用

入力値だけでは constructor witness が決まらない API は、期待型などから concrete container が確定するときだけ許可する。

```surtr
value: Option<Int> = pure(1)                       # 可
none: Option<Int> = Alternative::empty::<Int>()    # 可

pure(1)                              # error: constructor witness が未確定
Alternative::empty::<Int>()          # error: constructor witness が未確定
```

`Self` は dispatch target を表す hidden FunParam であり、call-site の明示型引数には書かない。
`Alternative::empty::<Int>()` の `Int` は `$A` を指定し、`Self` は期待型から解決する。

## FunParams の規則

各 method signature の型変数（`Self` を含む）は、入力位置で **FunParams** または **通常の値引数型** の
どちらか一方だけから導入しなければならない。同じ型変数を両方に書くこと、どちらにも書かず return type
だけに出すことは error とする。この規則は textual な出現回数ではなく、型変数を導入する入力チャネルに
対して適用する。

| API | FunParams で導入する型変数 | 値引数型で導入する型変数 |
|---|---|---|
| `Functor::fmap` / `Applicative::ap` / `Monad::bind` | なし | `Self`, `$A`, `$B` |
| `Applicative::pure` / `Monad::return` | `Self` | `$A` |
| `Alternative::empty` | `Self`, `$A` | なし |
| `TryFrom::try_from` | `$To` | `Self` |
| `Default::default` | `Self` | なし |

したがって、現行標準定義の次のような宣言は error とし、改修対象とする。

```surtr
def fmap::<Self, $A, $B>(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
def pure::<Self, $A>(value: $A) -> Self<$A>
```

標準 `Functor` / `Applicative` / `Monad` と各 concrete impl は、上表に従う FunParams へ移行する。

## 関数シグネチャ alias

`type` の第一用途は、runtime representation を持たない関数シグネチャ alias に限定する。

```surtr
type Mapper<$A, $B> = ($A -> $B)
type Predicate<$A> = ($A -> Boolean)
type Reducer<$Acc, $A> = ($Acc, $A -> $Acc)
type Semigroup<$A> = ($A, $A -> $A)
```

宣言した型変数はすべて RHS で使われなければならない。alias は型注釈位置で正規の関数型へ展開され、
runtime type、constructor、pattern、`impl` target、TypeRegistry entry を持たない。

alias は通常 type owner と別 namespace を作らない。既存の型名規則と同じ global type namespace に登録し、
`Namespace::Ty` を含む canonical type name により衝突検査・前方参照・解決を行う。型に visibility は持たない。

`Semigroup<$A>` は結合演算を表す関数値の型である。identity と演算を一緒に値として渡すため、
`Monoid<$A>` は構造体にする。

```surtr
defstruct Monoid<$A> {
  empty: $A,
  combine: Semigroup<$A>,
}
```

結合法則・単位元法則は compiler が証明しない。標準 `Monoid` 値の `@doc` と property-based law test で
契約化する。

### List 固有の代数 helper

畳み込みは `Foldable` trait に一般化しない。`List` owner に、現在の List API と矛盾しない次の helper を
置く。

```surtr
impl List {
  # 既存 API。初期値ありの left fold。
  def reduce(values: List<$A>, initial: $Acc, step: Reducer<$Acc, $A>) -> $Acc

  # empty を単位元として返す total fold。
  def fold(values: List<$A>, monoid: Monoid<$A>) -> $A

  # map 後に monoid で畳み込む。
  def fold_map(
    values: List<$A>,
    monoid: Monoid<$M>,
    mapper: Mapper<$A, $M>
  ) -> $M

  # 初期値なしの畳み込み。空 list は None。
  def reduce_with(values: List<$A>, combine: Semigroup<$A>) -> Option<$A>
}
```

`List::reduce` は既存の public API として維持する。`reduce_with` は初期値なしの契約を別名にするため、
既存 `reduce` と衝突しない。各 helper は List の先頭から末尾へ評価する。

## 導入する trait と API 群

### 既存 unary lineage

| Trait | 親 / shape | 必須 API | default API の候補 |
|---|---|---|---|
| `Functor` | `Self: Type<$A>` | `fmap` | `replace` |
| `Applicative` | `Self: Functor` | `pure`, `ap` | `map2`, `lift2` |
| `Monad` | `Self: Applicative` | `return`, `bind` | `join`, `tap` |
| `Alternative` | `Self: Applicative` | `empty`, `choose` | `or_else`, `guard` |
| `Contravariant` | `Self: Type<$A>` | `contramap` | — |
| `Invariant` | `Self: Type<$A>` | `imap` | — |
| `Comonad` | `Self: Type<$A>` | `extract`, `extend` | `duplicate` |

### Alternative

`Alternative` は `Option` と `List` に実装する。`Result` には、複数 error の選択・結合に関する公開契約が
未確定なため実装しない。

```surtr
deftrait Alternative
where
  Self: Applicative
{
  def empty::<Self, $A>() -> Self<$A>
  def choose(left: Self<$A>, right: Self<$A>) -> Self<$A>
}
```

`<|>` は `Alternative::choose` の surface sugar とする。`Alternative` は auto import しない。`choose` は
trait contract 用の名前であり、通常の user code は `<|>` を使える。

- `<|>` は 3 文字 token として最長一致で tokenize する。
- `<|>` は既存 flow 演算子と同一優先度・左結合である。ただし apply / compose / bind の flow operator
  class には含めず、Choice 専用 class とする。
- Choice は pipe injection、contextual payload の取り出し、早期 return のいずれも行わない。通常の二項
  trait dispatch として `Alternative::choose(left, right)` に lower する。
- `<|>` の直前・直後で改行してはならない。`left`、`<|>`、`right` は同一行に置く。

```surtr
primary <|> fallback |>= next  # (primary <|> fallback) |>= next
primary |>= next <|> fallback  # (primary |>= next) <|> fallback

primary
<|> fallback                   # parse error
primary <|>
fallback                        # parse error
```

- `Option`: 左が `Some` なら左を返し、`None` なら右を返す。
- `List`: 左から右への連結を返す。
- 引数は通常の eager evaluation に従う。`choose` は fallback の遅延評価を提供しない。
- law は `empty <|> x == x`、`x <|> empty == x`、結合則とする。
- `Monad::return(value) == Applicative::pure(value)` は standard implementation が守る law とし、compiler は
  証明しない。標準実装を property-based law test で固定する。

### binary constructor lineage

`Type<$A, $B>` を使う trait は HKT を導入せずに扱える。

```surtr
deftrait Bifunctor
where
  Self: Type<$A, $B>
{
  def bimap(
    self: Self<$A, $B>,
    map_left: Mapper<$A, $C>,
    map_right: Mapper<$B, $D>
  ) -> Self<$C, $D>

  # default methods: first, second
}
```

`Bifunctor` は `Pair<$A, $B>` や将来の `Either<$A, $B>` の候補である。現行の `Result<$T>` は abstract
`Error` を固定した unary container として扱うため、Bifunctor の対象にしない。

## user-defined constraint set

named constraint set は新しい `constraint` 構文を作らず、trait 継承で表す。

```surtr
deftrait Recoverable
where
  Self: Alternative
{
  def recover(self: Self<$A>, fallback: Self<$A>) -> Self<$A>
}
```

```surtr
def choose_first(
  primary: Recoverable<$A>,
  fallback: Recoverable<$A>
) -> Recoverable<$A>
```

同一宣言内で同じ direct parent を二度書くことは error とする。複数 parent の展開結果として同じ root が
重複する場合は、root と slot mapping が同じことを確認して正規化・統合する。

## 実装方針

### Spire

- user `type Name<$T...> = (.. -> ..)` を parse する。RHS は function type のみ許可する。
- `Trait<$A>` は既存の generic type annotation と同じ token 形で parse し、trait parameter application か
  constructor-slot application かは parser では決めない。
- `deftrait` の head parameter と `Self: Type<...>` / 継承 constraint の組合せは Scar で検査する。
- `<|>` は Choice token / AST node として parse する。flow 演算子と同じ左結合 tier で parse するが、
  flow 演算子にだけ許可する改行継続を適用しない。

### Sigil

- signature alias を declaration index へ登録し、通常 type owner と同じ global type namespace の canonical
  name で解決する。
- trait / alias の前方参照と canonical type name の重複を既存 declaration index 規則で検査する。

### Scar

- trait metadata に ordinary trait parameter arity、constructor-slot arity、constructor root、親からの slot
  mapping を保持する。
- constructor trait に trait parameter があれば error とする。
- `Trait<$...>` を metadata で判別し、arity を trait parameter または constructor slot に対して検査する。
- callable signature ごとに root 単位の hidden constructor witness を生成する。同じ root は統一し、異なる
  root は独立に解決する。
- input / expected type のいずれからも witness を確定できない call を error とする。
- FunParams と通常引数型について、全型変数の入力チャネルがちょうど一方であることを検査する。
- bare constructor trait の許可位置と、field に concrete constructor を要求する規則を検査する。
- function-signature alias を展開し、未使用 type parameter、arity 不一致、直接・相互循環 alias を error とする。
- explicit direct duplicate parent は error、同一 root / slot mapping の継承由来 bound は正規化する。

### Forge / Eldr

- 新しい runtime type、tag、opcode、dictionary、trait object は追加しない。
- Scar が concrete implementation を確定した後は、既存の trait dispatch / user function dispatch と同じ経路で lower する。

## 検証項目

- `Applicative<$A>` / `Bifunctor<$A, $B>` の concrete specialization 成功
- `TryFrom<$To>` と constructor trait application の識別
- constructor trait が trait parameter を持つ declaration の拒否
- FunParams と値引数型で同じ型変数を二重に導入する宣言の拒否
- 型変数が FunParams / 値引数型のどちらにも導入されず return type だけに現れる宣言の拒否
- 現行 `Functor` / `Applicative` / `Monad` の FunParams を上表どおりへ移行した成功ケース
- `pure(1)` / `Alternative::empty::<Int>()` の unresolved witness 拒否と expected type による成功
- 同一 root の witness 共有、異なる root の independent resolution、異なる root の method を混用した失敗
- `m: MyTrait` の static specialization、`List<MyTrait>` を含む bare trait の非許可位置での拒否、field の
  concrete constructor 要求
- `Option` / `List` に対する `Alternative` の `<|>` 契約と property-based law test
- `<|>` の最長一致 tokenize、同一優先度の左結合、flow 演算子との混在、operator 前後の改行拒否
- `Monad::return` と `Applicative::pure` の標準実装 law test
- `Reducer` / `Semigroup` / `Mapper` alias の展開と、`Monoid` field の関数値呼出し
- `List::reduce` の既存契約、`List::fold` / `fold_map` の単位元・順序、`reduce_with` の空 List `None`
- signature alias の展開、未使用 parameter、arity mismatch、alias cycle、canonical type name 衝突
