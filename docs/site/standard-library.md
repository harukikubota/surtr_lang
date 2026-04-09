# Surtr Standard Library Layout

このページは、Surtr の標準モジュール構成を利用者向けにまとめたものです。

標準モジュールは単なる補助ファイルではなく、language surface の一部です。  
`lib/*.srt` に書かれた `@@doc` は source 上の説明であり、将来的には `.eldr` の `Docs` chunk からも参照できる前提で扱います。

## 1. ロード順

標準モジュールの初期ロード順は次で固定されています。

```text
Bootstrap -> [Kernel, Int, String, Boolean, Error, List, Result, Float] -> user source
```

このうち auto import されるのは `Bootstrap` と `Kernel` だけです。  
他の標準モジュールは標準モジュールとして同梱されますが、名前空間としては明示 import 前提です。

## 2. 各モジュールの役割

### `Bootstrap`

- auto import の起点になる安定アンカー
- loader が最初に読む固定ステージ
- 標準 concrete error の置き場

`Bootstrap` は「何かでもかんでも置く場所」ではありません。  
将来 bootstrap 手順が増えても、入口の module 名と順序を固定するために残しています。
そのうえで、`NoneError` や `ZeroDivisionError` のような universally useful な
concrete error は、最初の標準ステージから使えるようここに置きます。

### `Kernel`

- `defmod Kernel` の中に `if`, `if_then`, `print`, `to_string`, `inspect`, `eprint`, `set_exit_code` のような cross-cutting builtin を置く
- auto import される最小の標準 API を置く
- 専用 file を持たない `Unit` の builtin type 宣言を置く

primitive type に強く結びつかない builtin は、ここへ集めます。
特に `if` / `if_then` は言語特性に近い special form ですが、source 上の契約と
説明を標準 surface に残すため `Kernel` に置きます。

### type modules

現時点では次の module が用意されています。

- `Int`
- `String`
- `Boolean`
- `Error`
- `List`
- `Result`
- `Float`

各 type module には 2 つの層があります。

1. file top-level の `@@builtin type ...`
2. `defmod Name { ... }` の module API

この分離により、「型そのものの compiler 契約」と「その型の helper / docs / 将来 API」を同じ file に置きつつ、役割は混ぜずに管理できます。

## 3. `@@builtin type` の契約

標準型宣言は、各対応 file のトップレベルで canonical shape を宣言します。

```surtr
// kernel.srt
@@builtin type Unit

// int.srt
@@builtin type Int

// list.srt
@@builtin type List<$A>

// result.srt
@@builtin type Result<$T>
```

compiler はこの head 自体を契約として扱います。  
そのため、標準モジュール側で name や generic parameter が変わっていると compile error になります。

特に次は重要です。

- `List` は `List<$A>`
- `Result` は `Result<$T>`

`Result<T, E>` は builtin type declaration ではなく、戻り値位置での error contract 記法として扱います。

## 4. `Error` と `Result` の読み方

`Error` は具体 error の列挙ではなく、recoverable failure を受ける抽象型です。

- `deferror Boom { ... }` のような宣言が具体 error を作る
- `Err(Boom)` のように `Result` の失敗側へ乗る
- `Error` 自体を new するのではなく、具体 error を経由して使う

`Result` は例外の代用品ではなく、`Either` 指向の値表現として読むと分かりやすくなります。

- `Ok(value)` が成功側
- `Err(error)` が失敗側
- `=?` は `Err` 側をそのまま伝播する糖衣構文

そのため、Surtr の error handling は「失敗も値として型に乗せる」ことが前提です。

`Result` の内部表現は enum-like な tagged value ですが、言語仕様では将来の一般 `Enum` 機能と同一視しません。  
利用者が見る surface contract は、専用の builtin type と専用 constructor contract です。

```surtr
@@builtin type Ok($T) -> Result<$T>
@@builtin type Err(Error) -> Result<$T>
```

この 2 行は通常の関数本体付き `def` ではなく、compiler が特別扱いする declaration-only contract です。

## 5. `@@doc` の使い方

標準モジュールの説明は `@@doc """..."""` で source に直接載せます。

```surtr
@@doc """
Standard `String` type declaration.
Text values produced by literals, interpolation, and textual conversion use this
head.
"""
@@builtin type String

@@doc """
String module.
Groups string-oriented helpers.
"""
defmod String {
  @@doc """
  Placeholder while the module API grows.
  """
  def dummy() { () }
}
```

`@@doc` を source に置く利点は次のとおりです。

- 標準モジュールと説明文がずれにくい
- dump や REPL の docs UI に同じ情報を流せる
- Rust 実装ではなく Surtr surface として API を説明できる

## 6. いま読むときの目印

- `Bootstrap`
  - auto import の固定起点と bootstrap error 群
- `Kernel`
  - cross-cutting builtin と `Unit`
  - `if` / `if_then` の language-level contract
- `Int`
  - arbitrary-precision integer surface
- `String`
  - 文字列処理の入口
- `Boolean`
  - 条件分岐の基本型
- `Error`
  - recoverable failure 側の値を受ける抽象型
- `List`
  - homogeneous sequence 型
- `Result`
  - `Ok` / `Err` を中心にした Either 指向の失敗表現
- `Float`
  - 実装はあるが契約整理を継続中の型

## 7. 更新するときのルール

- cross-cutting builtin value を足すときは `kernel.srt` の `defmod Kernel` と shared builtin metadata の両方を更新する
- builtin type を変えるときは、対応する `lib/*.srt` の `@@builtin type` と compiler 側の canonical contract を同時に更新する
- `Result` constructor contract を変えるときは `result.srt` の `Ok` / `Err` 宣言と checker 側の canonical rule を同時に更新する
- module API を足すときは `defmod Name` に実装し、まず `@@doc` を先に書く
