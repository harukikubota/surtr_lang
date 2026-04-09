# Surtr Language Guide

## 1. Surtr とは

Surtr は、Rust で実装している静的型付き関数型言語です。

目標は次の 3 つです。

- コンパイラとランタイムをできるだけシンプルに保つ
- 言語機能を増やしすぎず、構文の表現力で書きやすさを出す
- 失敗を型で表現し、曖昧な挙動を減らす

現時点では、言語の土台になるコア機能を優先して実装しています。

## 2. Hello, Surtr

```surtr
print("hello, surtr")
```

関数を定義するなら、次の形です。

```surtr
def add1(x: Int) -> Int { x + 1 }
print(to_string(add1(41)))
```

## 3. 値と型

Surtr で現在使える基本型は次のとおりです。

- `Int`
- `Float`
- `String`
- `Boolean`
- `Unit`

基本的な束縛は次の形です。

```surtr
name = "alice"
score: Int = 10
ok = True
```

`print` は文字列を表示します。数値や複合値を表示するときは、まず `to_string(...)` を通します。

```surtr
score = 10
print(to_string(score))
```

## 4. 文字列

文字列は二重引用符で書きます。

```surtr
name = "alice"
print("hello #{name}")
print("score=#{10 + 2}")
```

文字列結合は `++` です。

```surtr
print("hello" ++ " world")
```

## 5. 関数

関数定義は `def` を使います。

```surtr
def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(1, 2)))
```

名前付き引数も使えます。

```surtr
def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(y: 2, x: 1)))
```

現時点で公開前提の基本として押さえるとよい点は次のとおりです。

- 引数には型を書く
- 戻り値型を書く
- 関数本体は式として評価される
- 前方参照は許可される。後で同じコンパイル単位に定義が現れればよい

## 6. 条件分岐とパターンマッチ

Surtr では `if` と `match` が重要です。

### `if`

```surtr
flag = True
greeting = if(flag, "hello", "goodbye")
print(greeting)
```

`if` は値を返す分岐です。内部契約としては
`if(Boolean, (-> A), (-> A)) -> A`
のように branch が関数型で表されますが、通常の source では明示的な block を
書く必要はありません。

```surtr
message = if(flag, "ok", "retry")
print(message)
```

この関数型表記は「選ばれた branch だけが評価される」という言語特性を説明する
ためのものです。

`if_then` は条件付きで `Unit` を返す用途です。こちらも宣言上は
`if_then(Boolean, (-> Unit)) -> Unit`
ですが、普段はそのまま式を書けます。

```surtr
if_then(flag, print("flag is true"))
```

### `match`

```surtr
flag = True
print(to_string(match flag {
  True  => "yes",
  False => "no",
}))
```

`match` は Result に対してもよく使います。

```surtr
result: Result<Int> = Ok(42)
match result {
  Ok(val) => print(to_string(val)),
  Err(e)  => print("error"),
}
```

## 7. 構造体とレコード

Surtr には `defstruct` と `defrecord` があります。

### `defstruct`

`defstruct` は名前付きフィールドを持つデータ型です。

```surtr
defstruct User {
  name: String,
  age: Int,
}

user: User = User { name: "alice", age: 30 }
print(to_string(user.name))
print(to_string(user.age))
```

### `defrecord`

`defrecord` はより簡潔に書けるレコード定義です。

```surtr
defrecord Point(x: Float, y: Float)

point = Point(1.0, 2.0)
point2 = Point(y: 5.0, x: 3.0)

print(to_string(point.x))
print(to_string(point2.x))
```

どちらもフィールドアクセスは `value.field` です。

## 8. エラーと Result

Surtr では、失敗は例外ではなく `Result<T>` の値で表すのが基本です。  
考え方としては `Either` に近く、「成功の枝」と「失敗の枝」を同じ式の中で明示的に扱います。

- 成功: `Ok(value)`
- 失敗: `Err(error)`

`Error` は「失敗値が乗る抽象の受け口」です。  
ユーザーコードが `Error(...)` のように直接作る具体型ではなく、`deferror` で作る個別 error がこの抽象に流れ込みます。

独自エラーは `deferror` で定義します。

```surtr
deferror Boom {
  "boom"
}

ok: Result<Int> = Ok(7)
er: Result<Int> = Err(Boom)
```

`match` で扱うのが基本形です。これは `Either` の左右を分岐するのと同じ感覚です。

```surtr
print(match ok {
  Ok(v) => to_string(v),
  Err(_) => "bad",
})
```

標準で提供される具体 error もあります。たとえば `NoneError` は最初から使えます。

```surtr
ret: Result<Int> = Err(NoneError)
match ret {
  Ok(val) => print("ok"),
  Err(e)  => print("none"),
}
```

### `=?` による Result の束縛

Result を途中で取り出し、失敗ならそのまま返したい場合は `=?` を使います。

```surtr
def pick() -> Result<Int> {
  value =? Ok(42)
  Ok(value)
}
```

これは「`Ok` なら束縛し、`Err` なら現在の評価を中断して伝播する」という糖衣構文です。  
例外送出ではなく、`Either` 的な分岐を短く書くための記法だと考えると追いやすくなります。

`Result` の内部表現は enum-like な 2 分岐の tagged value ですが、Surtr の言語仕様では将来の一般 `Enum` 機能と同じものとしては扱いません。  
あくまで `Result` は dedicated な失敗表現であり、`Ok` / `Err` もその専用 constructor として見せます。

## 9. リスト

リストは `[]` で書きます。

```surtr
nums = [1, 2, 3]
strs = ["a", "b", "c"]

empty: List<Int> = []
```

空リストは要素型が分からないため、型注釈を付けるのが基本です。

## 10. 組込み関数

現時点で公開済みとして扱える主な組込み関数は次のとおりです。

- `print(String) -> Unit`
- `to_string(A) -> String`
- `inspect(A) -> String`
- `safe_div(A, A) -> Result<A>`
- `safe_mod(Int, Int) -> Result<Int>`
- `eprint(Error) -> Unit`
- `set_exit_code(Int) -> Unit`

`safe_div` と `safe_mod` は、失敗を例外ではなく `Result` で返します。

## 11. 標準モジュールの前提

現在の Surtr では、標準モジュールを次の順で先に読み込みます。

```text
Bootstrap -> [Kernel, Int, String, Boolean, Error, List, Result, Float] -> user source
```

役割の分け方は次のとおりです。

- `Bootstrap`
  - auto-import の起点になる安定アンカー
  - `NoneError` などの bootstrap concrete error
- `Kernel`
  - auto import される最小の標準 API
  - `defmod Kernel` 配下に置かれる `print` のような cross-cutting builtin
  - 専用 file を持たない `Unit` の type 宣言
- 各 type module
  - `Int` や `String` のような型ごとの helper と説明
  - その型自身の `@@builtin type` 宣言

auto import されるのは `Bootstrap` と `Kernel` だけです。  
他の type module も標準モジュールとして一緒にロードされますが、名前空間としては別レイヤーで保ちます。

## 12. `@@doc` と source ドキュメント

Surtr の標準モジュールは、説明文も source に載せます。

```surtr
@@doc """
Standard `Int` type declaration.
User-visible integer values backed by BigInt.
"""
@@builtin type Int
```

この `@@doc` は単なるコメントではなく、定義に紐付いた metadata として扱われます。  
つまり、標準ライブラリの説明は `lib/*.srt` を開いた時点で読めるようにしておく、という設計です。

利用者として押さえておくとよい点は次のとおりです。

- canonical builtin type head は各対応 file のトップレベルに並ぶ
- `Unit` だけは専用 module file を持たず `kernel.srt` に置かれる
- 各 `defmod Name { ... }` がモジュール API になる
- builtin type、module、関数、標準 error には `@@doc` を付けられる

## 13. `Result<T>` と `Result<T, E>` の見え方

Surtr の builtin type として宣言されるのは `Result<T>` です。

```surtr
value: Result<Int> = Ok(42)
```

一方で、関数の戻り値では `Result<T, E>` という書き方を使うことがあります。  
これは「成功値 `T` に加えて、どの error 群を返す関数か」を Either の `Err` 側の契約として文書化するための表記で、型宣言そのものの head は `Result<T>` のままです。

利用者目線では次の理解で十分です。

- 値として扱うときは `Result<T>`
- 関数契約を詳しく見せたいときは `Result<T, E>` が現れることがある
- どちらも `Ok(...)` / `Err(...)` と `match` を中心に扱う

## 14. 現時点のスコープ

このガイドは、現時点で確定している範囲だけを対象にしています。

含むもの:

- 基本型
- 関数
- `if`
- `match`
- `defstruct`
- `defrecord`
- `deferror`
- `Result`
- `List`

含まないもの:

- trait
- 型エイリアス / NewType
- パイプライン `|>`
- マクロシステム拡張
- 並列コンパイル

細かい構文や外部契約を確認したい場合は、次に [言語リファレンス](./language-reference.md) を読むのがおすすめです。標準モジュールの配置や `@@doc` の約束を見たい場合は [標準ライブラリガイド](./standard-library.md) を参照してください。
