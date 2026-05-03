# Standard Modules

Surtr の標準モジュールは language surface の一部です。

ロード順の全体像と設計背景は `./standard-library.md` にまとめています。  
ここでは「普段どこを開けばよいか」を先に整理します。

## まず覚える層

- `Bootstrap`
  - `import` / `include` の canonical anchor
  - 起動時に最初に読まれる固定ステージ
- `Kernel`
  - `print`, `inspect`, `if`, `assert`, `ensure` などの cross-cutting API
  - auto import される最小の標準 API
- trait modules
  - capability: `Numeric`, `Show`, `Compare`, `From`, `TryFrom`
  - operator dispatch / compatibility: `Eq`, `Ord`, `Concat` など
- type modules
  - `Int`, `String`, `Regex`, `Boolean`, `Error`, `List`, `Result`, `Option`, `HashMap`, `Lens`, `Float`
- effect / runtime-facing modules
  - `Process`, `IO`, `Task`, `Random`

## どこを見るか

- 条件分岐や出力: `../../lib/kernel.srt`
- 数値演算の契約: `../../lib/traits/numeric.srt`
- 変換: `../../lib/traits/from.srt`, `../../lib/traits/try_from.srt`
- 型ごとの helper: `../../lib/types/int.srt`, `../../lib/types/string.srt`, `../../lib/types/list.srt` など
- 正規表現: `../../lib/types/regex.srt`
- Lens path: `../../lib/lens.srt`

## auto import されるもの

auto import されるのは `Bootstrap`, `Kernel`, `Result` と、`@@autoimport` が付いた標準 trait です。  
それ以外の標準モジュールは同梱されますが、名前空間としては明示 import 前提です。

## REPL で見える最小例

```text
xldr(1)> print("hello")
hello
xldr(2)>
```

`print` が import なしで使えるのは `Kernel` が auto import されるためです。`Ok` / `Err` が bare 名で使えるのは `Result` が auto import されるためです。

## 次に読むページ

- `Kernel` を先に触りたいなら `./kernel.md`
- `Regex` を触りたいなら `./regex.md`
- trait 系を見たいなら `./trait-impls.md`
- path 操作を見たいなら `./lens.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`
  - `../../lib/lens.srt`
  - `../../lib/traits/numeric.srt`

## 躓きやすいポイント

- auto import されるのは `Bootstrap`, `Kernel`, `Result` と `@@autoimport` 付き標準 trait だけで、他の標準モジュールは明示 `import` 前提です。
- `Lens` は標準モジュールに見えても runtime value ではなく、compile-time only capability です。
- `Ord` は互換 helper で、新しい比較 API の正本は `Compare` です。
- REPL は OnceRead universe なので、読み込み後に trait universe を増分更新する前提ではありません。
