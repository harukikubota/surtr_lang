# `Either<$L, $R>` 仕様案（ドラフト）

**Status:** Draft
**作成日:** 2026-08-20

## 1. 目的

`Either<$L, $R>` を、二つの任意の値のいずれか一方を表す標準 enum として追加する。
`Either` は `Result` の別名でも汎用化でもない。特に、`Left` は `Error` に制約されず、
失敗伝播のための特別な意味を持たない。

`Either` は `Option` と同じ層の user-facing な標準コンテナとする。`Option` と同様に
通常の `defenum` と標準定義 source 上の関数・trait impl だけで表現し、compiler / VM の
builtin-special enum、専用 opcode、専用 runtime representation を追加しない。

## 2. 非目標と `Result` との境界

| 観点 | `Either<$L, $R>` | `Result<$T>` |
| --- | --- | --- |
| 左／失敗側の payload | 任意の `$L` | 常に `Error` |
| 正常側の payload | `$R` | `$T` |
| `=?` | 対象外 | `Ok` を取り出し、`Err` を伝播する |
| bare constructor sugar | なし。常に `Either::Left` / `Either::Right` | `Ok` / `Err` を許可 |
| error chain / recovery | 提供しない | `cause` / `recover` / `recover_kind` など |
| runtime | 通常 enum | builtin-special enum |

したがって `Either::Left(error)` を `=?` で伝播してはならない。また、`Either` と
`Result` 間の暗黙変換、`From` 実装、`Ok` / `Err` への alias はこの提案に含めない。
必要なら利用者が `match` で明示変換する。

## 3. 型と constructor surface

標準定義 source は `lib/types/either.srt` とする。

```surtr
@doc """
One of two independently typed values.
`Either` is a general choice value, not Result-style error control flow.
"""
defenum Either<$L, $R> {
  Left($L),
  Right($R),
}
```

constructor の正規形は `Either::Left(value)` と `Either::Right(value)` である。通常 enum
の namespace 規則に従い、bare `Left` / `Right` は導入しない。

## 4. `Either` module API

関数 surface は `impl Either` に置く。ここでの「右」は成功、「左」は失敗を意味しない。
右を主値側に選ぶのは、既存の `Functor` 系演算子との一貫性のためだけである。

| API | 型 | 契約 |
| --- | --- | --- |
| `is_left` | `Either<$L, $R> -> Boolean` | `Left(_)` のときだけ `True` |
| `is_right` | `Either<$L, $R> -> Boolean` | `Right(_)` のときだけ `True` |
| `wrap` | `$R -> Either<$L, $R>` | 値を `Right(value)` に包む |
| `swap` | `Either<$L, $R> -> Either<$R, $L>` | `Left(l) -> Right(l)`、`Right(r) -> Left(r)` |
| `fold` | `Either<$L, $R>, ($L -> $A), ($R -> $A) -> $A` | variant に応じて対応する handler を一度だけ呼ぶ |
| `map_left` | `Either<$L, $R>, ($L -> $A) -> Either<$A, $R>` | 左 payload だけを変換し、`Right` は不変 |

`Right` 側の変換は duplicate API を増やさず、`Functor::fmap` または `|*>` を使う。

```surtr
Either::Right(2) |*> {|n| n + 1}       # Either::Right(3)
Either::map_left(Either::Left("no"), String::upcase)
# Either::Left("NO")
```

`Option` と同等レベルの最小 surface として、variant 判定と主値側への `wrap` を必須にする。
`unwrap`、例外送出、暗黙 fallback、`Result` 固有の `map_err` / `cause` / `recover` は追加しない。

## 5. `FunctorFamily` と右部分適用

この文書での **FunctorFamily** は、型コンストラクタ trait の
`Functor`、`Applicative`、`Monad`、`Alternative` の総称である。新しい
`FunctorFamily` trait を宣言するわけではない。

二引数型の `Either<$L, $R>` は、左スロットを固定して `Either<$L, _>` を unary type
constructor として扱う。以後、各 trait の `Self` はこの部分適用 family を指す。

```text
Either<$L, _>
  fmap / |*>  : $R -> $B        => Either<$L, $B>
  pure        : $A              => Either<$L, $A>
  ap / |*|    : Either<$L, A->B>, Either<$L, A> => Either<$L, B>
  bind / |>=  : $R -> Either<$L, $B> => Either<$L, $B>
  empty / <|> : Either<$L, $A>
```

`fmap`、`ap`、`bind`、`<|>` の一連の式で `$L` は不変でなければならない。たとえば
`Either<String, Int>` の bind mapper が `Either<Int, String>` を返すことは family switch
として typecheck error とする。

この部分適用は既存の `Self: Type<$A>` 制約に対する必要な拡張である。trait dispatch は
`Either<$L, _>` を unary family と認識し、固定済みの `$L` を impl の入力・出力で同一に
unify する。`Either<$L, $R>` 全体を unary constructor と見なしたり、二つの slot を
暗黙に入れ替えたりしてはならない。

## 6. FunctorFamily の各実装

### 6.1 `Functor`

```surtr
impl Functor for Either<$L, $T> {
  def fmap(self: Either<$L, $A>, mapper: ($A -> $B)) -> Either<$L, $B> {
    match self {
      Either::Left(left) => Either::Left(left),
      Either::Right(right) => Either::Right(mapper(right)),
    }
  }
}
```

`Left` は mapper を評価せず、その payload を保持する。`Right` だけが mapper を一度評価する。

### 6.2 `Applicative`

```surtr
impl Applicative for Either<$L, $T> {
  def pure::<Either<$L, $T>>(value: $A) -> Either<$L, $A> {
    Either::Right(value)
  }

  def ap(
    mapper: Either<$L, ($A -> $B)>,
    value: Either<$L, $A>,
  ) -> Either<$L, $B> {
    match mapper {
      Either::Left(left) => Either::Left(left),
      Either::Right(f) => match value {
        Either::Left(left) => Either::Left(left),
        Either::Right(inner) => Either::Right(f(inner)),
      },
    }
  }
}
```

両方が `Left` のときは mapper 側（左引数）の payload を返す。payload を結合しないため、
`$L` に `Monoid` 等の追加制約は設けない。

### 6.3 `Monad`

```surtr
impl Monad for Either<$L, $T> {
  def return::<Either<$L, $T>>(value: $A) -> Either<$L, $A> {
    Either::Right(value)
  }

  def bind(
    self: Either<$L, $A>,
    mapper: ($A -> Either<$L, $B>),
  ) -> Either<$L, $B> {
    match self {
      Either::Left(left) => Either::Left(left),
      Either::Right(right) => mapper(right),
    }
  }
}
```

`Left` は mapper を評価せずに残し、`Right` のみ mapper に渡す。これは値の選択的な連鎖であり、
`Result` の `=?` によるエラー伝播ではない。

### 6.4 `Alternative`

既存 `Alternative::empty` は要素型だけから空の文脈値を構築する。そのため、任意の `$L` を
持つ `Either` に無条件の `empty` は存在しない。本提案では `$L: Default` を明示的な制約にし、
左の既定値を空値に使う。

```surtr
impl Alternative for Either<$L, $T>
where
  $L: Default
{
  def empty::<Either<$L, $T>, $A>() -> Either<$L, $A> {
    Either::Left(default::<$L>())
  }

  def choose(left: Either<$L, $A>, right: Either<$L, $A>) -> Either<$L, $A> {
    match left {
      Either::Right(_) => left,
      Either::Left(_) => right,
    }
  }
}
```

`<|>` は最初の `Right` を選び、左が `Left` なら右を返す。`Left` payload は蓄積・結合せず、
両方が `Left` なら右側の payload が残る。

`Default` を実装しない左型では `Functor` / `Applicative` / `Monad` は使えるが、
`Alternative` と `<|>` は dispatch できない。これは `Left` に架空の値を生成せず、既存
`Alternative` 契約を保つための意図的な制約である。

## 7. 実装範囲と確認項目

実装時は少なくとも次を更新する。

1. `lib/types/either.srt` に enum、inherent API、FunctorFamily の各 impl を追加する。
2. 標準定義 source の preload 一覧に `either.srt` を追加する。
3. 型コンストラクタ trait dispatch に `Either<$L, _>` の部分適用 family を追加する。
4. `lib/tests/either.srt` と必要な script fixture で constructor、`match`、各 API、`|*>`、`|*|`、`|>=`、`<|>` を検証する。
5. 左型が不一致の bind / applicative を type error にし、`Default` のない左型への `Alternative` dispatch failure を固定する。

この変更では `Result` の builtin metadata、runtime tag、`=?` lowering、`Error`、および
bare constructor resolver は変更しない。

## 8. 受け入れ例

```surtr
value: Either<String, Int> = Either::Right(2)
mapped = value |*> {|n| n + 1}
# Either::Right(3)

stopped: Either<String, Int> = Either::Left("missing")
unchanged = stopped |>= {|n| Either::Right(n + 1)}
# Either::Left("missing")

fallback: Either<String, Int> = Either::Left("first") <|> Either::Right(10)
# Either::Right(10)

text = Either::fold(
  Either::Left("offline"),
  {|reason| "left: " ++ reason},
  {|count| "right: " ++ to_string(count)},
)
# "left: offline"
```

`Either<Int, Int>` のように両側が同じ型でも、variant は型で消えない。利用者は `match`、
`fold`、または `is_left` / `is_right` で意味を明示する。
