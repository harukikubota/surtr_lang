# Surtr Standard Library Layout

このページは、Surtr の標準定義ソース構成を利用者向けにまとめたものです。

標準定義ソースは単なる補助ファイルではなく、language surface の一部です。  
`lib/*.srt` に書かれた `@doc` は source 上の説明であり、将来的には `.eldr` の `Docs` chunk からも参照できる前提で扱います。

Surtr 全体では、関数は常に何らかの namespace に属します。標準ライブラリでもこの方針は同じです。

- 通常の公開 API は `defmod Name { ... }` に置く
- trait 契約は `deftrait Name { ... }` に置く
- 型ごとの concrete 実装は `impl Trait for Type { ... }` に置く
- builtin type や error / enum 宣言は file top-level に置く

## 1. ロード順

標準定義ソースの初期ロード順は次で固定されています。

```text
Bootstrap -> [SpecialTypes, Function, Kernel, Add, Sub, Mul, Eq, Neq, Compare, Concat, Show, Ordering, Tuple, From, TryFrom, Encode, Decode, Functor, Applicative, Monad, PipeApply, Compose, Composable, LiftComposable, KleisliComposable, Int, String, Regex, Boolean, Error, List, Generator, HashMap, Result, Duration, Range, Option, Task, Facet, Float, Json, Config, Project, Random, File, FS, IO, Shell, StyledDoc, Test] -> user source
```

このうち auto import されるのは `Bootstrap`, `Kernel` と、`@autoimport` が付いた標準 `impl Type` owner helper surface および標準 trait です。  
他の標準定義ソースは標準定義ソースとして同梱されますが、名前空間としては明示 import 前提です。

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
`Bootstrap` module 配下の `@builtin def` として source に残します。
ただし surface 構文では引き続き top-level 専用の special form として扱います。

### `Kernel`

- `defmod Kernel` の中に `if`, `if_then`, `assert`, `ensure`, `and`, `or`, `eq`, `neq`, `concat`, `print`, `to_string`, `inspect`, `eprint`, `set_exit_code` のような cross-cutting builtin を置く
- auto import される最小の標準 API を置く

primitive type に強く結びつかない builtin は、ここへ集めます。
特に `if` / `if_then` は言語特性に近い special form ですが、source 上の契約と
説明を標準 surface に残すため `Kernel` に置きます。
`and` / `or` も宣言上は通常の 2 引数関数ですが、コンパイラが short-circuit
評価へ lower する call-style helper としてここに置きます。
equality / concat 系の call-style helper (`eq`, `neq`, `concat` など) も
primitive module をまたぐ読みやすさを優先して `Kernel` に置きます。
ordered comparison は `compare(left, right)` または `< <= > >=` を使い、専用の Boolean helper 名は公開しません。

### `SpecialTypes`

- `special_types.srt` に compiler-special builtin type を集約する
- 現在は `Unit`, `Closure`, `MatchArms<$Scrutinee, $Result>`, `CondClauses<$Result>`, `BulkUpdateEntries<$State>`, `Lazy<$T>`, `StandbyInit<$T>`, `Hole` をここへ置く
- `defmod` は持たず、top-level canonical type declaration だけを持つ
- user-facing な振る舞いは各 trait / callable / module surface 側から現れる

### type modules

現時点では次の module が用意されています。

- `Int`
- `String`
- `Boolean`
- `Error`
- `List`
- `Generator`
- `HashMap`
- `Result`
- `Range`
- `Facet`
- `Float`

各 type module には 2 つの層があります。

1. file top-level の `@builtin type ...`
2. `defmod Name { ... }` の module API

この分離により、「型そのものの compiler 契約」と「その型の helper / docs / 将来 API」を同じ file に置きつつ、役割は混ぜずに管理できます。
`impl Type` や `impl Trait for Type` は、この module API とは別の型専用 namespace として並びます。

数値 helper は共通 trait ではなく、`Int` / `Float` の type owner surface として置きます。

- `int.srt` の `impl Int` に `safe_div`, `safe_mod`, `abs`, `min`, `max` などを置く
- `float.srt` の `impl Float` に `safe_div`, `abs`, `min`, `max` などを置く
- `+`, `-`, `*` は `Add` / `Sub` / `Mul` dispatch を通るが、runtime には trait object を導入しない

## 3. `@builtin type` の契約

標準型宣言は、各対応 file のトップレベルで canonical shape を宣言します。

```surtr
// special_types.srt
@builtin type Unit

// special_types.srt
@builtin type Hole

// int.srt
@builtin type Int

// list.srt
@builtin type List<$A>

// hash_map.srt
@builtin type HashMap<$V>

// result.srt
@builtin type Result<$T>
```

compiler はこの head 自体を契約として扱います。  
そのため、標準定義ソース側で name や generic parameter が変わっていると compile error になります。

特に次は重要です。

- `List` は `List<$A>`
- `HashMap` は `HashMap<$V>`（key は常に `String`）
- `Result` は `Result<$T>`
- `Hole` は ignored-input callable marker

`Result<T, E>` は builtin type declaration ではなく、戻り値位置での error contract 記法として扱います。
`Hole` は ordinary data type ではなく、`_` の背後にある callable marker です。

target-oriented trait の型入力は `from::<Target>(value)` のような明示型引数で指定します。

compiler-special type の詳しい説明は `./special-types.md` を参照してください。

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
@builtin type Ok($T) -> Result<$T>
@builtin type Err(Error) -> Result<$T>
```

この 2 行は通常の関数本体付き `def` ではなく、compiler が特別扱いする declaration-only contract です。

## 5. `@doc` の使い方

標準定義ソースの説明は `@doc """..."""` で source に直接載せます。

```surtr
@doc """
Standard `String` type declaration.
Text values produced by literals, interpolation, and textual conversion use this
head.
"""
@builtin type String

@doc """
Concrete string-module error for `String::repeat`.
Negative counts stay as recoverable values instead of becoming implicit
runtime traps.
"""
deferror NegativeRepeatCount(count: Int) {
  "repeat count must be non-negative: #{count}"
}

impl String {
  @doc """
  Return the number of Unicode scalar values in the string.
  """
  @builtin def len(value: String) -> Int
}
```

`@doc` を source に置く利点は次のとおりです。

- 標準定義ソースと説明文がずれにくい
- dump や REPL の docs UI に同じ情報を流せる
- Rust 実装ではなく Surtr surface として API を説明できる
- language-provided macro surface も bootstrap module の function docs として揃えられる

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
  - `[]` を Nil とし、トップレベルの `cons`, `first`, `len` と `List` module helper を持つ
- `HashMap`
  - immutable な key-sorted map（key は常に `String`）
  - `hash![key => value, ...]` literal を持ち、key は `String` 型を得られる式
  - `HashMap::empty` / `from_entries` / `insert` / `remove` / `get` / `keys` / `values` を持つ
  - `inspect` / `to_string` は `hash!["key" => value, ...]` 形式
- `Result`
  - `Ok` / `Err` を中心にした Either 指向の失敗表現
- `Facet`
  - compile-time path capability
  - `Type.segment` / `value.segment` / `Facet::view` / `Facet::set` / `Facet::over`
- `Float`
  - 実装はあるが契約整理を継続中の型

## 7. 更新するときのルール

- cross-cutting runtime builtin value を足すときは `kernel.srt` の `defmod Kernel` と shared builtin metadata の両方を更新する
- 数値 helper surface を増やすときは対象 type owner (`int.srt` / `float.srt`) と shared builtin metadata / Forge lowering を同時に更新する
- `if` / `assert` / `and` / `eq` のような compiler-handled helper を足すときは `kernel.srt` と resolver/checker の canonical contract を同時に更新する
- builtin type を変えるときは、対応する `lib/*.srt` の `@builtin type` と compiler 側の canonical contract を同時に更新する
- `Result` constructor contract を変えるときは `result.srt` の `Ok` / `Err` 宣言と checker 側の canonical rule を同時に更新する
- module API を足すときは `defmod Name` に実装し、まず `@doc` を先に書く

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
@builtin type HashMap<$V>
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

- `insert` で duplicate key を更新すると、値のみ差し替える
- `remove` は key が存在しない場合 no-op
- `keys` / `values` はキー昇順 deterministic order を保つ
- `inspect` / `to_string` は key を quoted string で表示し、空 map は `hash![]` と表示する

`hash![key => value, ...]` は `HashMap::from_entries` へ lower される生成 literal で、key は `String` 型を得られる任意の式です。

## 10. `Result` module の位置づけ

`Result` module は constructor contract と、よく使う variant 判定 helper の置き場です。

```surtr
@builtin type Result<$T>
@builtin type Ok($T) -> Result<$T>
@builtin type Err(Error) -> Result<$T>
```

現時点でも中心は `Ok(...)`, `Err(...)`, `match`, `=?`, `|*>`, `|*|`, `|>=`, `>*`, `>=>` の言語構文と型規則ですが、
`Result::is_ok(...)` / `Result::is_err(...)` で variant 判定だけを簡潔に書けます。

## 11. `Option` module の位置づけ

`Option` は user-facing な補助 enum です。
`Some(value)` / `None` 相当の値を表せますが、Surtr の失敗伝播の主軸ではありません。

```surtr
defenum Option<$T> {
  Some($T),
  None,
}
```

`Option` は `=?` の対象ではありませんが、`|*>`、`|*|`、`|>=`、`>*`、`>=>` には `Option` 文脈の標準実装があります。
失敗伝播へ載せたい場合は `from::<Result>(value)`、値として分岐したい場合は `match` を使います。
`from::<Option>(value)` は `Err(_)` を `None` に畳み込む明示変換です。

## Public vs Hidden

`Process` / `Task` のような副作用系モジュールは、public helper と runtime/internal builtin を分けて読みます。  
`IO` と `Random` は現時点では public builtin をそのまま surface に出しており、hidden shim 経由ではありません。

- public API は通常の `def` / `impl Type` / `@doc` に現れる
- `@hidden __*` builtin は compiler/runtime 接続用で、利用者向け API 一覧には含めない

特に `|>` や trait helper の docs では public surface を正本とし、hidden builtin 名を直接使う前提にはしません。

## REPL Model

REPL は起動時に標準定義ソースと preload script を読み切った OnceRead universe で動きます。

- REPL 中の `include` は扱わない
- REPL 中の `import` は、起動時に読み込まれた固定 universe に対する既存 symbol の導入としては使える
- REPL 中の `defstruct` / `defenum` / `deftrait` / `impl` / `defmod` は増分 universe 更新を前提にしない
- trait impl 候補一覧や diagnostics は、その起動時 universe を前提に固定される

データ型の field で欠損を表したい場合は、`T?` または `Option<T>` を使います。
`T?` は `Option<T>` に下がる sugar です。

```surtr
user.nickname
|> from(Result)
|>= normalize_name
|> from(Option)
```

`Option<T>` field を `Result` パイプへ流すと、上のような往復変換が必要です。
`nickname: String?` も同じく `Option<String>` なので、この変換規則は変わりません。

## 12. `Facet` module の位置づけ

`Facet` は runtime の first-class value ではなく、compile-time にだけ存在する
path capability です。

```surtr
@builtin type Facet<$K, $S, $A, $T, $B>
```

読み方は次です。

- `S` は source の型
- `A` は focus の型
- `Facet<K, S, A, T, B>` は compiler-managed な path capability。`K/S/A` は path 導出、`T/B` は update-side slot を表す

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
- `Tuple._N` は `Facet<K, (...), ..., T, B>` が期待される場所で使うほか、同一スコープの local binding として deferred path に束縛できる
- `_0` 単体は使わない

enum variant path は `Enum.Variant` です。

```surtr
Expr.Add
Token.Ident
```

- selector は PascalCase 固定
- 実行時の値がその variant でなければ `Err(VariantMismatch(...))` になる

ネストした path は `/` または `Facet::chain` でつなぎます。

```surtr
User.profile / Profile.name
Facet::chain(User.profile, Profile.name)
```

chain 後の表示は canonical path に正規化されます。
`User.profile / Profile.name` は `User.profile.name` として扱われ、root path の
重複は表示に残りません。

### `value.segment` は read sugar

`value.segment` は path をその値に適用する sugar です。

```surtr
name = user.name
first = pair._0
```

これは概念的には次と同じです。

```surtr
name = Facet::view(User.name, user)
first = Facet::view(Tuple._0, pair)
```

### `Facet::view`

`Facet::view(facet, source)` は path の先を読みます。

```surtr
name = Facet::view(User.name, user)
first = Facet::view(Tuple._0, pair)
profile_name = Facet::view(User.profile / Profile.name, user)
```

返り値は path と source に応じて変わります。

- plain field / tuple path を plain value に適用すると plain `A`
- `source` が `Result<S>` なら `Result<A>`
- path に variant selector を含むと `Result<A>`

例:

```surtr
match Facet::view(Expr.Add, expr) {
  Ok(add_expr) => ...,
  Err(err) => ...,
}
```

### `Facet::set`

`Facet::set(facet, source, value)` は focus を置き換え、常に `Result<S>` を返します。

```surtr
user2 =? Facet::set(User.name, user, "bob")
pair2 =? Facet::set(Tuple._1, pair, 4)
user3 =? Facet::set(~user.name, "carol")
```

ネストした値も同じです。

```surtr
profile_name = User.profile / Profile.name
user2 =? Facet::set(profile_name, user, "bob")
```

### `Facet::over`

`Facet::over(facet, source, update_fun)` は現在値を見てから更新します。

```surtr
user2 =? Facet::over(User.name, user, {|name|
  Ok(name ++ "!")
})
user3 =? Facet::over(~user.name, {|name| Ok(name ++ "!")})
```

- `update_fun` は `A -> Result<A>` を返す必要がある
- `Err(...)` を返したらそのまま伝播する
- 返り値は常に `Result<S>`

focus が `Result<A>` のとき、`over` は `Ok(value)` の payload だけを更新します。
`Err(err)` の場合は updater を呼ばず、その field をそのまま残します。

### `Facet::over_result`

`Facet::over_result(facet, source, update_fun)` は `Result<A>` focus 全体を更新します。

- `update_fun` は `Result<A> -> Result<Result<A>>`
- `Ok(...)` と `Err(...)` の両方を明示的に作り直したい場面向け
- successful payload だけ触りたいなら `over` の方が軽い
- `~source.path` shorthand は source を伴う `Facet` API の第1引数だけで使える

### `Facet::case_*`

enum case path の最後の payload を直接更新したいときは `case_*` を使います。

- `Facet::case_set(facet, source, value)`
- `Facet::case_over(facet, source, update_fun)`

通常の `set` / `over` / `over_result` と同じく `Result<S>` を返しますが、
用途は「path の最後が enum case payload である」場合に絞られます。

### `Facet::bulk_update`

`Facet::bulk_update(source) { ... }` は、relative path ごとの Facet 更新を
改行区切りで並べる special form です。

- 返り値: `Result<S>`
- 許可される update 形: `set`, `over`, `over_result`, `case_set`, `case_over`
- nested path 形: `path { ... }`
- 通常 block ではないため、任意の式や `S -> Result<S>` updater は置けない
- `List.[expr]` は plain `Int`、`HashMap.[expr]` は plain `String` を要求する
- `const Facet<...>` に含める bracket segment は literal のみ

```surtr
updated =? Facet::bulk_update(user) {
  name <- set("taro")
  profile {
    nickname <- over_result({|name: Result<String>| Ok(name)})
  }
  score_by_kind.[kind] <- set(9)
}
```

### `Facet::chain`

`Facet::chain(outer, inner)` は 2 つの path を順につなぎます。`outer / inner` は同じ意味の operator sugar です。

```surtr
profile_name = Facet::chain(User.profile, Profile.name)
name = Facet::view(profile_name, user)
```

型の並びは次です。

- `outer: Facet<K, S, A, _, _>`
- `inner: Facet<L, A, B, _, _>`
- result: `Facet<K, S, B, _, _>`

### Facet のスコープ規約

`Facet` は同一スコープ内でのみ使用可能です。

```surtr
facet = User.name
name = Facet::view(facet, user)
```

REPL では `:type` / `:info` に加えて `:facet <FacetPath|binding>` が使えます。
`type` と `full path` の確認に加えて、variant selector や `Result` source を含む
path の停止点をまとめて見たいときに使います。

一方で、次はできません。

- 関数引数として渡す
- 関数から返す
- closure で capture する
- `List`, tuple, `Ok(...)`, `Err(...)` などの runtime container に入れる

関数境界を越えたいときは `Facet` 自体ではなく、`Facet::view(...)` 済みの値を渡します。

## 13. パイプ / bind 系と標準定義ソースの関係

標準定義ソース側から見ると、各演算子との対応は次です。

| 構文 | 標準 surface / 役割 |
|---|---|
| `x |> f(1)` | call 式への第一引数注入 |
| `list |*> f()` | `List::map` と同じ方向の変換 |
| `mapper |*| value` | `Applicative::apply` による文脈内 callable の適用 |
| `list |>= f()` | `List` の bind 方向の変換 |
| `&f >* &g` | 文脈付き関数の後ろに pure function をつなぐ lifted compose |
| `&f >=> &g` | `List` または `Result` を返す関数どうしの Kleisli 合成 |

重要なのは、compose 系の実装詳細ではなく surface contract です。

- apply 系は call 式でも書ける
- compose 系は closure value を要求する
- `List` は helper surface を公開する
- `Result` は言語構文中心だが、variant 判定 helper も持つ
