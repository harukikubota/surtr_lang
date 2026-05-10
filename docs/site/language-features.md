# Language Features

ここでは `import`, `include`, `@autoimport` をまとめます。

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
- `import` は file declaration area に加えて `defmod` / `impl Type` / `impl Trait for Type` body に書ける
- `def` / `defp` / `defextractor` / closure / top-level expr の中では使えない

## `include`

`include` は file を現在 compile unit へ取り込みます。

```surtr
include "./src/helper.srt"
```

- 文字列リテラル path だけを受け付ける
- file-based composition 用
- script source の先頭に連続して置く
- include 先の file は definition source として読む
- REPL top-level ではなく source file 側で使う前提
- `surtr repl --script file.srt` でも同じ規則で `include` を解決し、script を一度実行してから対話を始める

process examples でも、entry script から `include "./Agents.srt"` や `include "./Workers.srt"` の形で定義を読み込むのが基本です。全体像は `./process.md` にまとめています。

## `@autoimport`

`@autoimport` は標準 surface のうち「最初から見えてよい宣言」に付ける属性です。

標準ライブラリでは次が代表です。

- `../../lib/kernel.srt` の `defmod Kernel`
- `../../lib/trait/eq.srt`, `concat.srt`, `from.srt`, `try_from.srt` などの trait 宣言

module と trait では意味合いが少し違います。

- `@autoimport defmod`
  - module member を最初から見える standard surface に入れる
- `@autoimport deftrait`
  - trait method helper alias を unqualified で使えるようにする

trait 側で重要なのは、autoimport される helper が「別の関数定義」ではないことです。  
たとえば `concat(left, right)` は `Concat::concat(left, right)` への alias として解決されます。  
そのため、実装の有無・型検査・diagnostic の canonical な基準は常に trait 側にあります。

```surtr
print(concat("a", "b"))
print(from(42, String))
print(inspect(try_from("42", Int)))
```

一方で、すべての trait helper が autoimport されるわけではありません。  
`Add`, `Sub`, `Mul` のように「演算子の別表記」が主目的のものは、qualified call か明示 import を使います。

```surtr
print(to_string(Add::add(1, 2)))

import Add::add
print(to_string(add(3, 4)))
```

利用者目線では、次の覚え方で十分です。

- `print`, `if`, `inspect` のような cross-cutting API は `Kernel` 由来
- `eq`, `concat`, `from`, `try_from`, `to_string` のような頻出 helper は autoimport trait alias
- `Add::add`, `Sub::sub`, `Mul::mul` のような helper は qualified/import 前提

## どこで確認するか

- surface の正本: `../../doc/要件定義v9.md`
- 標準定義ソースの説明: `./standard-modules.md`
- 実例: `../../tests/spec/modules/`

## 確認したソース

- ソース
  - `../../lib/bootstrap.srt`
  - `../../lib/kernel.srt`
  - `../../lib/trait/eq.srt`
  - `../../lib/trait/concat.srt`
  - `../../lib/trait/from.srt`
  - `../../lib/trait/try_from.srt`
  - `../../lib/trait/add.srt`
  - `../../lib/trait/sub.srt`
  - `../../lib/trait/mul.srt`

## 躓きやすいポイント

- `Kernel` は auto import 済みなので、明示 `import Kernel` はむしろ compile error です。
- `concat(...)` や `from(...)` が裸で呼べても、実体は `Trait::method` 側です。
- `add(...)`, `sub(...)`, `mul(...)` は最初からは見えません。必要なら `Add::add(...)` のように呼びます。
- `include` は file composition 用で、名前空間 import の代わりではありません。
