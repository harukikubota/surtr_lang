# パイプ演算子

Surtr には、値や文脈付きの値を左から右へ流すためのパイプ演算子があります。
このページでは `|>`, `|*>`, `|>=` と、右辺で使える `_1` placeholder をまとめます。

## 先に覚えるルール

- `|>` は plain apply です
- `|*>` は map です
- `|>=` は bind です
- 右辺が call 式なら、左辺値は第 1 引数へ注入されます
- `_1` は右辺 call の direct positional argument に 1 回だけ置けます
- `_1` は pipe の外では使えません

## `|>` plain apply

`|>` は左辺の値を右辺へ渡します。

```surtr
value |> &normalize
value |> normalize(10)
value |> {|x| normalize(x, 10)}
```

右辺が call 式なら、第 1 引数へ左辺値が注入されます。

```surtr
value |> normalize(10)   # => normalize(value, 10)
user |> User::get_name() # => User::get_name(user)
```

右辺として使えるのは主に次です。

- capture: `&normalize`
- closure: `{|x| x + 1}`
- 関数型の変数
- 括弧付きの関数値式: `(make_normalizer(10))`
- call 式: `normalize(10)`

## `|*>` map

`|*>` は `Result` や `List` の中身だけを pure function で変換します。

```surtr
Ok(1) |*> add(2)
[" a ", " b "] |*> String::trim()
users |*> &User::get_name
```

型の読み方:

- `Result<A> |*> (A -> B) -> Result<B>`
- `List<A> |*> (A -> B) -> List<B>`

`Result` のときは `Err` をそのまま通します。

## `|>=` bind

`|>=` は文脈を保ったまま次の段階へ渡します。

```surtr
try_from("42", Int) |>= require_at_least(10)
[1, 2, 3] |>= expand()
Ok(" 42 ") |*> String::trim() |>= try_from(Int)
```

型の読み方:

- `Result<A> |>= (A -> Result<B>) -> Result<B>`
- `List<A> |>= (A -> List<B>) -> List<B>`

`Result` なら `Err` を伝播し、`List` なら返ってきた `List` をつなげます。

## `_1` pipe placeholder

右辺 call の中で第 1 引数注入位置をずらしたいときは `_1` を使います。

```surtr
value |> add(10, _1)
value |> wrap("[", _1, "]")
Ok(10) |*> sub(100, _1)
```

意味は次です。

```surtr
value |> add(10, _1)       # => add(10, value)
value |> wrap("[", _1, "]") # => wrap("[", value, "]")
```

`_1` がある場合、その call では通常の「第 1 引数への自動注入」は行いません。
代わりに `_1` の位置へ左辺値が入ります。

## `_1` の制約

`_1` は使える場所がかなり限定されています。

- pipe RHS の最外 call の direct positional argument にのみ置ける
- 1 つの RHS call で 1 回だけ使える
- pipe の外では使えない
- tuple path の `pair._1` とは別物です

次は OK です。

```surtr
value |> add(10, _1)
value |*> wrap("[", _1, "]")
value |>= validate(_1, rule)
```

次は不許可です。

```surtr
value |> f(add(10, _1))
value |> f(_1, _1)
x = _1
```

最初の例が不許可なのは、`_1` が nested expression の中へ入っているためです。

## nested `_1` は前のパイプ段へ出す

`f(add(10, _1))` のような nested `_1` を見つけたときは、
まず内側の変換を前の pipe step へ出してください。

```surtr
value |> f(add(10, _1))
```

これは次のように分けます。

```surtr
value
|> add(10, _1)
|> f()
```

もう少し深くネストしていても、浅いうちは同じ方針で分解できます。

```surtr
value |> f(g(add(10, _1)))
```

```surtr
value
|> add(10, _1)
|> g()
|> f()
```

深すぎる場合は closure の方が読みやすいことがあります。

```surtr
value |> {|term| f(g(h(add(10, term))))}
```

## call 注入と `_1` の違い

次の 2 つは似ていますが意味が違います。

```surtr
value |> add(10)
value |> add(10, _1)
```

- `add(10)` は `add(value, 10)`
- `add(10, _1)` は `add(10, value)`

`_1` は「自動注入を止めて、どこへ左辺値を入れるかを明示する」と読むと分かりやすいです。

## `pair._1` との違い

tuple path の `_1` は field / lens path 側の surface です。
これは必ず `.` の直後に現れます。

```surtr
pair._1
```

一方、pipe placeholder の `_1` は pipe RHS の call argument としてだけ特別扱いされます。

```surtr
value |> add(10, _1)
```

そのため lexer では衝突せず、文脈で区別されます。

## compose との対応

apply 系と compose 系は対応しています。

- `|>` に対応する関数値側の組み立ては `>>`
- `|*>` に対応する関数値側の組み立ては `>*`
- `|>=` に対応する関数値側の組み立ては `>=>`

値から書き始めるなら apply 系、先に関数を組み立てるなら compose 系が向いています。

## 例

```surtr
def wrap(left: String, value: String, right: String) -> String {
  left ++ value ++ right
}

def neighbors(n: Int) -> List<Int> {
  [n - 1, n, n + 1]
}

print("name" |> wrap("[", _1, "]"))
print(to_string([3, 5] |>= neighbors()))
```

## 関連ページ

- capture / closure / call の総論: `./callables.md`
- capture 専用ページ: `./capture-operator.md`
- 関数演算子の一覧: `./function-operators.md`
