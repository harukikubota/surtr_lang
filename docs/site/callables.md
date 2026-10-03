# 関数コールと関数値

Surtr では、見た目が似ていても次の 4 つは役割が違います。

- call 式: `add(1, 2)`
- capture: `&add`, `&User`, `&User::get_name`, `&add(&1, 10)`, `&`+``, `&`Boolean::not``
- closure: `{|x| x + 1}`
- backtick FuncLiteral: ``1 `add` 2``, ``1 `+` 2``

このページでは「いつ値になるか」「どこで呼ばれるか」をまとめます。

## 先に覚えるルール

- 裸の関数名は関数値になりません
- 関数値がほしいときは `&...` か closure を使います
- `add(1, 2)` は call、`&add` は capture です
- backtick FuncLiteral は中置 call の書き換えであり、関数値にはなりません
- compose 系演算子 `>>`, `>*`, `>=>` は call ではなく関数値を要求します
- unqualified infix `` `on` `` は常に `Function::on` を呼びます
- closure / capture 内の trait helper は、期待 callable 型がある場所まで解決を遅延できます
- local callable を変数へ束縛するときは concrete な signature が必要です
- closure / capture を高階関数へ直接渡すときは、その expected callable type から導出できれば注釈を省略できます

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

local callable は変数へ束縛する境界で signature を確定します。後続の call-site ごとに別の型へ generalize しません。

```surtr
int_id: (Int -> Int) = {|x| x}
value = int_id(1)
```

高階関数へ直接渡す場合は、引数の expected type がその場で型を決められます。

```surtr
def apply(value: $A, f: ($A -> $A)) -> $A { f(value) }
number = apply(1, {|x| x})
text = apply("surtr", {|x| x})
```

generic 関数の capture も同じです。`&identity` を変数へ置くなら concrete な callable 注釈が必要ですが、`apply(1, &identity)` のような直接引数では expected type から具体化できます。

一方で、compose 系が欲しいのは「実行結果」ではなく「あとで呼べる値」です。

```surtr
pipeline = &trim >> &render   # OK
pipeline = trim() >> render() # NG
```

## apply 系での call 式

`|>`, `|*>`, `|>=` の右辺では、call 式に左辺値が第 1 引数として注入されます。
`|*|` は call 式への注入ではなく、文脈内 callable と文脈内 value の適用です。

```surtr
value |> add(1)               # => add(value, 1)
user |> User::get_name()      # => User::get_name(user)
Ok("42") |*> String::trim()   # => Ok(String::trim("42"))
Ok(11) |>= require_at_least(10)
Ok(&inc) |*| Ok(1)
```

複数引数でも同じです。

```surtr
value |> wrap("[", "]")       # => wrap(value, "[", "]")
```

関数を返す call 式を apply したいときは括弧で明示します。

```surtr
value |> (make_normalizer(10))
```

これは `make_normalizer(10)(value)` の意味です。括弧内を一度評価し、得られた関数値へ入力を渡します。`|*>` と `|>=` でも同じで、括弧内の評価は要素ごとに繰り返しません。

括弧は期待型も内側の式へ伝えます。これはパイプに限らず、通常の引数、型注釈、関数の返り値、分岐内でも同じです。`always(10)` のように入力を使わない関数値も、使用側の関数型に沿って結果型を推論します。

## capture 演算子 `&`

`&` は関数、データ型コンストラクタ、method、operator surface を「あとで呼べる関数値」にします。

```surtr
inc = &add(&1, 1)
show_name = &User::get_name
trim = &String::trim
negate = &`Boolean::not`
adder: (Int, Int -> Int) = &`+`
```

ユーザ定義の Record、Struct、Enum variant も constructor capture の対象です。`&User` は
宣言順の引数を受け取り、`&User(&1, 20)` は placeholder を使う callable を作ります。generic Enum は
`&Either<_, Int>::Left` のように owner 型引数を明示できます。詳しい arity、推論、拒否規則は
[capture 演算子の詳細](./capture-operator.md) を参照してください。

関数値の推論型では `Result<T>` と表示します。`Result<T, E>` は関数定義の直接の戻り値位置だけに書ける補助的な error contract 表記です。ExtractorClosure の signature では `MatchResult<T>` と表示し、内部の `Error` は省きます。関数宣言の戻り値に明示した `Result<T, E>` と named `defextractor` の戻り値に明示した `MatchResult<T, Error>` は、`:sig` / `:doc` などの REPL コマンドで宣言シグネチャを照会したときに表示します。ExtractorClosure の型注釈では Error を指定できません。

REPL では capture を `FnCapture(module: Function, name: id, sig: (Int -> Int))` のように表示します。module とトレイトの所属名前空間を保ち、暗黙の `Global::` は省略します。

読み方は次です。

- `&add` は既存関数そのものを捕まえる
- `&add(&1, 1)` は placeholder を使って unary callable を作る
- `&User::get_name` は qualified method capture
- `&`Boolean::not`` は backtick 付きの qualified capture
- `&`+`` は 2 引数 operator callable

例:

```surtr
def add(x: Int, y: Int) -> Int { x + y }

inc = &add(&1, 1)
print(to_string(inc(41)))

names = users |*> &User::get_name
print(to_string(adder(1, 2)))
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
{ "ready" }
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

`{ ... }` はゼロ引数 closure です。即時評価される block 式ではありません。

```surtr
block = {
  tmp = 10
  tmp * 10
}

print(to_string(block()))
```

`match expr { pattern => expr, ... }` と `cond { cond => expr, ... }` の braces は
`=>` を持つ専用構文のコンテナで、closure literal ではありません。

closure は周囲の値を capture します。

```surtr
suffix = "!"
excited = {|name| name ++ suffix}
print(excited("alice"))
```

generic 引数がまだ決まっていなくても、closure や literal を直接渡せます。compiler は actual expression の shape から先に型を得て、generic slot と照合します。

```surtr
boxed = Box({|n: Int| n + 1})
values = wrap([1, 2, 3])
```

通常 call、constructor、Trait helper、apply、compose は同じ引数推論規則を使います。expected type が既知なら list の各要素、tuple の各 slot、`if` の全 branch、`match` の全 arm へ伝播します。空 collection など式だけでは型を一意にできない場合は、引き続き型注釈が必要です。

Trait helper を generic receiver に使う場合の必要な `where` bound は、[`trait-system.md`](./trait-system.md) を参照してください。

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

backtick FuncLiteral は「関数値」ではなく、引用された callee を表す補助構文です。

```surtr
10 `+` 5
7 `eq` 7
left `concat` right
`Add::add`(1, 2)
`+`(1, 2)
```

意味は次です。

- ``left `name` right`` は `name(left, right)`
- ``left `+` right`` は通常の演算子と同じ
- `` `name`(args...) `` と `` `Type::method`(args...) `` は通常の call と同じ
- `` `+`(left, right) `` は通常の二項演算子と同じ
- unqualified ``left `on` right`` は `Function::on(left, right)` として扱います
- ``left `Function::on` right`` も同じ意味で、flow 演算子より低優先度です
- ``left `Other::on` right`` は通常どおり `Other::on(left, right)` です

`on`、`and`、`or`、`eq`、`neq`、`lt`、`lte`、`gt`、`gte` は予約名です。変数・引数・Patternの束縛名・フィールド名には使えませんが、関数の宣言名には使えます。import規則は変わりません。標準の `and` / `or` は短絡評価を維持し、裸の比較関数6名は比較演算子と同じ優先度です。`MyMod::and` などの修飾中置Callは通常のCallとして引数を評価します。

FuncLiteral は値にならないので、単独では置けません。

```surtr
f = `eq`      # NG
items |*> `+` # NG
`Boolean`::`eq`(True, False) # NG: path は全体を一組の backtick で囲む
```

これが必要なら capture や closure を使います。

```surtr
eq7 = &eq(&1, 7)
plus = {|x| x + 1}
```

### 現時点の制約

- backtick FuncLiteral 自体は値にならない
- bare operator capture を変数へ束縛するときは、必要に応じて型注釈や使用側の期待型で文脈を与える
- `&1` 単体や `&add(10)` のような prefix partial capture は不許可

## trailing block

call 式の最終引数がclosure なら、末尾へ外出しして書けます。

```surtr
Test::it("increments") {
  print("ok")
}

List::map([1, 1, 2, 3]) {|num| num + 1 }
```

これは通常の call の sugar です。constructor call には使いません。

## 関連ページ

- キャプチャ演算子の詳細: `./capture-operator.md`
- パイプ apply / map / bind: `./pipe-operators.md`
- 関数演算子のまとまった一覧: `./function-operators.md`
- 全体の読み物: `./language-guide.md`
- 制約を短く引く: `./language-reference.md`
