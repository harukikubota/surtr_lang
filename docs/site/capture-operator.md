# キャプチャ演算子 `&`

Surtr の `&` は、既存の関数、method、データ型コンストラクタを
「あとで呼べる関数値」に変える演算子です。
このページでは bare capture と placeholder capture の両方をまとめます。

## 構文一覧

| 対象 | 構文パターン | 指すもの | 例 |
|---|---|---|---|
| 関数 | `&<関数>` / `&<修飾名>::<関数>` | 名前付き関数、method | `&add`, `&String::trim` |
| 関数値変数 | `&<関数値変数>` | 関数値変数を部分適用 | `&f(&1, 10)` |
| コンストラクタ | `&<型>` / `&<型>::<variant>` | Record、Struct、Enum variant のコンストラクタ | `&User`, `&Direction::Move` |
| コンストラクタ（引数指定） | `&<constructor>(<placeholder>, <固定値>, ...)` | 一部の引数を固定したコンストラクタ callable | `&User(&1, 20)`, `&Direction::Move(&1, 0)` |
| FacetPath | `&<FacetPath>` | 既存の FacetPath を callable にしたもの | `&User.name`, `&Tuple._0`, `&List.[idx]` |


## 先に覚えるルール

- `&f` は named capture です
- `&Type::method` や `&`Type::method`` も capture できます
- `&User` や `&Direction::Move` は constructor capture です
- `&`+`` のような operator capture もできます
- 引数位置を調整したいときは `&1`, `&2`, ... を使います
- `&f(10)` のような旧 partial capture は使えません
- placeholder は outermost な capture の中だけで有効です
- `&(&1)` や `&(&1 + &2)` のような anonymous capture は使えません

## named capture

もっとも基本の形は `&name` です。

```surtr
def add1(x: Int) -> Int { x + 1 }

f = &add1
print(to_string(f(41)))
```

qualified path も同じです。

```surtr
trim = &String::trim
show = &User::get_name
negate = &`Boolean::not`
```

これは「その関数そのものを値として取り出す」と読むと分かりやすいです。

## 部分適用

関数値に placeholder を含む引数を付けて capture すると、部分適用になります。
関数値の origin は問いません。名前付き関数、closure、capture、関数値変数のいずれにも
同じ構文を使えます。

```surtr
def add(x: Int, y: Int) -> Int { x + y }

by_add: (Int, Int -> Int) = &add
inc: (Int -> Int) = &add(&1, 1)
inc_from_value: (Int -> Int) = &by_add(&1, 1)
```

`&f` は、`f` の解決先によって振る舞いが変わります。

- `f` が名前付き関数なら、関数を capture して関数値にします。
- `f` が関数値変数なら、保持している関数値をそのまま参照します。
- どちらの場合も、`&f(&1, ...)` のように placeholder を付ければ部分適用になります。

## データ型コンストラクタ capture

Record、Struct、Enum variant のコンストラクタも関数値として capture できます。
標準の `Result` と `Boolean` も同じ規則を使います。

```surtr
defrecord User(name: String, age: Int)

make_user: (String, Int -> User) = &User
user = make_user("Ada", 20)
```

引数なしの `&User` は、コンストラクタが宣言しているすべての引数を受け取ります。

- Record は field の宣言順
- Struct は field 順ではなく `Type::new` の引数宣言順
- Enum variant は payload の宣言順

引数の一部を固定したり、順序を変えたりするときは通常関数と同じ placeholder を使います。

```surtr
make_twenty: (String -> User) = &User(&1, 20)

defenum Direction {
  Still,
  Move(Int, Int),
}

move_x = &Direction::Move(&1, 0)
swap_move = &Direction::Move(&2, &1)
```

placeholder 番号は生成される callable の入力順です。constructor へ値を渡す位置は宣言順のままなので、
`&Direction::Move(&2, &1)` は 2 個の入力を入れ替えて `Move` へ渡します。

payload を持たない Enum variant は、0 引数 callable として capture します。variant 値を作るには
capture を呼び出します。

```surtr
still: (-> Direction) = &Direction::Still
value = still()
```

### generic Enum

generic Enum では、通常の constructor call と同じ owner 型引数を指定できます。

```surtr
defenum Either<$L, $R> {
  Left($L),
  Right($R),
}

left: (String -> Either<String, Int>) = &Either<_, Int>::Left
left_fixed: (String -> Either<String, Int>) = &Either<_, Int>::Left(&1)
```

`_` は位置ごとに独立して推論されます。binding annotation や高階関数が要求する callable 型から
決定できなければ compile error になります。外側の generic 宣言ですでに導入されている `$T` も
owner 型引数に使えます。`::<...>` は関数の ReturnTypeArgument 用なので、Enum owner 型引数には
使いません。

### constructor capture の制限

constructor capture の引数は位置指定だけです。通常の Record / Struct call で named argument を
使える場合でも、capture 内では使えません。

```surtr
&User(name: &1, age: 20) # compile error
```

引数ブロックを書く場合は、1 個以上の placeholder が必要です。

```surtr
&User("Ada", 20) # compile error
&User(&1, 20)    # OK
```

固定した引数式は capture 作成時ではなく、生成された callable を呼ぶたびに、通常の constructor
引数と同じ左から右の順序で評価されます。

`deferror` などコンパイラが構築を管理する型は constructor capture できません。List、HashMap、
3 要素以上の tuple は literal で構築するため、nominal constructor capture の対象外です。2-tuple は
既存の ``&`(,)` `` を使います。

### Result と Boolean

`Ok` / `Err` / `True` / `False` も、通常の Enum variant と同じ規則で capture できます。

```surtr
wrap: (Int -> Result<Int>) = &Ok
wrap_placeholder: (Int -> Result<Int>) = &Ok(&1)
yes: (-> Boolean) = &True
no: (-> Boolean) = &False
```

成功型は payload や期待型から推論されます。`err = Err(NoneError)` の成功型は
多相のまま保持できますが、capture の callable binding には具体的な signature が必要です。

`Err` の payload は具象 `deferror` に限られます。`&Err` や `&Err(&1)` は
通常 callable の引数へ `Error` を公開するため拒否されます。具象 error を生成する式を
capture 内に固定し、通常の値だけを入力へ公開することはできます。

```surtr
deferror InvalidValue(value: Int) { to_string(value) }
fail: (Int -> Result<Int>) = &Err(InvalidValue(&1))
```

`Result<T>` の失敗枝に収まる Error は、通常 callable の Error 入出力公開には該当しません。

## operator capture

operator も backtick 経由で capture できます。

```surtr
add: (Int, Int -> Int) = &`+`
eqv = &`Boolean::eqv`
```

`&`+`` は 2 引数 callable を作ります。

```surtr
print(to_string(add(1, 2)))
print(to_string(eqv(True, False)))
```

placeholder capture と組み合わせることもできます。

```surtr
inc = &`+`(&1, 1)
flip_sub = &`-`(&2, &1)
```

## placeholder capture

引数の一部を固定したいときは placeholder capture を使います。

```surtr
def add(x: Int, y: Int) -> Int { x + y }
def sub(x: Int, y: Int) -> Int { x - y }

inc = &add(&1, 1)
dec = &sub(&1, 1)
flip_sub = &sub(&2, &1)
```

`&1` から `&16` は「この capture が受け取る引数の何番目か」を表します。

```surtr
inc(41)       # => add(41, 1)
flip_sub(2, 7) # => sub(7, 2)
```

placeholder の規則は次です。

- index は `1` から始まります
- index の上限は `16` です
- 最大 index が、その capture の引数個数になります
- index は欠番なく連続していなければなりません
- 同じ index は複数の引数位置で使えます

たとえば次は OK です。

```surtr
&ensure(&1, &pred, err)
&pair(&1, &2)
&sub(&2, &1)
&`+`(&1, 10)
```

次は不許可です。

```surtr
&add(&2, 10)   # `&1` がない
&add(&1, &3)   # `&2` がない
&add(&17, 10)  # index の上限を超える
```

## FacetPath　capture
`{|user: User| Facet::view(User.name, user) }` の糖衣構文として `&User.name` が使用できます。

## Lazy・Pattern・ErrorKind・Facet引数

引数固有の構文・型制約は、キャプチャでも適用されます。
標準Lazy関数は`&and(&1, &2)`など、引数を記述した呼び出しをキャプチャしてください。裸の`&and`などは拒否します。

| 引数位置 | 直接プレースホルダ |
|---|---|
| 通常の値 | 通常の要求型で受け取る |
| `Lazy<T>` | 正規化後の0引数関数型で受け取る |
| Pattern | 禁止。呼び出し内へ Pattern を直接書く |
| bindingを作るPatternの成功branch | 成功 scope に直接書く Expr。通常データの仮引数参照を許可 |
| `ErrorKind` | 禁止。具体的な `deferror` 型名を直接書く |
| `Facet<...>` | 禁止。具体的な FacetPath または外側の path binding を指定する |

```surtr
choose = &if(&1, &2, {|| 0})
# (Boolean, (-> Int) -> Int)
choose(True, {|| 42}) # 42

both = &and(&1, &2)
# (Boolean, (-> Boolean) -> Boolean)
both(True, {|| False}) # False

check: (Result<Int> -> Boolean) = &is_match(&1, Ok(_))
pick: (List<Int> -> Result<Int>) = &apply_pattern(&1, [_1, .._])
add_on_success = &if_let(&1, Ok(x), x + &2, 0)
# 第2引数は Int
```

`is_match`・`apply_pattern`・`if_let`・`if_let_then` は、Pattern を直接記述した完全な call をキャプチャできます。
Pattern がない bare capture と、consumer 自体の一般値参照は禁止です。
`Regex::matches` は通常関数なので、この Pattern consumer の規則には分類しません。

```surtr
&is_match(&1, &2)                     # compile error: Patternの直接置換
&is_match                            # compile error: Pattern未指定
&Result::recover_kind(&1, &2, &3)     # compile error: ErrorKindの直接置換
&Result::recover_kind(&1, NoneError, &2) # OK: 型名を固定
```

Lazy 位置の `&N` と `(&N)` は同じです。固定式の括弧による eager 境界と区別します。
固定式 `(EXPR)` はキャプチャの引数が導入される前のローカルコンテキストを参照するため、今回の `&N` を含めると compile error になります。
たとえば `&if_let(Ok(1), Ok(x), x + &1, 0)` の `&1` は `Int` ですが、`(x + &1)` にすると拒否します。
通常の引数や `match` arm 内の括弧は grouping であり、この事前評価境界にはなりません。
同じ番号を再使用した場合、すべての位置の要求型が一致しなければなりません。
たとえば `&and(&1, &1)` は `Boolean` と `(-> Boolean)` が衝突するため拒否します。
両 branch が未知の `&if(flag, &1, &2)` は、外側の型注釈または期待される関数型が必要です。

Pattern と成功 branch を本体に持つ生成関数値は、通常の引数・戻り値・変数として渡せます。
binding は call ごとの照合成功時に束縛され、成功 branch 内だけで参照できます。
Pattern 内の事前 Expr にある既存プレースホルダ、projection の `_1`〜`_16`、キャプチャの `&1`〜`&16` は別の役割です。
特殊ブロック内部へ新たにプレースホルダを許可する規則ではありません。

Lazy位置を直接プレースホルダにすると、通常の呼び出しで渡せる値と、生成された関数が受け取る型は異なります。
たとえば `and(True, False)` は有効ですが、上の `both`には `both(True, {|| False})` と渡します。
`Lazy<Error>`の正規化型は `(-> Error)` ですが、Errorを通常の関数型へ公開する制約は解除されません。
`require`・`ensure`・`Result::map_err`・`Result::cause`を通常の関数値として使うキャプチャでは、error式を呼び出し内へ固定してください。
引数の並べ替えも型に反映され、`&and(&2, &1)` の型は `((-> Boolean), Boolean -> Boolean)` です。

FacetPath 自体をプレースホルダ仮引数で受け取ることは禁止します。直接の置換や、合成の path 部分の置換もできません。
`[expr]` 内のプレースホルダは、index / key などの通常データを受け取って埋め込むため、使用できます。
外側の path binding はキャプチャ内で参照でき、source・更新値・更新関数には通常のプレースホルダを使えます。
`compose` は FacetPath を返すため、キャプチャできません。合成した path を Facet API で消費することはできます。

```surtr
p = Duration.millis
read = &Facet::view(p, &1)                  # OK: path を固定
&Facet::view(&1, 10ms)                      # compile error: FacetPath の置換
read_at: (Int, List<Int> -> Result<Int>) = &Facet::view(List.[&1], &2)
read_at(1, [10, 20])                       # Ok(20)
&Facet::view(&1 -> p, &2)                  # compile error: path 部分の置換
```

共通の正規化規則と、関数ごとのシグネチャ・実行条件は [Lazy evaluation](./lazy-evaluation.md#関数ごとのlazy引数) を参照してください。

## outer capture だけで使える

placeholder は outermost な capture にだけ属します。

```surtr
&outer(&1, &pred)
```

このとき `&pred` のような named capture は使えます。
ただし nested capture argument block の中へ placeholder を持ち込むことはできません。

```surtr
&outer(&1, &inner(10))  # compile error
```

## inferred field/facet capture

`_.path` は、期待される関数型から source 型を推論する unary capture です。

```surtr
users |*> _.name
users |*> _.profile.name
pairs |*> _._0
List::sort_by(users, &compare `Function::on` _.age)
```

`_.path` は単独の値ではなく、`A -> B` のような unary function context でだけ
型解決されます。source 型が文脈から決まると、明示形の `&Type.path` と同じ
field / Facet ルールで解決されます。

```surtr
users |*> _.age
users |*> &User.age
```

文脈がない場所では compile error になります。

```surtr
name = _.name  # compile error
```

この制約は「`&1` がどの capture に属するか」を明確に保つためです。

## 旧 partial capture は廃止

以前のような prefix partial application は使いません。

```surtr
&add(10)  # compile error
```

代わりに、placeholder で位置を明示します。

```surtr
&add(&1, 10)
&add(10, &1)
```

この形にそろえることで、「何番目の引数が後から入るか」が source 上で見えるようになります。

## anonymous capture は使えない

`&(...)` の形で式全体を直接 capture することはできません。

```surtr
&(&1)
&(&1 + &2)
&(print("Hello"))
```

identity がほしいだけなら named function を使います。

```surtr
&id
```

式を関数値にしたいなら、named helper を切り出すか closure を使います。

```surtr
{|x| x + 1}
{|text| print(text)}
```

## closure とどう使い分けるか

capture が向く場面:

- 既存関数をそのまま渡したい
- データ型コンストラクタを関数値として渡したい
- 引数位置だけを placeholder で調整したい
- module / type method を短く書きたい

closure が向く場面:

- その場で新しい処理を書きたい
- 外側のローカル変数を組み合わせたい
- 複数文のロジックを書きたい

たとえば次の 3 つは近い用途ですが、書き味が少し違います。

```surtr
users |*> &User::get_name
users |*> &format_name(&1, suffix)
users |*> {|user| format_name(user, suffix)}
```

## よくある不許可

```surtr
&1
&add(10)
&(&1 + &2)
&outer(&1, &inner(10))
```

理由は次です。

- `&1` 単体は capture の本体を持たない
- `&add(10)` は旧 partial capture だから不許可
- `&(...)` は anonymous capture だから不許可
- nested capture argument block の中では outer placeholder は見えない

## 例

```surtr
def add(x: Int, y: Int) -> Int { x + y }
def wrap(value: String, left: String, right: String) -> String {
  left ++ value ++ right
}
defrecord User(name: String, age: Int)

inc = &add(&1, 1)
bracket = &wrap(&1, "[", "]")
adder: (Int, Int -> Int) = &`+`
make_user: (String -> User) = &User(&1, 20)

print(to_string(inc(41)))
print(bracket("name"))
print(to_string(adder(1, 2)))
user = make_user("Ada")
```

## 関連ページ

- 関数コールと関数値の総論: `./callables.md`
- パイプ apply / map / bind: `./pipe-operators.md`
- 関数演算子の一覧: `./function-operators.md`
