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

### 3.1 セミコロン

Surtr のセミコロン `;` には、単なる文区切り以上の意味があります。

- 行末に付けた式は `Unit` として扱われる
- 改行 1 つ分の区切りとして扱われる
- `Unit` を返す closure が求められる場所では、最後に `;` を置くだけで合わせやすい

もっとも基本的な形は「値を返す式を、あえて `Unit` 扱いにする」使い方です。

```surtr
print("hello");          # この式全体は Unit
name = "taro";           # `=` 自体も Unit を返す
```

束縛式 `name = "taro"` 自体が `Unit` を返すので、`Unit` を返す式としてさらに重ねられます。

```surtr
{
  name = "taro";
  print(name);
}
```

セミコロンは `InsertNewLine` としても扱われるため、改行しなくても次の式を続けて書けます。

```surtr
name = "taro"; print(name)
```

この性質は、`Unit` を返す closure を受け取る API と相性がよいです。  
最後の式に `;` を付けるだけで、closure 全体を `Unit` 戻り値として読ませられます。

```surtr
tap("taro", {|name|
  print(name);
})
```

複数式の closure でも同じです。

```surtr
tap("taro", {|name|
  label = "user=#{name}";
  print(label);
})
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

### 5.1 関数はどこに属するか

Surtr では、関数は必ず何らかの namespace に属します。

- 普通の module 関数は `defmod Name { ... }`
- 構造体 / enum 付属関数は `impl Type { ... }`
- trait の契約は `deftrait Name { ... }`
- trait 実装は `impl Trait for Type { ... }`
- script / REPL の top-level `def` も、暗黙の擬似 module に属する

特に `impl Type` は「`self` / `Self` が使える `defmod` の型専用版」と捉えると理解しやすくなります。

```surtr
defmod Math {
  def add(x: Int, y: Int) -> Int { x + y }
}

defstruct User {
  name: String,
}

impl User {
  def new(name: String) -> Self {
    User { name: name }
  }

  def normalize(self: Self) -> Self {
    self
  }
}
```

`defstruct` / `defenum` のような型定義そのものは関数 namespace の中ではなく、
top-level 宣言名として直接見えます。

### 5.2 Trait System V1

Surtr には V1 の trait system があります。まずは capability trait と operator dispatch trait を分けて読むと理解しやすいです。

```surtr
deftrait Describable {
  def describe(self: Self) -> String
}

def render<$T: Describable>(x: $T) -> String {
  Describable::describe(x)
}

def show_value(x: impl Show) -> String {
  to_string(x)
}
```

押さえておくとよい点は次のとおりです。

- `deftrait` は method 宣言だけを持つ
- trait は `deftrait Name<$T, ...> { ... }` のように型引数を取れる
- 実装は `impl Describable for Int { ... }` の形で書く
- `+`, `-`, `*` は `Add` / `Sub` / `Mul` の dispatch
- `Compare` は三値比較の正本で、`< <= > >=` も `Compare` を前提に動く
- trait 側の型引数を使う実装は `impl Trait<Concrete> for Type { ... }` の形で書く
- `impl Trait` は parameter 位置だけで使える
- 戻り値でも同じ型を使いたいときは `<$T: Describable>` のように名前付き bound を使う
- `-> impl Trait` と `where ...` はまだ使えない

target type を明示する trait では、compiler-reserved な witness type
値引数だけでは決まらない target type は、明示型引数で指定します。

```surtr
text = from::<String>(42)
number =? try_from::<Int>("42")
json =? Encode::encode::<JsonValue>(value)
config =? Decode::decode::<Config>(json)
```

`::<...>` は runtime の引数を増やさず、trait dispatch と generic specialization の型入力になります。capture でも `&Decode::decode::<Config>` のように同じ構文を使えます。

### 5.4 関数コールと関数値

Surtr では、次の見た目がそれぞれ別物です。

- call 式: `add(1, 2)`
- capture: `&add`, `&User::get_name`, `&add(&1, 10)`
- closure: `{|x| x + 1}`
- backtick FuncLiteral: ``1 `add` 2``

まず普通の call はその場で実行されます。

```surtr
def add(x: Int, y: Int) -> Int { x + y }

sum = add(1, 2)
```

一方で、関数演算子の compose 系が欲しいのは「実行結果」ではなく「あとで呼ぶ値」です。  
そのため、関数値が欲しいときは capture か closure を使います。

```surtr
inc = &add(&1, 1)
show_name = &User::get_name
render = {|name| "[" ++ name ++ "]"}
pipeline = &String::trim >> render
```

裸の関数参照は値になりません。

```surtr
pipeline = trim >> render   # 不可
pipeline = &trim >> render  # 可
```

backtick FuncLiteral は中置 call の sugar です。

```surtr
10 `+` 5
7 `eq` 7
```

これは関数値ではないので、`f = `eq`` のような束縛はできません。  
関数コール・capture・closure・FuncLiteral の違いをまとまって見たいときは `./callables.md` を参照してください。

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
のように branch が関数型で表されますが、通常の source では明示的な closure を
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
構造体リテラル、`new`、`deconstruct`、private field、property access、`match` での分解は
`./structs.md` にまとめています。

```surtr
defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name, age }
  }
}

user: User = User("alice", 30)
print(to_string(user.name))
print(to_string(user.age))
```

ここでは最小限だけ押さえると十分です。

- `User(...)` は `User::new(...)` の糖衣
- `User { ... }` は `impl User` 内でのみ使う
- `User { name, age }` は `User { name: name, age: age }` の shorthand
- 分解は field pattern ではなく `User::deconstruct` を通す

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
通常の user code では `Result<T>` を返す関数の中で使います。
現在きちんと使える対象は `Result`、`List`、`String` です。
`Option` は標準 enum として存在しますが、`=?` や Result 文脈の関数演算子では特別扱いしません。
必要な場合は `from::<Result>(value)` で明示的に `Result` へ変換します。
欠損を field として持ちつつ Result パイプへそのまま流したい場合は、
`Option<T>` より `T?` を使う方が自然です。
そのため `num: Int =? Option::Some(1)` はエラーです。

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

LHS には list/string 分解、literal match、Extractor を再帰的に書けます。  
途中で `Err(...)` が出ればそのまま早期伝播し、`NoMatch` は error として返されます。  
REPL ではその失敗を表示しますが、セッション自体は継続します。

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

固定範囲をその場で書きたいときは range literal も使えます。

```surtr
nums = [1..3]          # => [1, 2, 3]
chars = ["a".."c"]     # => Ok([a, b, c])
```

`Int` range はそのまま `List<Int>` になり、`String` range は char validation を伴うので `Result<List<String>, Error>` になります。
constant literal は compile-time に畳まれますが、surface 契約は変わりません。`String` endpoint が不正な場合は literal でも変数でも `Generator::range_char` と同じ `InvalidCharRange` が runtime に返ります。

ここでの単位元は `[]` です。  
Surtr は一般化された `pure` を置かず、`[]` と `List::cons` / `[x]` をはっきり分けています。
`[head, ..tail]` の分解は pattern 位置専用で、`List` と `String` のどちらにも使えます。
値側の基本操作は引き続き `List::cons` で、`["t", ..source]` は list 構築のままです。

## 10. パイプラインと合成

Surtr のパイプ系は大きく 2 種類あります。

- apply 系
  - `|>`
  - `|*>`
  - `|*|`
  - `|>=`
- compose 系
  - `>>`
  - `>*`
  - `>=>`

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
print(to_string(4 |> &add(&1, 1)))
print(to_string(4 |> {|x| x + 1}))
print(" alice " |> String::trim() |> String::surround("[", "]"))
```

関数値を変数に束縛して渡すこともできます。

```surtr
inc_fn: (Int -> Int) = &inc
print(to_string(4 |> inc_fn))
```

call 式が関数値を返す場合は、括弧で囲むと「返ってきた関数へ適用する」という意味になります。

```surtr
print(to_string(4 |> (make_adder(10))))
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

### 10.4 `|*|` は文脈内の function を適用する

`|*|` は `Applicative::apply` です。左辺に文脈内の function、右辺に
同じ文脈の value を置きます。

```surtr
Ok(&inc) |*| Ok(1)
Ok(curry(&Add::add)) |*| Ok(1) |*| Ok(2)
```

複数引数の function は `curry()` で明示的にカリー化します。演算子は
左結合なので、カリー化された function の引数を左から順に適用できます。

### 10.5 `>>`, `>*`, `>=>` は「関数値をつなぐ」

ここが apply 系との一番大きな違いです。  
compose 系は「値」ではなく「関数値」をつなぎます。

```surtr
pipeline = &trim >> &render
result_pipeline = &parse >=> &validate
lifted_pipeline = &parse >* &render
```

Surtr では、compose の左右は関数値です。capture、closure、または関数型の変数を渡せます。

```surtr
&parse >=> &validate
{|x| parse(x)} >=> {|y| validate(y)}
&parse >* &render
parser = &parse
validator = &validate
parser >=> validator
normalizer = &String::trim >> {|text| "[" ++ text ++ "]"}
```

次のような call 式は compose できません。

```surtr
parse() >=> validate()   # 不可
parse() >* render()      # 不可
inc() >> render()        # 不可
```

理由は単純で、`parse()` は compose 位置では関数値として扱わないからです。関数値を返す call 式を使いたい場合は `(make_parser()) >=> (make_validator())` のように括弧で明示します。

### 10.5 裸の関数参照は使わない

Surtr では裸の関数参照を関数値として扱いません。

```surtr
value |> normalize       # 不可
pipeline = parse >=> validate  # 不可
```

関数値がほしいなら `&` を付けます。

```surtr
value |> &normalize
pipeline = &parse >=> &validate
```

### 10.6 Backtick FuncLiteral

2 引数関数や既存演算子は backtick 付きの中置記法でも書けます。

```surtr
def eq(left: Int, right: Int) -> Boolean {
  left == right
}

print(to_string(10 `+` 5))
print(to_string(7 `eq` 7))
print("a" `concat` "b")
```

この構文は「関数値を作る」ものではなく、その場で 2 引数 call として解釈されます。

- ``left `name` right`` は `name(left, right)` と同じ
- ``left `+` right`` のような operator 版は対応する通常演算と同じ
- 単独の `` `eq` `` のような書き方はできない
- qualified path も使えるので、``left `Type::method` right`` は `Type::method(left, right)` と同じ

優先度は通常の pipeline / compose より強く、comparison より強い `Expr` クラスとして扱います。  
`+`, `-`, `*`, `++` は同列・左結合です。

```surtr
print(to_string(2 + 3 * 4))   # => 20
```

capture 側でも backtick 版を使えます。

```surtr
inc = &`+`(&1, 1)
not_fn = &`Boolean::not`
```

## 11. 組込み関数

現時点で公開済みとして扱える主な組込み関数は次のとおりです。

- `print(String) -> Unit`
- `to_string(A) -> String`
- `inspect(A) -> String`
- `safe_div(A, A) -> Result<A, ZeroDivisionError>` (`Int::safe_div` / `Float::safe_div` の runtime target)
- `safe_mod(Int, Int) -> Result<Int, ZeroDivisionError>`
- `eprint(Error) -> Unit`
- `set_exit_code(Int) -> Unit`

`safe_div` と `safe_mod` は、失敗を例外ではなく `Result<_, ZeroDivisionError>` で返します。  
`+`, `-`, `*` は内部では `Add` / `Sub` / `Mul` trait dispatch を通りますが、VM では引き続き具体的な opcode / builtin へ lower されます。

## 12. 標準定義ソースの前提

現在の Surtr では、標準定義ソースを次の順で先に読み込みます。

```text
Bootstrap -> [SpecialTypes, Kernel, Show, Eq, Ordering, Compare, Concat, From, TryFrom, Int, String, Regex, Boolean, Error, List, Generator, HashMap, Result, Option, Facet, Float] -> user source
```

役割の分け方は次のとおりです。

- `Bootstrap`
  - auto-import の起点になる安定アンカー
  - `NoneError` などの bootstrap concrete error
- `SpecialTypes`
  - `Unit`, `Hole` の canonical builtin type 宣言
- `Kernel`
  - auto import される最小の標準 API
  - `defmod Kernel` 配下に置かれる `print` のような cross-cutting builtin
- 各 type module
  - `Int` や `String` のような型ごとの helper と説明
  - その型自身の `@builtin type` 宣言

auto import されるのは `Bootstrap`, `Kernel`、`@autoimport` が付いた標準 `impl Type` owner helper surface と、`@autoimport` が付いた標準 trait です。  
他の type module も標準定義ソースとして一緒にロードされますが、名前空間としては別レイヤーで保ちます。

## 13. `@doc` と source ドキュメント

Surtr の標準定義ソースは、説明文も source に載せます。

```surtr
@doc """
Standard `Int` type declaration.
User-visible integer values backed by BigInt.
"""
@builtin type Int
```

この `@doc` は単なるコメントではなく、定義に紐付いた metadata として扱われます。  
つまり、標準ライブラリの説明は `lib/*.srt` を開いた時点で読めるようにしておく、という設計です。

利用者として押さえておくとよい点は次のとおりです。

- canonical builtin type head は各対応 file のトップレベルに並ぶ
- compiler-special type (`Unit`, `Hole`) は `special_types.srt` に集約される
- 各 `defmod Name { ... }` がモジュール API になる
- builtin type、module、関数、標準 error には `@doc` を付けられる

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
- trait
- `Result`
- `List`

含まないもの:

- 型エイリアス / NewType
- マクロシステム拡張
- 並列コンパイル

細かい構文や外部契約を確認したい場合は、次に [言語リファレンス](./language-reference.md) を読むのがおすすめです。標準定義ソースの配置や `@doc` の約束を見たい場合は [標準ライブラリガイド](./standard-library.md) を参照してください。
