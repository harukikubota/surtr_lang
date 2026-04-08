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

`if_then` は条件付きで `Unit` を返す用途です。

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

Surtr では、失敗は `Result<T>` で表すのが基本です。

- 成功: `Ok(value)`
- 失敗: `Err(error)`

独自エラーは `deferror` で定義します。

```surtr
deferror Boom {
  "boom"
}

ok: Result<Int> = Ok(7)
er: Result<Int> = Err(Boom)
```

`match` で扱うのが基本形です。

```surtr
print(match ok {
  Ok(v) => to_string(v),
  Err(_) => "bad",
})
```

標準で提供されるエラーもあります。たとえば `NoneError` は最初から使えます。

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

現在の Surtr では、`Bootstrap` と `Kernel` という標準モジュール層を先に読み込みます。

- `Bootstrap`
  - builtin 宣言
  - 汎用 error 定義
- `Kernel`
  - builtin 以外の標準 API

この 2 つは auto import 対象です。つまり、通常のユーザーコードでは明示 `import` しなくても使えます。

## 12. 現時点のスコープ

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

細かい構文や外部契約を確認したい場合は、次に [言語リファレンス](./language-reference.md) を読むのがおすすめです。
