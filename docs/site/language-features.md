# Language Features

ここでは `import`, `include`, `@@autoimport` をまとめます。

## `import`

`import` は module から import 可能 member を現在 scope へ入れます。

```surtr
import Math
import Math::add
```

読み方は次の通りです。

- `import Mod`
  - import 可能 member を unqualified で入れる
- `import Mod::name`
  - 単一 member だけ入れる

注意点:

- `Bootstrap` / `Kernel` の明示 import は compile error
- 同一 file での重複 import は compile error
- `Type::new` のように import 対象外の宣言がある

## `include`

`include` は file を現在 compile unit へ取り込みます。

```surtr
include "./src/helper.srt"
```

- 文字列リテラル path だけを受け付ける
- file-based composition 用
- REPL top-level ではなく source file 側で使う前提

## `@@autoimport`

`@@autoimport` は標準 surface のうち「最初から見えてよい宣言」に付ける属性です。

標準ライブラリでは次が代表です。

- `../../lib/kernel.srt` の `defmod Kernel`
- `../../lib/trait/numeric.srt` などの trait 宣言

ただし、利用者が最初から unqualified に触れる前提として固定しているのは `Bootstrap` と `Kernel` です。

## どこで確認するか

- surface の正本: `../../doc/要件定義v9.md`
- 標準モジュールの説明: `./standard-modules.md`
- 実例: `../../tests/spec/modules/`

## 確認したソース

- ソース
  - `../../lib/bootstrap.srt`
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- `Kernel` は auto import 済みなので、明示 `import Kernel` はむしろ compile error です。
- `include` は file composition 用で、名前空間 import の代わりではありません。
