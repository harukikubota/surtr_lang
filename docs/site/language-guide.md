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

### 5.1 Trait System V1

Surtr には V1 の trait system があります。最初の trait は `Numeric` です。

```surtr
deftrait Numeric {
  def add(self: Self, rhs: Self) -> Self
}

def twice<$N: Numeric>(x: $N) -> $N {
  x + x
}

def show_abs(x: impl Numeric) -> String {
  inspect(Numeric::abs(x))
}
```

押さえておくとよい点は次のとおりです。

- `deftrait` は method 宣言だけを持つ
- 実装は `impl Numeric for Int { ... }` の形で書く
- `impl Trait` は parameter 位置だけで使える
- 戻り値でも同じ型を使いたいときは `<$N: Numeric>` のように名前付き bound を使う
- `-> impl Numeric` と `where ...` はまだ使えない

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

## 7. 構造体・レコード・Enum

Surtr には `defstruct` / `defrecord` / `defenum` があります。

### `defstruct`

`defstruct` は名前付きフィールドを持つデータ型です。

```surtr
defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}

user: User = User("alice", 30)
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

### `defenum`

`defenum` はバリアントを持つ代数的データ型です。

```surtr
defenum Direction {
  Up,
  Down,
  Left,
  Right,
}

dir: Direction = Direction::Left
print(match dir {
  Direction::Up => "U",
  Direction::Down => "D",
  Direction::Left => "L",
  Direction::Right => "R",
})
```

タプル payload 付きバリアントも使えます。

```surtr
defenum KeyInput {
  Arrow(Direction),
  Enter,
}

key: KeyInput = KeyInput::Arrow(Direction::Up)
print(match key {
  KeyInput::Arrow(d) => "arrow",
  KeyInput::Enter => "enter",
})
```

補足:

- `Enum::Variant(...)` で値を作る
- `match` は網羅必須
- enum 値への field access（例: `.idx`）はサポートしない

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

`=?` は Result 専用というより、Surtr では「失敗を伝播する束縛」の入口です。  
現在きちんと使える対象は `Result`、`List`、`String` です。

```surtr
[head, ..tail] =? [1, 2, 3]
print(to_string(head))
print(to_string(tail))
```

```surtr
[first, ..tail] =? "source"
print(first)   # => "s"
print(tail)    # => "ource"
```

`Result` の内部表現は enum-like な 2 分岐の tagged value ですが、Surtr の言語仕様では `defenum` と同一 contract にはしません。  
あくまで `Result` は dedicated な失敗表現であり、`Ok` / `Err` もその専用 constructor として見せます。

## 9. リスト

リストは `[]` で書きます。

```surtr
nums = [1, 2, 3]
strs = ["a", "b", "c"]

empty: List<Int> = []
```

空リストは要素型が分からないため、型注釈を付けるのが基本です。

`List` には値操作とパイプ / bind 系で使う helper surface があります。

```surtr
List::cons(1, [])             # => [1]
List::first([1, 2, 3])        # => Ok(1)
List::len([1, 2, 3])          # => 3
List::map([1, 2], &to_string)
List::find_map([1, 2], &lookup)
```

ここでの単位元は `[]` です。  
Surtr は一般化された `pure` を置かず、`[]` と `List::cons` / `[x]` をはっきり分けています。
`[head, ..tail]` の分解は pattern 位置専用で、`List` と `String` のどちらにも使えます。
値側の基本操作は引き続き `List::cons` で、`["t", ..source]` は list 構築のままです。

## 10. パイプラインと合成

Surtr のパイプ系は大きく 2 種類あります。

- apply 系
  - `|>`
  - `|*>`
  - `|>=`
- compose 系
  - `>>`
  - `|=>`

### 10.1 `|>` は「値を流す」

一番基本の形はこれです。

```surtr
def add(x: Int, y: Int) -> Int { x + y }
print(to_string(1 |> add(2)))
```

このとき `1 |> add(2)` は `add(1, 2)` と同じ意味です。  
Surtr では call 式の第一引数へ左辺値を注入するので、Elixir に近い読み方ができます。

capture や closure も使えます。

```surtr
print(to_string(4 |> &add(1)))
print(to_string(4 |> {|x| x + 1}))
```

bare capture を値として観察したいときは `inspect(...)` を使います。

```surtr
print(inspect(&Boolean::xor))
# => FnCapture(module: Boolean, name: xor, signature: xor(left: Boolean, right: Boolean) -> Boolean)
```

method path も同じです。

```surtr
user |> User::get_name()
```

これは `User::get_name(user)` の意味です。

### 10.2 `|*>` は文脈の中身だけ変える

`|*>` は `Result` や `List` の shape を保ったまま、中の値だけ変えます。

```surtr
Ok(1) |*> add(2)
[1, 2, 3] |*> add(10)
```

読み方は次です。

- `Ok(1) |*> add(2)` は `Ok(add(1, 2))`
- `[1, 2, 3] |*> add(10)` は各要素へ `add(elem, 10)`

右辺は plain function である必要があります。  
`A -> Result<B>` や `A -> List<B>` を渡したいときは `|>=` を使います。

### 10.3 `|>=` は次の文脈段階へ進む

`|>=` は bind です。

```surtr
def require_at_least(x: Int, floor: Int) -> Result<Int, TooSmall> {
  if(x >= floor, Ok(x), Err(TooSmall))
}

value: Result<Int> = Ok(11)
checked = value |>= require_at_least(10)
```

ここでも右辺が call 式なら左辺値が第一引数へ注入されます。

`List` では flat_map 的に動きます。

```surtr
def expand(n: Int) -> List<Int> { [n, n + 10] }
print(to_string([1, 2, 3] |>= expand()))
```

### 10.4 `>>` と `|=>` は「関数値をつなぐ」

ここが apply 系との一番大きな違いです。  
compose 系は「値」ではなく「関数値」をつなぎます。

```surtr
pipeline = &trim >> &render
result_pipeline = &parse |=> &validate
```

Surtr では、compose の左右は capture か closure だけです。

```surtr
&parse |=> &validate
{|x| parse(x)} |=> {|y| validate(y)}
```

次のような call 式は compose できません。

```surtr
parse() |=> validate()   # 不可
inc() >> render()        # 不可
```

理由は単純で、`parse()` は関数そのものではなく「実行結果の値」だからです。

### 10.5 裸の関数参照は使わない

Surtr では裸の関数参照を関数値として扱いません。

```surtr
value |> normalize       # 不可
pipeline = parse |=> validate  # 不可
```

関数値がほしいなら `&` を付けます。

```surtr
value |> &normalize
pipeline = &parse |=> &validate
```

### 10.6 Backtick FuncLiteral

2 引数関数や既存演算子は backtick 付きの中置記法でも書けます。

```surtr
def eq(left: Int, right: Int) -> Boolean {
  left == right
}

print(to_string(10 `+` 5))
print(to_string(7 `eq` 7))
```

この構文は「関数値を作る」ものではなく、その場で 2 引数 call として解釈されます。

- ``left `name` right`` は `name(left, right)` と同じ
- ``left `+` right`` のような operator 版は対応する通常演算と同じ
- 単独の `` `eq` `` のような書き方はできない
- V1 では unqualified name と symbolic operator だけを対象にする
- `` `Type::method` `` のような qualified backtick path は未対応

優先度は通常の pipeline / compose より強く、comparison より強い `Expr` クラスとして扱います。  
`+`, `-`, `*`, `++` は同列・左結合です。

```surtr
print(to_string(2 + 3 * 4))   # => 20
```

一方、capture 側の operator 版や placeholder capture はまだありません。

```surtr
f = &`+`   # 未実装
```

## 11. 組込み関数

現時点で公開済みとして扱える主な組込み関数は次のとおりです。

- `print(String) -> Unit`
- `to_string(A) -> String`
- `inspect(A) -> String`
- `safe_div(A, A) -> Result<A, ZeroDivisionError>`
- `safe_mod(Int, Int) -> Result<Int, ZeroDivisionError>`
- `eprint(Error) -> Unit`
- `set_exit_code(Int) -> Unit`

`safe_div` と `safe_mod` は、失敗を例外ではなく `Result<_, ZeroDivisionError>` で返します。  
`+`, `-`, `*` は内部では `Numeric` trait dispatch を通りますが、VM では引き続き具体的な opcode / builtin へ lower されます。

## 12. 標準モジュールの前提

現在の Surtr では、標準モジュールを次の順で先に読み込みます。

```text
Bootstrap -> [Kernel, Numeric, Int, String, Boolean, Error, List, Result, Float] -> user source
```

役割の分け方は次のとおりです。

- `Bootstrap`
  - auto-import の起点になる安定アンカー
  - `NoneError` などの bootstrap concrete error
- `Kernel`
  - auto import される最小の標準 API
  - `defmod Kernel` 配下に置かれる `print` のような cross-cutting builtin
  - 専用 file を持たない `Unit` の type 宣言
- `Numeric`
  - compile-time trait dispatch の基準になる trait 宣言
  - `Int` / `Float` が共有する `add`, `sub`, `mul`, `safe_div`, `abs`, `min`, `max` の契約
- 各 type module
  - `Int` や `String` のような型ごとの helper と説明
  - その型自身の `@@builtin type` 宣言

auto import されるのは `Bootstrap` と `Kernel` だけです。  
他の type module も標準モジュールとして一緒にロードされますが、名前空間としては別レイヤーで保ちます。

## 13. `@@doc` と source ドキュメント

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

## 14. `Result<T>` と `Result<T, E>` の見え方

Surtr の builtin type として宣言されるのは `Result<T>` です。

```surtr
value: Result<Int> = Ok(42)
```

一方で、関数の戻り値では `Result<T, E>` という書き方を使うことがあります。  
これは「成功値 `T` に加えて、どの error 群を返す関数か」を Either の `Err` 側の契約として文書化するための表記で、型宣言そのものの head は `Result<T>` のままです。

利用者目線では次の理解で十分です。

- 値として扱うときは `Result<T>`
- 関数契約を詳しく見せたいときは `Result<T, E>` が現れることがある
- どちらも `Ok(...)` / `Err(...)` と `match` を中心に扱い、variant 判定だけなら `Result::is_ok(...)` / `Result::is_err(...)` も使える

## 15. 現時点のスコープ

このガイドは、現時点で確定している範囲だけを対象にしています。

含むもの:

- 基本型
- 関数
- `if`
- `match`
- `defstruct`
- `defrecord`
- `deferror`
- trait (`Numeric` first)
- `Result`
- `List`

含まないもの:

- 型エイリアス / NewType
- マクロシステム拡張
- 並列コンパイル

細かい構文や外部契約を確認したい場合は、次に [言語リファレンス](./language-reference.md) を読むのがおすすめです。標準モジュールの配置や `@@doc` の約束を見たい場合は [標準ライブラリガイド](./standard-library.md) を参照してください。
