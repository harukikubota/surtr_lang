# Trait Impls

Surtr の trait system は V1 です。  
公開 surface では `deftrait` と `impl Trait for Type` を使います。

## 分類

標準 trait は大きく 3 層に分けて読むと分かりやすいです。

- capability trait
  - `Show`, `Compare`, `From`, `TryFrom`
- operator dispatch trait
  - `Add`, `Sub`, `Mul`, `Eq`, `Neq`, `Concat`
  - `Functor`, `Applicative`, `Monad`, `PipeApply`, `Compose`, `Composable`, `LiftComposable`, `KleisliComposable`

`Compare` は新しい API が三値比較を要求するときの正本です。`< <= > >=` も公開 surface では `Compare` によって意味づけられます。  
数値 helper は generic trait ではなく、`Int::abs` / `Float::safe_div` のような concrete type owner surface として提供します。

## 宣言側

trait と impl は file-oriented に書きます。

```surtr
deftrait Describable {
  def describe(self: Self) -> String
}

impl Describable for Int {
  def describe(self: Self) -> String { to_string(self) }
}
```

正本の標準 trait 宣言は次を参照してください。

- `../../lib/traits/from.srt`
- `../../lib/traits/try_from.srt`

## 呼び出し側

利用者がまず触るのは helper surface です。

```text
xldr(1)> print(to_string(Int::abs(-4)))
4
xldr(2)>
```

### `Functor`, `Applicative`, `Monad`

この3つは文脈付き計算を段階的に扱う標準 trait です。

- `Functor::fmap` / `|*>` は文脈の中身だけを変換します。
- `Applicative::pure` は値を文脈へ持ち上げ、`Applicative::ap` / `|*|` は文脈内の callable と文脈内の値を組み合わせます。
- `Monad::return` は値を文脈へ持ち上げ、`Monad::bind` / `|>=` は文脈内の値を次の文脈付き計算へ渡します。

これら3 trait は auto import 対象です。そのため、qualified call の代わりに
`fmap(...)`, `pure(...)`, `ap(...)`, `return(...)`, `bind(...)` と書けます。
演算子は文脈の流れを読みやすくし、helper の直接呼び出しは flat な式や高階関数で
便利です。

複数引数の mapper は `curry()` で明示的にカリー化します。

```surtr
Ok(curry(&Add::add)) |*| Ok(1) |*| Ok(2) # => Ok(3)

value: Result<Int> = pure(1)
mapped: Result<Int> = fmap(value, {|n| n + 1})
```

変換系は `From` / `TryFrom` trait が裏側の coherence を担います。

```text
xldr(1)> print(match try_from::<Int>("42") { Ok(value) => to_string(value), Err(err) => inspect(err), })
42
xldr(2)>
```

## 読み方

- `deftrait Name { ... }`
  - method signature だけを持つ契約 namespace
- `impl Trait for Type { ... }`
  - concrete 実装
- `impl Trait<Concrete> for Type { ... }`
  - target type つき trait 実装

型注釈と明示型引数のルールは `./type-annotations.md` にまとめています。

## 関連ページ

- 変換の呼び出し surface は `./definitions-and-usage.md`
- 型注釈は `./type-annotations.md`
- 標準定義ソース内での位置づけは `./standard-modules.md`
- 制約一覧は `./language-reference.md`

## 確認したソース

- ソース
  - `../../lib/traits/from.srt`
  - `../../lib/traits/try_from.srt`
  - `../../lib/types/int.srt`

## 躓きやすいポイント

- 匿名 `impl Trait` 型は使わず、名前付き型変数と `where` clause で制約します。
- `where` clause と multi-trait bound は signature slot の制約として使えます。
- `+`, `-`, `*` は `Add` / `Sub` / `Mul` の dispatch です。
- `|*>`, `|*|`, `|>=` はそれぞれ `Functor::fmap`, `Applicative::ap`, `Monad::bind` の dispatch です。
- `|*|` は未カリー化 callable を暗黙変換しません。複数引数では `curry()` を明示します。
- `From` / `TryFrom` の呼び出し surface は簡潔でも、coherence 自体は trait 実装側で管理されています。
