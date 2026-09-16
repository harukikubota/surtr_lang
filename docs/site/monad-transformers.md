# Monad transformers

`OptionT`、`EitherT`、`ReaderT`、`StateT` は、base の `Monad` を通常の
Surtr 値として包む標準型です。compiler 専用の runtime 値ではなく、構造体、関数、
List、Facet の値として扱えます。

```surtr
base: Identity<Int> = Identity::new(7)
lifted: OptionT<Identity, Int> = MonadT::lift(base)
mapped: OptionT<Identity, Int> = lifted |*> {|value| value + 1}

Identity::run(OptionT::run(mapped)) # Option::Some(8)
```

`MonadT::lift` の結果 carrier は impl 数や標準型名から選ばれません。上のように型注釈、
引数、または戻り値の文脈から `OptionT<Identity, Int>` を一意に決めます。base の値を
Transformer の計算へ暗黙に混ぜる経路はありません。

## 共通契約

4つの型はすべて `Functor`、`Applicative`、`Monad`、`MonadT<M>` を実装します。
`MonadT::lift` はbase computationを外側のcarrierへ明示的に接続します。

```text
def lift::<Self>(value: $M<$A>) -> Self<$A>
```

Transformerは通常のnominal型なので、field、関数引数、戻り値、collection、Facetへ渡せます。
公開fieldの読み書きは通常のFacet規則に従い、Transformerの内部をcompilerが自動探索することはありません。

`M`とpayload `A`が型注釈や引数から決まらない呼び出しは、標準型の候補数や登録順で補完されず、ambiguityとして拒否されます。
通常の型注釈では完全なcarrierを書き、call-siteのReturnTypeArgumentでは既存のbare head、完全・部分型application、`_`を使います。

### Result effect と failure target

`@result_effect` は引数を取らず、`defstruct` の直前に一度だけ書ける
compiler-owned annotation です。`Monad` と `MonadT<$M>` を
実装する struct にだけ指定でき、field がちょうど一つで public、その field の最外
constructor が captured base `$M` と一致する必要があります。annotation のない型、
非 Monad wrapper、複数 field の struct、関数 field を持つ `ReaderT` / `StateT` に
Result effect を暗黙付与しません。
標準型では `OptionT` だけが明示的な適用対象です。`EitherT`、`ReaderT`、`StateT`、
および user-defined MonadT は、宣言に有効な `@result_effect` がなければ対象になりません。

annotation が有効になるのは base Monad が canonical `Result` に直接具体化された
場合だけです。内部のどこかに `Result` があるだけでは対象になりません。

```text
OptionT<Result, A> -> Result effect
OptionT<List, A>   -> Result effect なし
T<U<Result, _>, A> -> Result effect なし
```

SafeBind `=?` と failureMatcher となる partial `<-` の failure target は
`Result effect > Alternative > Monad` の優先順位で選ばれます。`OptionT<Result, A>`
では `Err(error)` を `inner: Err(error)` として保持し、`OptionT<List, A>` では
`Alternative::empty()` に進みます。`guard` は通常の `Alternative` 関数なので、
`OptionT<Result, A>` でも `guard(False)` は `OptionT::empty()`（`Ok(None)`）です。

この規則は SafeBind RHS の分解規則を変更しません。RHS を自動的に外側一段だけ
分解するのは canonical `Result` だけで、`OptionT<Result, A>` を暗黙に flatten
したり、nested Result を再帰的に分解したりしません。必要な base 接続には引き続き
`MonadT::lift`、`run`、Extractor などを明示します。

次の三例では、同じ `OptionT<Result, _>` でも失敗の起点によって結果が分かれます。

```surtr
deferror Stop { "stop" }

def source() -> Result<Int, Stop> {
  Err(Stop)
}

def wrapped() -> OptionT<Result, Int> {
  value =? source()
  OptionT::some::<Result>(value + 1)
}

OptionT::run(wrapped()) # => Err(Stop("stop"))

mismatch: OptionT<Result, Int> = do::<OptionT<Result, _>> {
  2 <- OptionT::some::<Result>(1)
  OptionT::some::<Result>(3)
}
OptionT::run(mismatch) # => Err(PatternMismatch("Pattern did not match."))

blocked: OptionT<Result, Unit> = guard(False)
OptionT::run(blocked) # => Ok(Option::None)
```

通常関数のSafeBindとdoのfailure matcherはResult effectを使うためErrorを保持します。
一方、`guard`はOptionT自身の`Alternative`を使うためabsenceを返します。

### ユーザ定義のbaseとTransformer

`MonadT`は標準4型のallowlistではありません。通常の`Functor` / `Applicative` / `Monad`を満たす
baseとTransformerを定義し、同じ`MonadT<$M>`経路を利用できます。次は
`tests/fixtures/script/pass/stdmod/user_defined_monad_transformer.srt`と同じ最小構成です。

```surtr
defstruct UserBase<$A> { value: $A }

impl UserBase {
  def new(value: $A) -> UserBase<$A> { UserBase { value } }
  def run(self: UserBase<$A>) -> $A { self.value }
}

impl Functor for UserBase<$T> {
  def fmap(self: UserBase<$A>, mapper: ($A -> $B)) -> UserBase<$B> {
    UserBase::new(mapper(self.value))
  }
}
impl Applicative for UserBase<$T> {
  def pure::<UserBase<$T>>(value: $A) -> UserBase<$A> { UserBase::new(value) }
  def ap(mapper: UserBase<($A -> $B)>, value: UserBase<$A>) -> UserBase<$B> {
    UserBase::new(Function::apply(mapper.value, value.value))
  }
}
impl Monad for UserBase<$T> {
  def return::<UserBase<$T>>(value: $A) -> UserBase<$A> { UserBase::new(value) }
  def bind(self: UserBase<$A>, mapper: ($A -> UserBase<$B>)) -> UserBase<$B> {
    mapper(self.value)
  }
}

@result_effect
defstruct UserWrap<$M, $A>
where
  $M: Monad
{
  inner: $M<$A>,
}

impl UserWrap {
  def new(inner: $M<$A>) -> UserWrap<$M, $A>
  where
    $M: Monad
  {
    UserWrap { inner }
  }

  def run(self: UserWrap<$M, $A>) -> $M<$A>
  where
    $M: Monad
  {
    self.inner
  }
}

impl Functor for UserWrap<$M, $T>
where
  $M: Monad
  $T: Functor.$A
{
  def fmap(self: UserWrap<$M, $A>, mapper: ($A -> $B)) -> UserWrap<$M, $B> {
    UserWrap::new(Functor::fmap(self.inner, mapper))
  }
}
impl Applicative for UserWrap<$M, $T>
where
  $M: Monad
  $T: Applicative.$A
{
  def pure::<UserWrap<$M, $T>>(value: $A) -> UserWrap<$M, $A> {
    UserWrap::new(Monad::return(value))
  }
  def ap(
    mapper: UserWrap<$M, ($A -> $B)>,
    value: UserWrap<$M, $A>,
  ) -> UserWrap<$M, $B> {
    UserWrap::new(Applicative::ap(mapper.inner, value.inner))
  }
}
impl Monad for UserWrap<$M, $T>
where
  $M: Monad
  $T: Monad.$A
{
  def return::<UserWrap<$M, $T>>(value: $A) -> UserWrap<$M, $A> {
    UserWrap::new(Monad::return(value))
  }
  def bind(
    self: UserWrap<$M, $A>,
    mapper: ($A -> UserWrap<$M, $B>),
  ) -> UserWrap<$M, $B> {
    UserWrap::new(Monad::bind(self.inner, {|item| mapper(item).inner}))
  }
}
impl MonadT<$M> for UserWrap<$M, $T>
where
  $M: Monad
  $T: MonadT.$A
{
  def lift::<UserWrap<$M, $T>>(value: $M<$A>) -> UserWrap<$M, $A> {
    UserWrap::new(value)
  }
}

base: UserBase<Int> = UserBase::new(41)
lifted: UserWrap<UserBase, Int> = MonadT::lift(base)
explicit = MonadT::lift::<UserWrap<UserBase, _>>(base)
mapped = Functor::fmap(lifted, {|item| item + 1})
print(to_string(UserBase::run(UserWrap::run(mapped))))
```

`lifted`ではexpected type、`explicit`では明示RTAが出力carrierを決めます。compilerは標準型名、
impl数・登録順、field名・representationからbaseや出力carrierを推測しません。この定義にもruntime
Trait dictionary、暗黙lift、Transformer専用runtime値はなく、通常のnominal値とstatic dispatchだけを使います。
`@result_effect`は`UserWrap<UserBase, _>`では有効化されず、baseをcanonical `Result`へ
具体化したときだけResult effectを提供します。

### `Applicative::ap` の評価順

各 Transformer は mapper 側を先に扱いますが、representation に応じて順序が異なります。

| 型 | 順序 |
|---|---|
| `OptionT` | `mapper.inner` を先に sequence し、`None` なら `value.inner` を sequence しない |
| `EitherT` | `mapper.inner` を先に sequence し、`Left` なら `value.inner` を sequence しない |
| `ReaderT` | mapper と value を同じ環境でこの順に実行し、得た base 値を `Applicative::ap` で結合する |
| `StateT` | mapper transition を先に sequence し、返された次状態で value transition を実行する |

これは Transformer が保持する computation の順序です。`Applicative::ap(mapper, value)` の
引数式自体は通常どおり先に評価され、Transformer が lazy special form になるわけではありません。

## OptionT

`OptionT<M, A>` は `M<Option<A>>` を保持します。`None` と base 自身の失敗は別です。

| 関数 | 型 | 意味 |
|---|---|---|
| `OptionT::new` | `M<Option<A>> -> OptionT<M, A>` | representation を包む |
| `OptionT::run` | `OptionT<M, A> -> M<Option<A>>` | representation を取り出す |
| `OptionT::some` | `A -> OptionT<M, A>` | base の `pure` で `Some` を包む |
| `OptionT::none` | `() -> OptionT<M, A>` | base の `pure` で `None` を包む |
| `OptionT::from_option` | `Option<A> -> OptionT<M, A>` | Option を base の `pure` で持ち上げる |

`OptionT` は `Functor`、`Applicative`、`Monad`、`MonadT<M>`、`Alternative` を
実装します。`Alternative::empty` は `M::empty` ではなく `pure_M(None)` です。

```surtr
none: OptionT<Result, Int> = Alternative::empty()
OptionT::run(none) # Ok(Option::None)
```

baseが`List`なら`empty: OptionT<List, A>`のrepresentationは`[Option::None]`であり、
baseの空List `[]`ではありません。`OptionT::choose`はbase由来のmultiplicityを保持します。
例えば左が`[None, None, Some(1)]`、右が`[Some(2), Some(3)]`なら、各`None`から右の2分岐が
それぞれ生じ、最後の`Some(1)`も保持されます。分岐を単一のOptionへ集約しません。

## EitherT

`EitherT<L, M, A>` は `M<Either<L, A>>` を保持します。`Right` が mapped payload、
`L` と `M` が固定された captured 型です。

| 関数 | 型 | 意味 |
|---|---|---|
| `EitherT::new` | `M<Either<L, A>> -> EitherT<L, M, A>` | representation を包む |
| `EitherT::run` | `EitherT<L, M, A> -> M<Either<L, A>>` | representation を取り出す |
| `EitherT::left` | `L -> EitherT<L, M, A>` | base の `pure` で `Left` を包む |
| `EitherT::right` | `A -> EitherT<L, M, A>` | base の `pure` で `Right` を包む |
| `EitherT::from_either` | `Either<L, A> -> EitherT<L, M, A>` | Either を base の `pure` で持ち上げる |
| `EitherT::map_left` | `(EitherT<L, M, A>, L -> B) -> EitherT<B, M, A>` | Left payload だけを変換する |
| `EitherT::map_right` | `(EitherT<L, M, A>, A -> B) -> EitherT<L, M, B>` | Right payload だけを変換する |
| `EitherT::bimap` | `(EitherT<L, M, A>, L -> B, A -> C) -> EitherT<B, M, C>` | 両 payload を対応する関数で変換する |

`EitherT` は `Functor`、`Applicative`、`Monad`、`MonadT<M>` を実装します。
`Functor::fmap` は `EitherT::map_right` と同じ Right mapping です。
標準の `Alternative` はありません。既定の `Left` 値を暗黙に生成しないためです。

## ReaderT

`ReaderT<R, M, A>` は `R -> M<A>` を保持します。同じ環境を前段と後段へ渡します。

| 関数 | 型 | 意味 |
|---|---|---|
| `ReaderT::new` | `(R -> M<A>) -> ReaderT<R, M, A>` | 環境関数を保持する |
| `ReaderT::run` | `(ReaderT<R, M, A>, R) -> M<A>` | 環境を明示して実行する |
| `ReaderT::ask` | `() -> ReaderT<R, M, R>` | 環境全体を読む |
| `ReaderT::asks` | `(R -> A) -> ReaderT<R, M, A>` | 環境の射影を読む |
| `ReaderT::local` | `(ReaderT<R, M, A>, (R -> R)) -> ReaderT<R, M, A>` | 一つの計算が見る環境だけを変える |

base が `Monad + Alternative` のときだけ `ReaderT` も `Alternative` を実装します。

## StateT

`StateT<S, M, A>` は `S -> M<(A, S)>` を保持します。`ap` と `bind` は前段が
base 内で返した次状態を後段へ渡します。

| 関数 | 型 | 意味 |
|---|---|---|
| `StateT::new` | `(S -> M<(A, S)>) -> StateT<S, M, A>` | 状態遷移を保持する |
| `StateT::run` | `(StateT<S, M, A>, S) -> M<(A, S)>` | 値と次状態を返す |
| `StateT::eval` | `(StateT<S, M, A>, S) -> M<A>` | 一度実行し、値だけを射影する |
| `StateT::exec` | `(StateT<S, M, A>, S) -> M<S>` | 一度実行し、次状態だけを射影する |
| `StateT::get` | `() -> StateT<S, M, S>` | 現在の状態を読む |
| `StateT::gets` | `(S -> A) -> StateT<S, M, A>` | 状態の射影を読む |
| `StateT::put` | `S -> StateT<S, M, Unit>` | 状態を置き換える |
| `StateT::modify` | `(S -> S) -> StateT<S, M, Unit>` | 状態を関数で更新する |

base が `Monad + Alternative` のときだけ `StateT` も `Alternative` を実装します。
両 branch には同じ初期状態を渡し、base の `choose` に選択を委ねます。

### Alternativeと評価

`ReaderT::choose` / `StateT::choose`は、生成したTransformerを`run`したときに両branchの通常計算を
評価してからbaseの`Alternative::choose`へ渡します。`StateT`は両branchへ同じ初期状態を渡しますが、
片方で起きた外部作用をrollbackする契約ではありません。

`OptionT`の`None`やbase failureによる内部短絡は、Transformerが保持するcomputation内部の規則です。
`Alternative::choose(left_expression, right_expression)`など通常の関数呼び出しの引数式はeagerに評価され、
Transformerの短絡が引数式の評価を取り消したり遅延化したりすることはありません。

## Field と REPL

Transformerは通常のstruct値として扱います。例えば `OptionT` の公開representationはFacetで明示的に操作できます。

```surtr
x: OptionT<Result, Int> = OptionT::new(Ok(Option::Some(1)))
raw: Result<Option<Int>> = Facet::view(OptionT.inner, x)
updated = Facet::put(OptionT.inner, x, Ok(Option::Some(2)))
```

REPLでも `new`、`lift`、`fmap`、`run`を別々の通常入力として評価できます。各入力では型注釈または引数からcarrierを具体化してください。
Transformer自体もMonad carrierとして`do`で逐次処理できます。base carrierの値は自動liftされないため、必要な場合は`MonadT::lift`を明示します。
Result effect の適用は do-local carrier ごとに決まり、外側の do や関数から継承しません。

```surtr
result: EitherT<String, Identity, Int> = do::<EitherT<String, Identity, _>> {
  value <- EitherT::right::<String, Identity>(20)
  EitherT::right::<String, Identity>(value + 1)
}
```

## 境界

- `map_t`、`map_inner` は標準 API にありません。
- `from_left`、`from_right`、`lift_either`、`lift_inner` の別名は追加しません。
- `IdentityT`、`ResultT`、Transformer 自体を引数に取る抽象 API はありません。
- `lift` は通常の関数呼び出しであり、base operation の評価を遅延化しません。
