# Surtr Standard Library Layout

このページは、Surtr の標準モジュール構成を利用者向けにまとめたものです。

標準モジュールは単なる補助ファイルではなく、language surface の一部です。  
`lib/*.srt` に書かれた `@@doc` は source 上の説明であり、将来的には `.eldr` の `Docs` chunk からも参照できる前提で扱います。

Surtr 全体では、関数は常に何らかの namespace に属します。標準ライブラリでもこの方針は同じです。

- 通常の公開 API は `defmod Name { ... }` に置く
- trait 契約は `deftrait Name { ... }` に置く
- 型ごとの concrete 実装は `impl Trait for Type { ... }` に置く
- builtin type や error / enum 宣言は file top-level に置く

## 1. ロード順

標準モジュールの初期ロード順は次で固定されています。

```text
Bootstrap -> [Kernel, Numeric, Show, Eq, Ordering, Compare, Ord, Concat, From, TryFrom, Int, String, Regex, Boolean, Error, List, HashMap, Result, Lens, Float] -> user source
```

このうち auto import されるのは `Bootstrap` と `Kernel` だけです。  
他の標準モジュールは標準モジュールとして同梱されますが、名前空間としては明示 import 前提です。

## 2. 各モジュールの役割

### `Bootstrap`

- auto import の起点になる安定アンカー
- loader が最初に読む固定ステージ
- `import` / `include` builtin function docs の canonical anchor
- 標準 concrete error の置き場

`Bootstrap` は「何かでもかんでも置く場所」ではありません。  
将来 bootstrap 手順が増えても、入口の module 名と順序を固定するために残しています。
そのうえで、`NoneError` や `ZeroDivisionError` のような universally useful な
concrete error は、最初の標準ステージから使えるようここに置きます。
同時に、`import` / `include` のような language-provided macro surface も
`Bootstrap` module 配下の `@@builtin def` として source に残します。
ただし surface 構文では引き続き top-level 専用の special form として扱います。

### `Kernel`

- `defmod Kernel` の中に `if`, `if_then`, `assert`, `ensure`, `and`, `or`, `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `concat`, `print`, `to_string`, `inspect`, `eprint`, `set_exit_code` のような cross-cutting builtin を置く
- auto import される最小の標準 API を置く
- 専用 file を持たない `Unit` の builtin type 宣言を置く

primitive type に強く結びつかない builtin は、ここへ集めます。
特に `if` / `if_then` は言語特性に近い special form ですが、source 上の契約と
説明を標準 surface に残すため `Kernel` に置きます。
`and` / `or` も宣言上は通常の 2 引数関数ですが、コンパイラが short-circuit
評価へ lower する call-style helper としてここに置きます。
comparison / concat 系の call-style helper (`eq`, `lt`, `concat` など) も
primitive module をまたぐ読みやすさを優先して `Kernel` に置きます。

### type modules

現時点では次の module が用意されています。

- `Numeric`
- `Int`
- `String`
- `Boolean`
- `Error`
- `List`
- `HashMap`
- `Result`
- `Lens`
- `Float`

各 type module には 2 つの層があります。

1. file top-level の `@@builtin type ...`
2. `defmod Name { ... }` の module API

この分離により、「型そのものの compiler 契約」と「その型の helper / docs / 将来 API」を同じ file に置きつつ、役割は混ぜずに管理できます。
`impl Type` や `impl Trait for Type` は、この module API とは別の型専用 namespace として並びます。

`Numeric` だけは type module ではなく、トップレベル trait 宣言専用の標準 module です。

- `numeric.srt` に `deftrait Numeric` を置く
- `int.srt` のトップレベルに `impl Numeric for Int` を置く
- `float.srt` のトップレベルに `impl Numeric for Float` を置く
- `+`, `-`, `*` は `Numeric` dispatch を通るが、runtime には trait object を導入しない

## 3. `@@builtin type` の契約

標準型宣言は、各対応 file のトップレベルで canonical shape を宣言します。

```surtr
// kernel.srt
@@builtin type Unit

// int.srt
@@builtin type Int

// compiler-reserved type witness
@@builtin type TypeRef<$T>

// list.srt
@@builtin type List<$A>

// hash_map.srt
@@builtin type HashMap<$V>

// result.srt
@@builtin type Result<$T>
```

compiler はこの head 自体を契約として扱います。  
そのため、標準モジュール側で name や generic parameter が変わっていると compile error になります。

特に次は重要です。

- `List` は `List<$A>`
- `HashMap` は `HashMap<$V>`（key は常に `String`）
- `Result` は `Result<$T>`
- `TypeRef` は `TypeRef<$T>`

`Result<T, E>` は builtin type declaration ではなく、戻り値位置での error contract 記法として扱います。
`TypeRef<$T>` は ordinary value type ではなく、target type witness 専用の
compiler-reserved builtin type です。

`TypeRef<$T>` の使い道は限定されています。

- trait head で宣言した型引数に対応する witness parameter
- `From<$To>`, `TryFrom<$To>`, `Decode<$To>` のような target-oriented trait method の parameter
- `from(value, TargetTy)` / `try_from(value, TargetTy)` の第 2 引数を内部で表す型

逆に、次には使いません。

- 通常の `def` の引数や戻り値
- field type
- local binding の型注釈
- first-class value としての生成・保存・返却

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

`Result` の内部表現は enum-like な tagged value ですが、言語仕様では `defenum` と同一視しません。  
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
- language-provided macro surface も bootstrap module の function docs として揃えられる

## 6. いま読むときの目印

- `Bootstrap`
  - auto import の固定起点と bootstrap error 群
- `Kernel`
  - cross-cutting builtin と `Unit`
  - `if` / `if_then` の language-level contract
- `Numeric`
  - compile-time only trait dispatch の最初の公開契約
  - `Int` / `Float` が共有する算術 surface
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
  - `[]` を Nil とし、トップレベルの `cons`, `first`, `len` と `List` module helper を持つ
- `HashMap`
  - immutable な insertion-order map（key は常に `String`）
  - `HashMap::empty` / `from_entries` / `insert` / `remove` / `get` / `keys` / `values` を持つ
  - `inspect` / `to_string` は `HashMap("key" => value, ...)` 形式
- `Result`
  - `Ok` / `Err` を中心にした Either 指向の失敗表現
- `Lens`
  - compile-time path capability
  - `Type.segment` / `value.segment` / `Lens::view` / `Lens::set` / `Lens::over`
- `Float`
  - 実装はあるが契約整理を継続中の型

## 7. 更新するときのルール

- cross-cutting runtime builtin value を足すときは `kernel.srt` の `defmod Kernel` と shared builtin metadata の両方を更新する
- `Numeric` surface を増やすときは `numeric.srt` の trait 宣言、各 concrete impl、Scar の trait dispatch、Forge の lowering を同時に更新する
- `if` / `assert` / `and` / `eq` のような compiler-handled helper を足すときは `kernel.srt` と resolver/checker の canonical contract を同時に更新する
- builtin type を変えるときは、対応する `lib/*.srt` の `@@builtin type` と compiler 側の canonical contract を同時に更新する
- `Result` constructor contract を変えるときは `result.srt` の `Ok` / `Err` 宣言と checker 側の canonical rule を同時に更新する
- module API を足すときは `defmod Name` に実装し、まず `@@doc` を先に書く

## 8. `List` helper surface

`List` は pipeline / bind 系の外部契約を支える最小 helper surface を持ちます。

### `List` helpers

```surtr
List::cons(1, [])     # => [1]
List::first([1, 2])   # => Ok(1)
List::len([1, 2, 3])  # => 3
```

- `List::cons` は先頭への prepend
- `List::first` は `Result` で先頭要素を返す
- `List::len` は O(1) 契約を保つ core helper
- `[]` は Nil 側の単位元として扱う
- `List` は先頭からの逐次処理と pattern 分解を主用途にする
- MatchBlock では `Kernel::uncons(head, tail)` または `[head, ..tail]` で `List` / `String` を分解する
- Expr 位置の `["t", ..source]` は list 構築であり、String constructor ではない

### `List::map`

```surtr
List::map([1, 2, 3], &to_string)
```

- plain unary callable を各要素へ適用する
- `|*>` の `List` 側意味と一致する
- flatten はしない

`inspect(&to_string)` や `inspect(&Boolean::xor)` のように bare callable を観察すると、
利用者向けには `FnCapture(module: ..., name: ..., signature: ...)` 形式で表示される。

### `List::reduce` / `List::reduce_while`

```surtr
List::reduce([1, 2, 3], 0, {|acc, x| acc + x })
List::reduce_while([1, 2, 3], 0, {|acc, x|
  if(x == 2, ReduceStep::Stop(acc), ReduceStep::Resume(acc + x))
})
```

- `reduce` は左畳み込み
- `reduce_while` は `ReduceStep::Resume` / `ReduceStep::Stop` で途中終了できる

### 推奨スタイル

```surtr
acc = List::reduce(xs, [], {|acc, x|
  List::cons(f(x), acc)
})

ret = List::reverse(acc)
```

- `List` 構築は `List::cons + List::reverse` を基本にする
- `acc ++ [x]` のような後方連結中心の構築は避ける

### 最小 API に含めないもの

- `uncons` 関数
- `tail`
- `append`
- `concat`
- `flat_map`
- `zip`
- `take`
- `drop`
- `sort`
- indexed access

### どう使い分けるか

- 値 1 個から `List` を作る: `[x]` または `List::cons(x, [])`
- 要素数を見る: `List::len`
- 先頭だけほしい: `List::first`
- `List` の shape を保って中身だけ変える: `|*>` または `List::map`
- 条件付き検索や途中終了をしたい: `List::find`, `List::find_map`, `List::any`, `List::all`, `List::reduce_while`

## 9. `HashMap` module の位置づけ

`HashMap` は key を `String` に固定した immutable map です。

```surtr
@@builtin type HashMap<$V>
```

公開 surface は次で固定されています。

- `HashMap::empty() -> HashMap<$V>`
- `HashMap::from_entries(List<(String, $V)>) -> HashMap<$V>`
- `HashMap::len(map) -> Int`
- `HashMap::contains_key(map, key) -> Boolean`
- `HashMap::get(map, key) -> Result<$V>`（miss は `Err(NoneError)`）
- `HashMap::insert(map, key, value) -> HashMap<$V>`
- `HashMap::remove(map, key) -> HashMap<$V>`
- `HashMap::keys(map) -> List<String>`
- `HashMap::values(map) -> List<$V>`

意味論の要点:

- `insert` で duplicate key を更新すると、値のみ差し替え、最初の挿入順を維持する
- `remove` は key が存在しない場合 no-op
- `keys` / `values` は insertion order を保つ
- `inspect` / `to_string` は key を quoted string で表示し、空 map は `HashMap()` と表示する

将来 `hash![...]` の literal sugar を入れる余地はあるが、現時点の正本 surface は `HashMap::from_entries` / `HashMap::insert` を基準にする。

## 10. `Result` module の位置づけ

`Result` module は constructor contract と、よく使う variant 判定 helper の置き場です。

```surtr
@@builtin type Result<$T>
@@builtin type Ok($T) -> Result<$T>
@@builtin type Err(Error) -> Result<$T>
```

現時点でも中心は `Ok(...)`, `Err(...)`, `match`, `=?`, `|*>`, `|>=`, `|=>` の言語構文と型規則ですが、
`Result::is_ok(...)` / `Result::is_err(...)` で variant 判定だけを簡潔に書けます。

## 11. `Lens` module の位置づけ

`Lens` は runtime の first-class value ではなく、compile-time にだけ存在する
path capability です。

```surtr
@@builtin type Lens<$S, $A>
```

読み方は次です。

- `S` は source の型
- `A` は focus の型
- `Lens<S, A>` は「`S` の中の `A` を指す path」

### path の書き方

もっともよく使う path は `Type.segment` です。

```surtr
User.name
Profile.display_name
```

- struct / record field path を作る
- 値を読むのではなく path 自体を表す

tuple path は `Tuple._N` を使います。

```surtr
Tuple._0
Tuple._1
```

- `.0`, `.1` ではなく `._0`, `._1`
- `Tuple._N` は `Lens<(...), ...>` が期待される場所でだけ使う
- `_0` 単体は使わない

enum variant path は `Enum.Variant` です。

```surtr
Expr.Add
Token.Ident
```

- selector は PascalCase 固定
- 実行時の値がその variant でなければ `Err(VariantMismatch(...))` になる

ネストした path は `Lens::compose` でつなぎます。

```surtr
Lens::compose(User.profile, Profile.name)
```

### `value.segment` は read sugar

`value.segment` は path をその値に適用する sugar です。

```surtr
name = user.name
first = pair._0
```

これは概念的には次と同じです。

```surtr
name = Lens::view(User.name, user)
first = Lens::view(Tuple._0, pair)
```

### `Lens::view`

`Lens::view(lens, source)` は path の先を読みます。

```surtr
name = Lens::view(User.name, user)
first = Lens::view(Tuple._0, pair)
profile_name = Lens::view(Lens::compose(User.profile, Profile.name), user)
```

返り値は path と source に応じて変わります。

- plain field / tuple path を plain value に適用すると plain `A`
- `source` が `Result<S>` なら `Result<A>`
- path に variant selector を含むと `Result<A>`

例:

```surtr
match Lens::view(Expr.Add, expr) {
  Ok(add_expr) => ...,
  Err(err) => ...,
}
```

### `Lens::set`

`Lens::set(lens, source, value)` は focus を置き換え、常に `Result<S>` を返します。

```surtr
user2 =? Lens::set(User.name, user, "bob")
pair2 =? Lens::set(Tuple._1, pair, 4)
```

ネストした値も同じです。

```surtr
profile_name = Lens::compose(User.profile, Profile.name)
user2 =? Lens::set(profile_name, user, "bob")
```

### `Lens::over`

`Lens::over(lens, source, update_fun)` は現在値を見てから更新します。

```surtr
user2 =? Lens::over(User.name, user, {|name|
  Ok(name ++ "!")
})
```

- `update_fun` は `A -> Result<A>` を返す必要がある
- `Err(...)` を返したらそのまま伝播する
- 返り値は常に `Result<S>`

### `Lens::compose`

`Lens::compose(outer, inner)` は 2 つの path を順につなぎます。

```surtr
profile_name = Lens::compose(User.profile, Profile.name)
name = Lens::view(profile_name, user)
```

型の並びは次です。

- `outer: Lens<S, A>`
- `inner: Lens<A, B>`
- result: `Lens<S, B>`

### Stage1 の制約

`Lens` は compile-time only なので、同一スコープで消費します。

```surtr
lens = User.name
name = Lens::view(lens, user)
```

一方で、次はできません。

- 関数引数として渡す
- 関数から返す
- closure で capture する
- `List`, tuple, `Ok(...)`, `Err(...)` などの runtime container に入れる

関数境界を越えたいときは `Lens` 自体ではなく、`Lens::view(...)` 済みの値を渡します。

## 12. パイプ / bind 系と標準モジュールの関係

標準モジュール側から見ると、各演算子との対応は次です。

| 構文 | 標準 surface / 役割 |
|---|---|
| `x |> f(1)` | call 式への第一引数注入 |
| `list |*> f()` | `List::map` と同じ方向の変換 |
| `list |>= f()` | `List` の bind 方向の変換 |
| `&f |=> &g` | `List` または `Result` を返す関数の合成 |

重要なのは、compose 系の実装詳細ではなく surface contract です。

- apply 系は call 式でも書ける
- compose 系は closure value を要求する
- `List` は helper surface を公開する
- `Result` は言語構文中心だが、variant 判定 helper も持つ
