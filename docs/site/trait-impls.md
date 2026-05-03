# Trait Impls

Surtr の trait system は V1 です。  
公開 surface では `deftrait` と `impl Trait for Type` を使います。

## 分類

標準 trait は大きく 3 層に分けて読むと分かりやすいです。

- capability trait
  - `Show`, `Compare`, `From`, `TryFrom`, `Numeric`
- operator dispatch trait
  - `Add`, `Sub`, `Mul`, `Eq`, `Neq`, `Lt`, `Lte`, `Gt`, `Gte`, `Concat`
  - `Functor`, `Chainable`, `PipeApply`, `Compose`, `Composable`, `LiftComposable`, `KleisliComposable`
- compatibility trait
  - `Ord`

`Compare` は新しい API が三値比較を要求するときの正本です。`Ord` は grouped Boolean helper としての互換層に留まります。  
`Numeric` は演算子親ではなく、`safe_div` / `abs` / `min` / `max` のような generic helper 用 capability です。

## 宣言側

trait と impl は file-oriented に書きます。

```surtr
deftrait Numeric {
  def abs(self: Self) -> Self
  def min(self: Self, rhs: Self) -> Self
}

impl Numeric for Int {
  def abs(self: Self) -> Self { if(self < 0, 0 - self, self) }
  def min(self: Self, rhs: Self) -> Self { if(self < rhs, self, rhs) }
}
```

正本の標準 trait 宣言は次を参照してください。

- `../../lib/traits/numeric.srt`
- `../../lib/traits/from.srt`
- `../../lib/traits/try_from.srt`

## 呼び出し側

利用者がまず触るのは helper surface です。

```text
xldr(1)> print(to_string(Numeric::abs(-4)))
4
xldr(2)>
```

変換系は `From` / `TryFrom` trait が裏側の coherence を担います。

```text
xldr(1)> print(match try_from("42", Int) { Ok(value) => to_string(value), Err(err) => inspect(err), })
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

`TypeRef<$T>` を含む型注釈ルールは `./type-annotations.md` にまとめています。

## 関連ページ

- 変換の呼び出し surface は `./definitions-and-usage.md`
- 型注釈は `./type-annotations.md`
- 標準モジュール内での位置づけは `./standard-modules.md`
- 制約一覧は `./language-reference.md`

## 確認したソース

- ソース
  - `../../lib/traits/numeric.srt`
  - `../../lib/traits/from.srt`
  - `../../lib/traits/try_from.srt`
  - `../../lib/types/int.srt`

## 躓きやすいポイント

- `impl Trait` は parameter 位置だけで、`-> impl Trait` はまだ使えません。
- `where` clause と multi-trait bound はまだ使えません。
- `+`, `-`, `*` は `Numeric` ではなく `Add` / `Sub` / `Mul` の dispatch です。
- `From` / `TryFrom` の呼び出し surface は簡潔でも、coherence 自体は trait 実装側で管理されています。
