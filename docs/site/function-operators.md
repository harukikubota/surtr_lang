# 関数演算子

Surtr には、値の流し込み、文脈付き計算、関数合成を短く書くための関数演算子があります。
このページでは `|>`, `|*>`, `|>=`, `>>`, `>*`, `>=>`, `=?` をまとめて引けるようにします。
apply 系の詳説は `./pipe-operators.md`、capture 自体の詳説は `./capture-operator.md`、関数コールや closure の総論は `./callables.md` に分けています。

## 先に覚えるルール

- 裸の関数名は関数値になりません。関数値が欲しいときは `&name` を使います
- `|>` の右辺が call 式なら、左辺値が第 1 引数へ注入されます
- `>>`, `>*`, `>=>` は compose なので、左右とも関数値でなければなりません
- `|>`, `|*>`, `|>=`, `>>`, `>*`, `>=>`, `=?` は同一優先度、左結合です

## `|>` 値を流す

```surtr
value |> &normalize
value |> normalize(10)
user |> User::get_name()
4 |> {|x| x + 1}
```

読み下しは次です。

```surtr
value |> normalize(10)   # => normalize(value, 10)
user |> User::get_name() # => User::get_name(user)
```

右辺として使えるのは主に次です。

- capture: `&normalize`
- closure: `{|x| x + 1}`
- 関数型の変数: `normalizer`
- 括弧付きの関数値式: `(make_normalizer(10))`
- call 式: `normalize(10)`

たとえば次の 4 つはそれぞれ少し意味が違います。

```surtr
value |> &normalize
value |> normalize(10)
value |> {|x| normalize(x, 10)}
value |> (make_normalizer(10))
```

- `&normalize` は既存関数を unary callable として渡す
- `normalize(10)` は call 右辺への第一引数注入
- closure 版はその場でロジックを足せる
- 括弧付き call は「返ってきた関数値」へ適用する

実例:

```surtr
def wrap(value: String, left: String, right: String) -> String {
  left ++ value ++ right
}

def add(x: Int, y: Int) -> Int {
  x + y
}

print("name" |> wrap("[", "]"))
print(to_string(4 |> &add(&1, 1)))
print(to_string(4 |> {|x| x * 10}))
```

## `|*>` 文脈 map

`|*>` は `Result` または `List` の中身だけを pure function で変換します。

```surtr
Ok(1) |*> add(2)
["a", "b"] |*> String::trim()
users |*> &User::get_name
```

型の読み方:

- `Result<A> |*> (A -> B) -> Result<B>`
- `List<A> |*> (A -> B) -> List<B>`

`Result` のときは `Err` をそのまま通します。  
右辺は plain function である必要があり、`A -> Result<B>` は受けません。

```surtr
scores = [1, 2, 3] |*> add(10)
labels = [1, 2, 3] |*> {|n| "#" ++ to_string(n)}
```

## `|>=` 文脈 bind

`|>=` は文脈を保ったまま次の段階へ渡します。

```surtr
try_from("42", Int) |>= require_at_least(10)
[1, 2, 3] |>= expand()
Ok(" 42 ") |*> String::trim() |>= try_from(Int)
```

型の読み方:

- `Result<A> |>= (A -> Result<B>) -> Result<B>`
- `List<A> |>= (A -> List<B>) -> List<B>`

`Result` なら `Err` を伝播し、`List` なら各要素から返った `List` をつなげるイメージです。

```surtr
def neighbors(n: Int) -> List<Int> { [n - 1, n, n + 1] }

print(to_string([3, 5] |>= neighbors()))
```

## `>>` 通常関数合成

```surtr
pipeline = &trim >> &render
check = &String::trim >> {|text| text != ""}
```

型の読み方:

- `(A -> B) >> (B -> C) -> (A -> C)`

compose なので、`trim() >> render()` のような call 式は不許可です。

```surtr
def trim(text: String) -> String { String::trim(text) }
def render(text: String) -> String { "[" ++ text ++ "]" }

pipeline = &trim >> &render
print(pipeline("  alice  "))
```

## `>*` Lifted 合成

`>*` は文脈を返す関数の後ろへ pure function をつなぎます。

```surtr
pipeline = &parse >* &render
pipeline2 = &parse >* {|n| "#" ++ to_string(n)}
```

型の読み方:

- `(A -> Result<B>) >* (B -> C) -> (A -> Result<C>)`
- `(A -> List<B>) >* (B -> C) -> (A -> List<C>)`

これは「`f >* g` は `x` に対して `f(x) |*> g`」と読むと分かりやすいです。

```surtr
def parse_int(text: String) -> Result<Int> {
  try_from(text, Int)
}

def render_int(value: Int) -> String {
  "#" ++ to_string(value)
}

pipeline = &parse_int >* &render_int
```

## `>=>` Kleisli 合成

`>=>` は文脈を返す関数同士を直列接続します。

```surtr
pipeline = &parse >=> &validate
pipeline2 = &parse >=> {|n| require_at_least(n, 10)}
```

型の読み方:

- `(A -> Result<B>) >=> (B -> Result<C>) -> (A -> Result<C>)`
- `(A -> List<B>) >=> (B -> List<C>) -> (A -> List<C>)`

これは「`f >=> g` は `x` に対して `f(x) |>= g`」に対応します。

```surtr
def parse_int(text: String) -> Result<Int> {
  try_from(text, Int)
}

def require_small(x: Int) -> Result<Int> {
  if(x < 100, Ok(x), Err(NoneError))
}

pipeline = &parse_int >=> &require_small
```

## `=?` SafeBind

`=?` は失敗しうる値から成功側だけを束縛し、失敗はそのまま返す構文です。

```surtr
value: Int =? try_from("1", Int)
[head, ..tail] =? [1, 2, 3]
[first, ..rest] =? "source"
```

現時点での対象は次です。

- `Result`
- `List`
- `String`

`Option` は対象外なので、必要なら `Option::to_result` で `Result` へ変換してから使います。

## `Result` でよくある形

```surtr
def parse_int(text: String) -> Result<Int> {
  try_from(text, Int)
}

def require_small(x: Int) -> Result<Int> {
  if(x < 100, Ok(x), Err(NoneError))
}

def render(x: Int) -> String {
  to_string(x)
}

pipeline = &parse_int >=> &require_small >* &render

text = "42"
result = pipeline(text)
```

この形なら、`match` を深くネストせずに段階を左から右へ読めます。

`List` でも同じ読み方ができます。

```surtr
def duplicate(n: Int) -> List<Int> { [n, n] }
def label(n: Int) -> String { "item:" ++ to_string(n) }

pipeline = &duplicate >* &label
expanded = [1, 2, 3] |>= duplicate()
```

## よくある不許可

```surtr
value |> normalize          # 不可
pipeline = parse >=> check  # 不可
parse() >=> check()         # 不可
```

理由は次です。

- 裸の関数参照は関数値として扱わない
- compose は関数の「実行結果」ではなく関数値同士をつなぐ

一方で、operator capture と placeholder capture は使えます。

```surtr
f: (Int, Int -> Int) = &`+`
inc = &`+`(&1, 1)
```

## 迷ったときの選び方

- 値から始めるなら apply 系: `|>`, `|*>`, `|>=`
- 関数どうしを先に組み立てるなら compose 系: `>>`, `>*`, `>=>`

## 関連ページ

- パイプ apply / map / bind の詳細: `./pipe-operators.md`
- キャプチャ演算子 `&` の詳細: `./capture-operator.md`
- 関数コールと関数値の総論: `./callables.md`
- 右辺をその場で少し変えたいなら closure
- 既存関数をそのまま渡したいなら capture
- call と capture の違いで迷ったら `./callables.md`

## 演算子の対応表

- `value |> f` は「値を関数へ渡す」
- `ctx |*> f` は「文脈の中身だけを写す」
- `ctx |>= f` は「文脈を保って次へ渡す」
- `f >> g` は「通常関数の合成」
- `f >* g` は「文脈関数の後ろへ pure function をつなぐ」
- `f >=> g` は「文脈関数どうしを合成する」
- `pattern =? expr` は「失敗したらその場で伝播する束縛」

## どこを見るか

- `Result` 文脈での使い方: `./error-handling.md`
- 関数コール / capture / closure / FuncLiteral: `./callables.md`
- 言語全体の仕様一覧: `./language-reference.md`
- 例を多めに見たいとき: `./language-guide.md`
