# 関数演算子

Surtr には、値の流し込み、文脈付き計算、関数合成を短く書くための関数演算子があります。
このページでは `|>`, `|*>`, `|>=`, `>>`, `>*`, `>=>`, `=?` をまとめて引けるようにします。

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

## `|*>` 文脈 map

`|*>` は `Result` または `List` の中身だけを pure function で変換します。

```surtr
Ok(1) |*> add(2)
["a", "b"] |*> String::trim()
```

型の読み方:

- `Result<A> |*> (A -> B) -> Result<B>`
- `List<A> |*> (A -> B) -> List<B>`

`Result` のときは `Err` をそのまま通します。  
右辺は plain function である必要があり、`A -> Result<B>` は受けません。

## `|>=` 文脈 bind

`|>=` は文脈を保ったまま次の段階へ渡します。

```surtr
try_from("42", Int) |>= require_at_least(10)
[1, 2, 3] |>= expand()
```

型の読み方:

- `Result<A> |>= (A -> Result<B>) -> Result<B>`
- `List<A> |>= (A -> List<B>) -> List<B>`

`Result` なら `Err` を伝播し、`List` なら各要素から返った `List` をつなげるイメージです。

## `>>` 通常関数合成

```surtr
pipeline = &trim >> &render
```

型の読み方:

- `(A -> B) >> (B -> C) -> (A -> C)`

compose なので、`trim() >> render()` のような call 式は不許可です。

## `>*` Lifted 合成

`>*` は文脈を返す関数の後ろへ pure function をつなぎます。

```surtr
pipeline = &parse >* &render
```

型の読み方:

- `(A -> Result<B>) >* (B -> C) -> (A -> Result<C>)`
- `(A -> List<B>) >* (B -> C) -> (A -> List<C>)`

これは「`f >* g` は `x` に対して `f(x) |*> g`」と読むと分かりやすいです。

## `>=>` Kleisli 合成

`>=>` は文脈を返す関数同士を直列接続します。

```surtr
pipeline = &parse >=> &validate
```

型の読み方:

- `(A -> Result<B>) >=> (B -> Result<C>) -> (A -> Result<C>)`
- `(A -> List<B>) >=> (B -> List<C>) -> (A -> List<C>)`

これは「`f >=> g` は `x` に対して `f(x) |>= g`」に対応します。

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

## よくある不許可

```surtr
value |> normalize          # 不可
pipeline = parse >=> check  # 不可
parse() >=> check()         # 不可
f = &`+`                    # 未実装
```

理由は次です。

- 裸の関数参照は関数値として扱わない
- compose は関数の「実行結果」ではなく関数値同士をつなぐ
- operator capture や placeholder capture はまだ未実装

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
- 言語全体の仕様一覧: `./language-reference.md`
- 例を多めに見たいとき: `./language-guide.md`
