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
  - `Numeric`, `Show`, `Eq`, `Compare`, `Ord`, `Concat`, `From`, `TryFrom`
- type modules
  - `Int`, `String`, `Boolean`, `Error`, `List`, `Result`, `Lens`, `Float`

## どこを見るか

- 条件分岐や出力: `../../lib/kernel.srt`
- 数値演算の契約: `../../lib/trait/numeric.srt`
- 変換: `../../lib/trait/from.srt`, `../../lib/trait/try_from.srt`
- 型ごとの helper: `../../lib/int.srt`, `../../lib/string.srt`, `../../lib/list.srt` など
- Lens path: `../../lib/lens.srt`

## auto import されるもの

auto import されるのは `Bootstrap` と `Kernel` だけです。  
それ以外の標準モジュールは同梱されますが、名前空間としては明示 import 前提です。

## REPL で見える最小例

```text
xldr(1)> print("hello")
hello
xldr(2)>
```

`print` が import なしで使えるのは `Kernel` が auto import されるためです。

## 次に読むページ

- `Kernel` を先に触りたいなら `./kernel.md`
- trait 系を見たいなら `./trait-impls.md`
- path 操作を見たいなら `./lens.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`
  - `../../lib/lens.srt`
  - `../../lib/trait/numeric.srt`

## 躓きやすいポイント

- auto import されるのは `Bootstrap` と `Kernel` だけで、他の標準モジュールは明示 `import` 前提です。
- `Lens` は標準モジュールに見えても runtime value ではなく、compile-time only capability です。
