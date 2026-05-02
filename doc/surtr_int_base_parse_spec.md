# Surtr Int Base Syntax / Parse API 仕様書

## 目的

Int の表記・文字列パース・基数メタ情報を次の3層に分離する。

1. **Int リテラル構文**
   - ソースコード上の数値表記。
   - `0xff`, `0o17`, `0b1101`, `0d123`, `123` を `Int` へ落とす。

2. **パース関数 API**
   - 文字列から `Int` を作る標準ライブラリ API。
   - 基数を引数に取らず、関数名またはリテラル構文で基数を決める。

3. **IntBase enum**
   - 基数の型安全な表現。
   - 内部実装・診断・メタ情報表示で使う。
   - 通常の公開パース API では基数引数として使わない。

---

## 基本方針

### 採用する方針

```surtr
0xff     // hex literal
0o17     // octal literal
0b1101   // binary literal
0d123    // explicit decimal literal
123      // default decimal literal
```

これらはすべて最終的に `Int` 型として扱う。

```surtr
let a: Int = 0xff
let b: Int = 0o17
let c: Int = 0b1101
let d: Int = 0d123
let e: Int = 123
```

### 採用しない方針

基数を第2引数に渡す API は標準公開 API としては採用しない。

```surtr
// 採用しない
Int::parse_base("ff", 16)
Int::parse_base("ff", IntBase::Hex)
try_from("ff", 16)
try_from("ff", IntBase::Hex)
```

理由:

- `from` / `try_from` の第2引数は `TypeNameOnly` として扱う。
- `try_from(String, Int)` にすると型変換構文と基数指定が混ざる。
- `parse_base(text, base)` にすると、以下のエラーが同じ Result に混ざる。
  - 入力文字列が基数に対して無効
  - 指定された基数そのものが無効
- 基数別関数に分ければ、無効な基数指定が API 上発生しない。

---

## Int リテラル構文

### 対応表

| 表記 | 基数 | 意味 |
|---|---:|---|
| `123` | 10 | prefix なしの10進数 |
| `0d123` | 10 | 明示的な10進数 |
| `0xff` | 16 | 16進数 |
| `0o17` | 8 | 8進数 |
| `0b1101` | 2 | 2進数 |

### prefix

| prefix | 基数 | IntBase |
|---|---:|---|
| `0b` | 2 | `IntBase::Bin` |
| `0o` | 8 | `IntBase::Oct` |
| `0d` | 10 | `IntBase::Dec` |
| `0x` | 16 | `IntBase::Hex` |

### prefix なし

prefix がない数値列は10進数として扱う。

```surtr
123
```

これは次と同じ意味を持つ。

```surtr
0d123
```

ただし、ソース上の表記としては `123` を標準形、`0d123` を明示形とする。

---

## 字句仕様

### 概要

Int リテラルは、字句解析または構文解析の段階で次の情報に分解する。

```text
IntLiteralToken {
  base: IntBase,
  body: String,
  span: Span,
}
```

例:

```surtr
0xff
```

```text
IntLiteralToken {
  base: IntBase::Hex,
  body: "ff",
  span: ...
}
```

### digit 本体

prefix は digit 本体に含めない。

| 入力 | base | body |
|---|---|---|
| `123` | `Dec` | `"123"` |
| `0d123` | `Dec` | `"123"` |
| `0xff` | `Hex` | `"ff"` |
| `0o17` | `Oct` | `"17"` |
| `0b1101` | `Bin` | `"1101"` |

### 不正リテラル

以下は不正。

```surtr
0o18
```

理由:

- `0o` は8進数。
- `8` は8進数の digit ではない。

これは runtime の `ParseIntError` ではなく、コンパイル時診断として扱う。

```text
invalid digit for octal integer literal: 8
```

### prefix のみ

以下は不正。

```surtr
0x
0o
0b
0d
```

理由:

- prefix の後に digit 本体が存在しない。

診断例:

```text
missing digits after integer base prefix: 0x
```

---

## Decimal prefix `0d`

### 役割

`0d` は10進数の明示 prefix として採用する。

```surtr
0d123
```

これは次と同じ値になる。

```surtr
123
```

### 採用理由

- `0x`, `0o`, `0b` と並べた時に基数表記として統一感がある。
- `parse_literal` でソース上の Int リテラル構文をそのまま扱える。
- 明示的に10進数として扱いたいケースを表現できる。

### 注意

`0d` 単体を値や base marker として扱わない。

```surtr
// 採用しない
Int::parse_base("123", 0d)
```

`0d` はあくまで Int リテラル構文上の prefix である。

---

## パース関数 API

## API 全体

```surtr
Int::parse("123")
Int::parse_dec("123")
Int::parse_hex("ff")
Int::parse_oct("17")
Int::parse_bin("1101")

Int::parse_literal("123")
Int::parse_literal("0d123")
Int::parse_literal("0xff")
Int::parse_literal("0o17")
Int::parse_literal("0b1101")
```

---

## `Int::parse`

### 役割

10進数文字列を `Int` に変換する。

```surtr
Int::parse("123")
```

これは `parse_dec` の別名として扱う。

```surtr
Int::parse("123") == Int::parse_dec("123")
```

### 受け付ける入力

```surtr
"0"
"123"
"999999999999999999999999999999"
```

### 受け付けない入力

```surtr
"0xff"
"0b1101"
"0o17"
"0d123"
"12x"
""
```

`Int::parse` は prefix 付きリテラル構文を読まない。  
prefix 付き表記を読みたい場合は `Int::parse_literal` を使う。

---

## 基数別パース関数

### `Int::parse_dec`

10進 digit 列を読む。

```surtr
Int::parse_dec("123")
```

### `Int::parse_hex`

16進 digit 列を読む。

```surtr
Int::parse_hex("ff")
Int::parse_hex("FF")
Int::parse_hex("10")
```

prefix は含めない。

```surtr
// 不正
Int::parse_hex("0xff")
```

### `Int::parse_oct`

8進 digit 列を読む。

```surtr
Int::parse_oct("17")
```

以下は不正。

```surtr
Int::parse_oct("18")
```

### `Int::parse_bin`

2進 digit 列を読む。

```surtr
Int::parse_bin("1101")
```

以下は不正。

```surtr
Int::parse_bin("102")
```

---

## `Int::parse_literal`

### 役割

Surtr の Int リテラル構文を文字列として読む。

```surtr
Int::parse_literal("123")
Int::parse_literal("0d123")
Int::parse_literal("0xff")
Int::parse_literal("0o17")
Int::parse_literal("0b1101")
```

### 意味

`parse_literal` は、ソースコード上の Int リテラルと同じ構文を文字列 API として提供する。

```surtr
let a = 0xff
let b = Int::parse_literal("0xff")
```

この2つは、成功時に同じ `Int` 値を持つ。

### prefix なし

prefix がなければ10進数として扱う。

```surtr
Int::parse_literal("123")
```

これは次と同じ意味。

```surtr
Int::parse_dec("123")
```

### prefix あり

```surtr
Int::parse_literal("0xff")   // Hex
Int::parse_literal("0o17")   // Oct
Int::parse_literal("0b1101") // Bin
Int::parse_literal("0d123")  // Dec
```

### 複合エラーを許容する理由

`parse_literal` は複合関数である。

内部的には次を行う。

1. prefix を読む
2. 基数を決める
3. digit 本体を読む
4. `Int` に変換する

そのため、以下のようなエラーが同じ Result に入ることは自然である。

```surtr
Int::parse_literal("0q123") // unknown base prefix
Int::parse_literal("0o18")  // invalid digit for octal
Int::parse_literal("0x")    // missing digits
Int::parse_literal("")      // empty
```

通常の基数別関数では無効な基数指定を発生させない。  
一方、`parse_literal` はソースリテラル構文そのものを読むため、prefix 解釈エラーを持つ。

---

## `from` / `try_from` との関係

`from` / `try_from` は型変換用 API として扱う。

```surtr
try_from("123", Int)
try_from("true", Bool)
try_from("2026-05-02", Date)
```

第2引数は `TypeNameOnly`。

そのため、以下は採用しない。

```surtr
try_from("ff", 16)
try_from("ff", IntBase::Hex)
```

基数付きの文字列パースは `Int` の関数 API に寄せる。

```surtr
Int::parse_hex("ff")
Int::parse_literal("0xff")
```

---

## IntBase enum

## 定義

```surtr
defenum IntBase {
  Bin,
  Oct,
  Dec,
  Hex,
}
```

## 役割

`IntBase` は、基数を型安全に扱うための enum である。

主な用途:

1. リテラルトークンの内部表現
2. パース実装の内部引数
3. エラー診断
4. 表示・メタ情報
5. ツール・LSP・REPL 表示

---

## IntBase のメソッド

```surtr
impl IntBase {
  def radix(self) -> Int {
    match self {
      IntBase::Bin => 2,
      IntBase::Oct => 8,
      IntBase::Dec => 10,
      IntBase::Hex => 16,
    }
  }

  def prefix(self) -> String {
    match self {
      IntBase::Bin => "0b",
      IntBase::Oct => "0o",
      IntBase::Dec => "0d",
      IntBase::Hex => "0x",
    }
  }

  def label(self) -> String {
    match self {
      IntBase::Bin => "binary",
      IntBase::Oct => "octal",
      IntBase::Dec => "decimal",
      IntBase::Hex => "hexadecimal",
    }
  }
}
```

### `radix`

基数を整数として返す。

```surtr
IntBase::radix(IntBase::Hex) // 16
```

### `prefix`

ソース上の prefix を返す。

```surtr
IntBase::prefix(IntBase::Hex) // "0x"
```

### `label`

診断・表示向けの名前を返す。

```surtr
IntBase::label(IntBase::Oct) // "octal"
```

---

## 公開 API と内部 API の分離

## 公開 API

ユーザが通常使う API。

```surtr
Int::parse("123")
Int::parse_dec("123")
Int::parse_hex("ff")
Int::parse_oct("17")
Int::parse_bin("1101")
Int::parse_literal("0xff")
```

公開 API は基数を引数に取らない。

## 内部 API

実装内部では `IntBase` を引数に取る共通関数を持ってよい。

```surtr
def parse_digits(text: String, base: IntBase) -> Result<Int, ParseIntError>
```

これは `private` または runtime/internal 扱いにする。

```surtr
impl Int {
  def parse_dec(text: String) -> Result<Int, ParseIntError> {
    Int::parse_digits(text, IntBase::Dec)
  }

  def parse_hex(text: String) -> Result<Int, ParseIntError> {
    Int::parse_digits(text, IntBase::Hex)
  }

  def parse_oct(text: String) -> Result<Int, ParseIntError> {
    Int::parse_digits(text, IntBase::Oct)
  }

  def parse_bin(text: String) -> Result<Int, ParseIntError> {
    Int::parse_digits(text, IntBase::Bin)
  }
}
```

この構成により、ユーザ API では無効な基数指定を発生させず、実装は共通化できる。

---

## エラー設計

## `ParseIntError`

基数がすでに確定しているパース関数のエラー。

対象:

```surtr
Int::parse
Int::parse_dec
Int::parse_hex
Int::parse_oct
Int::parse_bin
```

定義例:

```surtr
defenum ParseIntError {
  Empty,

  SignOnly,

  InvalidDigit {
    base: IntBase,
    char: String,
    index: Int,
  },
}
```

### `Empty`

入力が空。

```surtr
Int::parse_dec("")
```

### `SignOnly`

符号だけで digit がない場合に使う。  
符号を Int パース API で許可する場合のみ必要。

```surtr
Int::parse_dec("-")
```

### `InvalidDigit`

基数に対して使えない文字が現れた。

```surtr
Int::parse_oct("18")
```

```surtr
Err(ParseIntError::InvalidDigit {
  base: IntBase::Oct,
  char: "8",
  index: 1,
})
```

---

## `ParseIntLiteralError`

`Int::parse_literal` 用のエラー。

対象:

```surtr
Int::parse_literal
```

定義例:

```surtr
defenum ParseIntLiteralError {
  Empty,

  SignOnly,

  UnknownBasePrefix {
    prefix: String,
  },

  MissingDigits {
    base: IntBase,
  },

  InvalidDigit {
    base: IntBase,
    char: String,
    index: Int,
  },
}
```

### `UnknownBasePrefix`

未定義の prefix が使われた。

```surtr
Int::parse_literal("0q123")
```

### `MissingDigits`

prefix の後に digit がない。

```surtr
Int::parse_literal("0x")
```

### `InvalidDigit`

prefix から決まった基数に対して不正な文字が出現した。

```surtr
Int::parse_literal("0o18")
```

---

## エラー型を分ける理由

`parse_dec` / `parse_hex` / `parse_oct` / `parse_bin` は、基数が関数名で確定している。

```surtr
Int::parse_hex("fg")
```

この場合、失敗理由は「hex digit として無効」に限定される。

一方、`parse_literal` は prefix 解析を含む。

```surtr
Int::parse_literal("0q123")
```

この場合、失敗理由は「未知の prefix」である。

そのため、`ParseIntError` と `ParseIntLiteralError` を分けると責務が明確になる。

ただし、初期実装では共通化してもよい。  
その場合も、API 境界として以下は維持する。

- 基数別関数では無効な基数指定は起こらない。
- `parse_literal` では prefix 解釈エラーが起こりうる。

---

## コンパイラ診断との関係

## ソースコード上のリテラル

```surtr
let x = 0o18
```

これはコンパイル時診断にする。

```text
invalid digit for octal integer literal: 8
```

## 文字列 API

```surtr
Int::parse_literal("0o18")
```

これは runtime の `Result` として返す。

```surtr
Err(ParseIntLiteralError::InvalidDigit {
  base: IntBase::Oct,
  char: "8",
  index: 3,
})
```

## Span の扱い

コンパイラ内部では Span 付きの診断を出せるようにする。

```text
parse_int_literal_token(token: IntLiteralToken) -> Result<Int, Diagnostic>
```

標準ライブラリの文字列 API では Span を持たない。

```text
Int::parse_literal(text: String) -> Result<Int, ParseIntLiteralError>
```

---

## 実装方針

## 共通処理

内部では次の処理を共通化する。

```text
parse_literal(text)
  -> detect_base_prefix(text)
  -> parse_digits(body, base)
  -> Int
```

```text
parse_dec(text)
  -> parse_digits(text, IntBase::Dec)

parse_hex(text)
  -> parse_digits(text, IntBase::Hex)

parse_oct(text)
  -> parse_digits(text, IntBase::Oct)

parse_bin(text)
  -> parse_digits(text, IntBase::Bin)
```

---

## 擬似コード

```surtr
impl Int {
  def parse(text: String) -> Result<Int, ParseIntError> {
    Int::parse_dec(text)
  }

  def parse_dec(text: String) -> Result<Int, ParseIntError> {
    Int::parse_digits(text, IntBase::Dec)
  }

  def parse_hex(text: String) -> Result<Int, ParseIntError> {
    Int::parse_digits(text, IntBase::Hex)
  }

  def parse_oct(text: String) -> Result<Int, ParseIntError> {
    Int::parse_digits(text, IntBase::Oct)
  }

  def parse_bin(text: String) -> Result<Int, ParseIntError> {
    Int::parse_digits(text, IntBase::Bin)
  }

  def parse_literal(text: String) -> Result<Int, ParseIntLiteralError> {
    match IntLiteralSyntax::split(text) {
      Ok(parts) => Int::parse_digits_literal(parts.body, parts.base),
      Err(error) => Err(error),
    }
  }
}
```

内部専用:

```surtr
impl Int {
  def parse_digits(text: String, base: IntBase) -> Result<Int, ParseIntError> {
    // base ごとの digit 検証
    // BigInt 前提なら overflow は発生しない
    // 固定幅 Int を採用する場合のみ overflow を返す
  }
}
```


---

## 大文字小文字

16進数 digit は小文字・大文字の両方を許可する。

```surtr
Int::parse_hex("ff")
Int::parse_hex("FF")
Int::parse_hex("Ff")
```

prefix は小文字のみを標準とする。

```surtr
0xff
0o17
0b1101
0d123
```

以下は初期フェーズでは非対応でよい。

```surtr
0Xff
0O17
0B1101
0D123
```

必要であれば後方互換を壊さず追加できる。

---

## underscore 区切り

数値リテラルの可読性向上として `_` 区切りを追加できる。

```surtr
1_000_000
0xff_ff
0b1101_0010
```

これは本仕様の必須要素ではない。  
初期実装では後回しでよい。
