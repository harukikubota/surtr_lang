# 関数コールと関数値

Surtr では、見た目が似ていても次の 4 つは役割が違います。

- call 式: `add(1, 2)`
- capture: `&add`, `&User::get_name`, `&add(&1, 10)`
- closure: `{|x| x + 1}`
- backtick FuncLiteral: ``1 `add` 2``, ``1 `+` 2``

このページでは「いつ値になるか」「どこで呼ばれるか」をまとめます。

## 先に覚えるルール

- 裸の関数名は関数値になりません
- 関数値がほしいときは `&...` か closure を使います
- `add(1, 2)` は call、`&add` は capture です
- backtick FuncLiteral は中置 call の書き換えであり、関数値にはなりません
- compose 系演算子 `>>`, `>*`, `>=>` は call ではなく関数値を要求します

## 関数コール

普通の関数呼び出しは `f(arg1, arg2, ...)` です。

```surtr
def add(x: Int, y: Int) -> Int { x + y }

print(to_string(add(1, 2)))
print(to_string(User::get_name(user)))
```

call はその場で実行され、結果の値を返します。

```surtr
sum = add(1, 2)              # Int
name = User::get_name(user)  # String
```

一方で、compose 系が欲しいのは「実行結果」ではなく「あとで呼べる値」です。

```surtr
pipeline = &trim >> &render   # OK
pipeline = trim() >> render() # NG
```

## apply 系での call 式

`|>`, `|*>`, `|>=` の右辺では、call 式に左辺値が第 1 引数として注入されます。

```surtr
value |> add(1)               # => add(value, 1)
user |> User::get_name()      # => User::get_name(user)
Ok("42") |*> String::trim()   # => Ok(String::trim("42"))
Ok(11) |>= require_at_least(10)
```

複数引数でも同じです。

```surtr
value |> wrap("[", "]")       # => wrap(value, "[", "]")
```

関数を返す call 式を apply したいときは括弧で明示します。

```surtr
value |> (make_normalizer(10))
```

これは `make_normalizer(10)(value)` の意味です。

## capture 演算子 `&`

`&` は関数や method を「あとで呼べる関数値」にします。

```surtr
inc = &add(&1, 1)
show_name = &User::get_name
trim = &String::trim
```

読み方は次です。

- `&add` は既存関数そのものを捕まえる
- `&add(&1, 1)` は placeholder を使って unary callable を作る
- `&User::get_name` は qualified method capture

例:

```surtr
def add(x: Int, y: Int) -> Int { x + y }

inc = &add(&1, 1)
print(to_string(inc(41)))

names = users |*> &User::get_name
```

`inspect(...)` すると bare capture の metadata を観察できます。

```surtr
print(inspect(&Boolean::xor))
```

## closure

closure はその場で作る関数値です。

```surtr
{|x| x + 1}
{|x: Int| x + 1}
{|| "ready"}
```

引数型注釈は任意です。

```surtr
add1 = {|x| x + 1}
render = {|user: User| User::get_name(user) ++ "!"}
```

複数文の本体も書けます。

```surtr
tap(3, {|n|
  print("seen")
  print(to_string(n))
})
```

closure は周囲の値を capture します。

```surtr
suffix = "!"
excited = {|name| name ++ suffix}
print(excited("alice"))
```

関数演算子の右辺にもそのまま置けます。

```surtr
4 |> {|x| x + 1}
Ok(3) |*> {|n| n * 10}
pipeline = {|x| parse(x)} >=> {|y| validate(y)}
```

## capture と closure の使い分け

capture が向く場面:

- 既存関数をそのまま渡したい
- placeholder capture で引数位置を明示したい
- module / type method を短く渡したい

closure が向く場面:

- その場で小さな処理を書きたい
- 外側の値を組み合わせたい
- 複数文の処理にしたい

たとえば次の 3 つは似ています。

```surtr
users |*> &User::get_name
users |*> {|user| User::get_name(user)}
users |> List::map(&User::get_name)
```

1 行目は最短、2 行目は変形しやすく、3 行目は helper surface を明示したいときに向きます。

## Backtick FuncLiteral

backtick FuncLiteral は「関数値」ではなく「中置 call の補助構文」です。

```surtr
10 `+` 5
7 `eq` 7
left `concat` right
```

意味は次です。

- ``left `name` right`` は `name(left, right)`
- ``left `+` right`` は通常の演算子と同じ

FuncLiteral は値にならないので、単独では置けません。

```surtr
f = `eq`      # NG
items |*> `+` # NG
```

これが必要なら capture や closure を使います。

```surtr
eq7 = &eq(&1, 7)
plus = {|x| x + 1}
```

### 現時点の制約

- V1 の FuncLiteral は unqualified name と symbolic operator のみ
- `` `Type::method` `` のような qualified path は未対応
- `&\`+\`` のような operator capture は未実装
- placeholder capture (`&1`, `&2`) は未実装

## trailing block

call 式の最終引数が block なら、末尾へ外出しして書けます。

```surtr
Test::it("increments") {
  print("ok")
}
```

これは通常の call の sugar です。constructor call には使いません。

## よくある迷いどころ

```surtr
value |> normalize           # NG
value |> &normalize          # OK
value |> normalize(10)       # OK

pipeline = parse >=> check   # NG
pipeline = &parse >=> &check # OK

1 `+` 2                      # OK
f = `+`                      # NG
```

見分け方は単純です。

- すぐ実行したいなら call
- あとで渡したいなら capture / closure
- 二項 call を中置で読みたければ FuncLiteral

## 関連ページ

- キャプチャ演算子の詳細: `./capture-operator.md`
- パイプ apply / map / bind: `./pipe-operators.md`
- 関数演算子のまとまった一覧: `./function-operators.md`
- 全体の読み物: `./language-guide.md`
- 制約を短く引く: `./language-reference.md`
