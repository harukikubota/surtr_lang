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
- 標準定義ソース内での位置づけは `./standard-modules.md`
- 制約一覧は `./language-reference.md`

## 確認したソース

- ソース
  - `../../lib/traits/from.srt`
  - `../../lib/traits/try_from.srt`
  - `../../lib/types/int.srt`

## 躓きやすいポイント

- `impl Trait` は parameter 位置だけで、`-> impl Trait` はまだ使えません。
- `where` clause と multi-trait bound はまだ使えません。
- `+`, `-`, `*` は `Add` / `Sub` / `Mul` の dispatch です。
- `From` / `TryFrom` の呼び出し surface は簡潔でも、coherence 自体は trait 実装側で管理されています。
