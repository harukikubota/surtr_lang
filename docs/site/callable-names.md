# 関数名と呼出し構文

関数は `名前(引数)` で呼び出します。このページでは、呼びたい関数の名前の指定方法と、関数宣言・関数値に応じた呼出し方を説明します。

## 関数を名前で呼ぶ

`def` で定義した関数は、その名前の後に引数を並べて呼び出します。

```surtr
def sum_pair(left: Int, right: Int) -> Int { left + right }

sum_pair(2, 3) # 5
```

モジュールや型、トレイトに属する関数は、`所属先::関数名` で指定できます。

```surtr
String::trim("  hello  ") # "hello"
Add::add(2, 3)           # 5
```

`print` や `to_string` など、よく使う標準関数は最初から短い名前で呼べます。それ以外の関数を短い名前で使うときは、ファイルの宣言部分に `import` を書きます。

```surtr
import Add::add

add(2, 3) # 5
```

同名の関数を両方使うときは、同じ名前で `import` せず、`所属先::関数名` で呼び分けます。同名の取り込みが重なるとエラーになります。取り込み方と重複の規則は [import と include](./language-features.md) を参照してください。

## 関数値を変数や引数から呼ぶ

関数値を入れた変数も、`変数名(引数)` で呼び出します。既存の関数を関数値にするには `&` を付けます。

```surtr
def sum_pair(left: Int, right: Int) -> Int { left + right }

operation = &sum_pair
operation(2, 3) # 5

double = {|value: Int| value * 2}
double(3)      # 6
```

関数の引数として受け取った関数値も同じ書き方です。引数と外側の関数が同じ名前なら、関数本体では引数を使います。

```surtr
def transform(value: Int) -> Int { value + 1 }
def run(transform: (Int -> Int)) -> Int { transform(3) }

run({|value: Int| value * 2}) # 6
transform(3)                 # 4
```

関数値の作り方と型注釈は [関数コールと関数値](./callables.md) で説明しています。

## 2つの引数の間に関数名を書く

2引数の関数は、名前をバッククォートで囲んで中置呼出しにできます。左側の値が第1引数、右側の値が第2引数です。

```surtr
def sum_pair(left: Int, right: Int) -> Int { left + right }

2 `sum_pair` 3 # sum_pair(2, 3) と同じく 5
2 `Add::add` 3 # Add::add(2, 3) と同じく 5
```

この書き方を使えるのは、関数名を指定するときです。関数値を保存した変数や、関数値を受け取る引数には使えません。

```surtr
operation = {|left: Int, right: Int| left + right}

operation(2, 3)   # 5
# 2 `operation` 3 # エラー: operation は変数
```

内側の変数や引数が同名の関数を隠している場合も、中置呼出しにはできません。`operation(2, 3)` のように括弧で引数を渡します。

複数の演算子と組み合わせるときの優先順位は [関数演算子](./function-operators.md) を参照してください。

## 自分の関数や変数に名前を付ける

`sum_pair` や `total_price` のように、処理や値の意味が分かる名前を付けます。`def`、`match`、`do` など、構文に使うキーワードは名前に使えません。

次の標準関数名も予約されているため、新しい関数・変数・引数・フィールドの名前には使えません。モジュールの中で定義するときも同じです。

| 用途 | 予約されている関数名 |
|---|---|
| 値の比較 | `eq`、`neq`、`lt`、`lte`、`gt`、`gte` |
| 論理演算 | `and`、`or` |
| 関数の引数への前処理 | `on` |
| パターンとの照合 | `if_let`、`if_let_then`、`is_match`、`apply_pattern` |

標準関数の使い方は [標準ライブラリの案内](./standard-modules.md)、自分の型に比較などの機能を持たせる方法は [トレイト実装](./trait-impls.md) を参照してください。
