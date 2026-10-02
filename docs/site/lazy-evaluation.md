# Lazy evaluation と括弧

`Lazy<T>` は標準の special form の引数専用マーカーです。利用者の関数の引数・戻り値・型注釈には使えず、通常の値として保持できません。
コンパイラは Lazy 入力を0引数関数へ正規化し、選ばれた branch だけを一回呼び出します。

利用者の関数で遅延処理を受け取る場合は、通常の0引数関数型を書きます。

```surtr
def wrap_if(flag: Boolean, yes: (-> $A), no: (-> $A)) -> $A {
  if(flag, yes, no)
}
wrap_if(True, {|| 42}, {|| 0}) # 42
```

Lazyを消去したキャプチャも、このwrapperと同じ通常の関数値です。生成関数へ渡す引数は通常どおり評価済みの値で、0引数関数の本体は選択時に実行します。

## 裸のcallと括弧

Lazy 位置へ直接書いた call は、戻り値型にかかわらず一段包みます。
`if(flag, make_value(), fallback)` の `make_value()` は、その branch が選ばれるまで実行しません。
call が0引数関数を返しても、その戻り値の関数を追加で呼び出しません。

Lazy 位置の `(EXPR)` は eager 境界です。内側を先に一度評価し、得られた値を正規化へ渡します。
評価順序は各 special form の既存の順序に従います。

```surtr
def selected() -> Unit { print("selected") }
def eager() -> Unit { print("eager") }

if(True, selected(), (eager()))
# eager
# selected
```

`(make_block())` なら `make_block()` を先に実行します。そこで得たブロックの本体は、branch が選ばれた場合だけ実行します。
値の取得とブロック本体の実行は別の操作です。
クロージャリテラルの本体にある括弧は、その本体内のgroupingです。引数位置のeager境界にはなりません。

```surtr
def make_closure() -> (-> Int) {
  print("acquire")
  {|| print("execute"); 9}
}
if(False, (make_closure()), { (1 + 2) * 3 })
# acquire
# 戻り値は9。thenのクロージャ本体は実行しない
```

## branchの型と正規化

型の先頭に連続する0引数関数の段数を depth と呼びます。
`Int` は depth 0、`(-> Int)` は depth 1、`(-> (-> Int))` は depth 2 です。
引数付き関数はそれ自体が基底型なので、`(Int -> String)` は depth 0 です。

既知の branch 入力を、次の規則で共通の0引数関数型へそろえます。

- 同じ型で depth 0 なら、両側を一段包みます。
- 同じ型で depth 1 以上なら、そのまま使います。
- depth が異なるなら、浅い側だけを一段包みます。二段以上は包みません。
- 正規化した型は通常の型検査で照合します。基底型の暗黙変換は行いません。

```surtr
if(True, {|| {|| 1}}, {|| 1})
# 戻り値は (-> Int)。右側を一段包む

if(True, {|| 1}, {|| {|| 1}})
# 戻り値は (-> Int)。左側を一段包む

if(True, {|| {|| 1}}, 1)
# compile error: depth の差が二段あり、一段ではそろわない
```

call の戻り値型は、正規化した共通 branch を一回呼び出した型です。
戻り値に関数が残る場合は、その関数を通常の値として返し、追加では呼び出しません。
この説明上の「包む」は、すべての branch で runtime closure の生成を要求するものではありません。

## Lazy位置のキャプチャ

キャプチャの直接プレースホルダは、正規化後の0引数関数型を受け取ります。
既知 branch の型と depth から要求型が決まり、プレースホルダ自体を追加で包むことはありません。
既知branchがある場合、外側の型注釈でこの正規化や評価方法を変更することはできません。

| branch | プレースホルダの型 | callの戻り値型 |
|---|---|---|
| `&1, 1` | `(-> Int)` | `Int` |
| `&1, {|| 1}` | `(-> Int)` | `Int` |
| `&1, {|| {|| 1}}` | `(-> (-> Int))` | `(-> Int)` |

```surtr
choose = &if(&1, &2, {|| 0})
# Boolean, (-> Int) を受け取る
choose(False, {|| 42})
# 0。第2引数のブロック本体は実行しない
```

`&N` と `(&N)` は同じ仮引数参照です。この grouping に eager 境界や追加の wrap を適用しません。
固定式の `(EXPR)` には eager 境界が適用されます。

両 branch が未知なら、外側の型注釈か、期待される関数型が必要です。
REPL でもその行の完了時に具体的な signature を要求し、後続の call から決め直しません。
宣言済みのgeneric関数とは異なり、未確定のlocalキャプチャを新しい多相関数として保存する規則はありません。

```surtr
select: ((-> Int), (-> Int) -> Int) = &if(True, &1, &2)
```

同じ番号は一つの仮引数です。各位置の要求型が衝突する場合は拒否します。

```surtr
&and(&1, &2) # Boolean, (-> Boolean) を受け取る
&and(&1, &1) # compile error: Boolean と (-> Boolean) は一致しない
```

## 型エラーの案内

Lazyキャプチャの型不一致には、元の標準関数に応じた説明と修正案が付きます。
型注釈との不一致と、生成された関数への引数の不一致が対象です。
案内のシグネチャと引数番号には、プレースホルダの並べ替えと正規化後の型が反映されます。

`and`・`or`などでは0引数関数を渡す例、同じ番号の要求型が衝突する場合は番号を分ける例、両branchが未知の場合は具体的な期待関数型を与える例を示します。
Error系は既存のError制約に従い、error式をキャプチャ内へ固定する案内になります。
通常の型判定や評価規則は変わりません。入れ子の別callで起きたエラーや、Lazyキャプチャ由来と確定できない関数値には、通常の型診断を使います。

## Pattern bindingと成功branch

`if_let`・`if_let_then` は、binding を作らない Pattern なら通常の Lazy 規則を使います。
binding を作る Pattern の成功 branch は、その成功 scope に直接書く Expr です。
binding を実際に使うかどうかで規則を切り替えません。

```surtr
f = &if_let(&1, Ok(x), x + &2, 0)
# 第2引数は Int。成功時だけ x + 第2引数を評価する
```

成功 branch 内の括弧による eager 式は、照合前の scope で名前解決します。
外側に `x` がなければ、次は `UndefinedVariable` になります。

```surtr
if_let(Ok(1), Ok(x), (x + 1), 0)
```

成功 branch を外部 thunk で置き換えて、その lexical scope を後から変えることはありません。
成功 branch 内で作る通常の closure は、通常の規則に従って binding の値を保持できます。
Pattern自身がbindingとして束縛した関数値も、成功scopeの通常データとして参照できます。成功branchを外部thunkで置き換える操作とは区別します。
`match` arm の括弧は、その arm 内の grouping です。Lazy の eager 境界にはなりません。

## 関数ごとのLazy引数

通常の呼び出しでは、Lazy引数へ値や式を直接書けます。直接プレースホルダでキャプチャした位置には、正規化後の0引数関数を渡します。
以下のシグネチャは各例で生成される関数の型です。プレースホルダの順序を変えると、生成される引数の順序も変わります。
裸の`&and`や`&or`など、標準のLazy関数名だけを指定するキャプチャは使えません。`&and(&1, &2)`のように引数を書いてください。

### `and`

左辺が `True` の場合だけ右辺を実行し、左辺が `False` なら `False` を返します。
右辺のプレースホルダには `(-> Boolean)` が必要です。

```surtr
and(True, False) # False
both = &and(&1, &2)
# (Boolean, (-> Boolean) -> Boolean)
both(True, {|| False}) # False
```

`both(True, False)` は型エラーです。生成された関数には右辺の値ではなく、右辺を返す0引数関数を渡します。
`&and(&1, &1)` は、一つの仮引数に `Boolean` と `(-> Boolean)` を要求するため拒否します。左右を別々のプレースホルダにしてください。

### `or`

左辺が `False` の場合だけ右辺を実行し、左辺が `True` なら `True` を返します。
生成される型は `and` と同じですが、右辺を実行する条件が異なります。

```surtr
or(False, True) # True
either = &or(&1, &2)
# (Boolean, (-> Boolean) -> Boolean)
either(False, {|| True}) # True
```

`and`・`or`とも、通常の呼び出しで右辺を括弧で囲むと、その式を短絡判定前に評価します。

### `if`

flagが `True` ならthen、`False` ならelseを一回呼び出します。
キャプチャのbranch引数型と戻り値型は、既知branchの型と前述の正規化規則で決まります。

```surtr
if(True, 42, 0) # 42
choose = &if(&1, &2, 0)
# (Boolean, (-> Int) -> Int)
choose(True, {|| 42}) # 42
```

これは `Int` を返す例です。既知branchの型に0引数関数が残る場合は、生成される型も変わります。
両branchがプレースホルダなら、型注釈または期待される関数型を与えてください。

### `if_then`

flagが `True` の場合だけbranchを実行し、`False` なら何もせず `Unit` を返します。
branchのプレースホルダには `(-> Unit)` が必要です。

```surtr
if_then(True, print("selected")) # selected
when_true = &if_then(&1, &2)
# (Boolean, (-> Unit) -> Unit)
when_true(True, {|| print("selected")}) # selected
```

### `if_let`

Patternの照合成功時にthen、失敗時にelseを実行します。Patternを直接書いた完全な呼び出しをキャプチャできます。
bindingを作らないPatternでは、branchのプレースホルダは通常のLazy規則に従います。

```surtr
if_let(Ok(1), Ok(_), 42, 0) # 42
on_ok = &if_let(Ok(1), Ok(_), &1, 0)
# ((-> Int) -> Int)
on_ok({|| 42}) # 42
```

bindingを作るPatternの成功branchは、その成功scopeに直接書く式です。式内のプレースホルダは通常データを受け取ります。
外部の0引数関数で成功branchを置き換えることはできません。

```surtr
add_on_ok = &if_let(&1, Ok(x), x + &2, 0)
# (Result<Int>, Int -> Int)
add_on_ok(Ok(1), 2) # 3
```

### `if_let_then`

Patternの照合成功時だけbranchを実行し、失敗時は `Unit` を返します。
bindingを作らないPatternのbranchプレースホルダには `(-> Unit)` が必要です。

```surtr
if_let_then(Ok(1), Ok(_), print("matched")) # matched
on_match = &if_let_then(Ok(1), Ok(_), &1)
# ((-> Unit) -> Unit)
on_match({|| print("matched")}) # matched
```

bindingを作るPatternの成功branchは、`if_let`と同じく成功scopeに直接書く式です。

### `assert`

flagが `False` の場合だけerrorを実行して `Err` を返します。`True` なら `Ok(())` を返します。
errorのプレースホルダの正規化型は `(-> Error)` です。
ただし、`&assert(&1, &2)` が要求する `(Boolean, (-> Error) -> Result<Unit>)` は、Errorを通常の関数引数へ公開します。
既存のError制約により、この生成関数を通常の関数値として呼び出したり受け渡したりすることはできません。
キャプチャではerror式を固定してください。

```surtr
assert(True, NoneError) # Ok(())
check = &assert(&1, NoneError())
# (Boolean -> Result<Unit>)
check(True) # Ok(())
```

### `ensure`

valueを一回評価してpredicateへ渡し、成功なら `Ok(value)` を返します。失敗時だけerrorを実行して `Err` を返します。
predicateは通常の1引数関数です。errorのプレースホルダの正規化型は `(-> Error)` ですが、Errorを公開する通常の関数型としては使えません。
error式を固定すると、入力データだけを受け取る関数を作れます。

```surtr
ensure(3, {|n| n > 0}, NoneError) # Ok(3)
positive: (Int -> Result<Int>) = &ensure(&1, {|n| n > 0}, NoneError())
positive(3) # Ok(3)
```

### `Result::map_err`

`Err`の場合だけerrorを実行して新しいerrorへ置き換え、以前のcauseを保持しません。`Ok`はそのまま返します。
errorのプレースホルダの正規化型は `(-> Error)` ですが、Errorを通常の関数引数へ公開する制約は解除されません。
error式を固定してキャプチャします。

```surtr
Result::map_err(Ok(1), EmptyList) # Ok(1)
replace_error: (Result<Int> -> Result<Int>) = &Result::map_err(&1, EmptyList())
replace_error(Ok(1)) # Ok(1)
```

### `Result::cause`

`Err`の場合だけerrorを実行し、新しいerrorのcauseに以前のerrorを追加します。`Ok`はそのまま返します。
errorのプレースホルダの正規化型は `(-> Error)` です。`map_err`と同じく、通常の関数値として呼び出すキャプチャではerror式を固定します。

```surtr
Result::cause(Ok(1), EmptyList) # Ok(1)
add_cause: (Result<Int> -> Result<Int>) = &Result::cause(&1, EmptyList())
add_cause(Ok(1)) # Ok(1)
```

errorを受け取る各関数でも、通常の呼び出しでerror式を括弧で囲むと、判定前に一度評価します。
Errorの保持・受け渡しに関する既存の制約は、Lazyの正規化後も適用されます。

## `recover_kind`のErrorKind

`Result::recover_kind` の marker は `ErrorKind` です。Lazy 入力ではありません。
具体的な `deferror` 型名を直接書きます。修飾名も使えます。
constructor の payload 数にかかわらず、Error の生成や constructor の実行は行いません。

```surtr
Result::recover_kind(result, NetworkError, {|err| recover(err)})
&Result::recover_kind(&1, NetworkError, &2)
```

runtime Error 値、constructor call、文字列、抽象 `Error`、存在しない型名、非エラー型は拒否します。
`ErrorKind` は標準引数専用で、利用者の引数・戻り値・型注釈や変数には使えません。
キャプチャでも marker を直接プレースホルダへ置き換えることはできません。

## pipe RHS の括弧は別の規則

pipe の右辺では、括弧は「callable を返す式をまず評価する」ために使えます。
これは `Lazy<T>` eager boundary ではありません。

```surtr
def make_closure() -> (Int -> Int) {
  {|value| value + 1}
}

print(to_string(41 |> (make_closure())))
# 42
```

処理順は次のとおりです。

1. `make_closure()` を評価する
2. 式全体の静的な結果型が `Int -> Int` として解決される
3. pipe が `41` をその callable に渡す

`(make_closure())` が返した closure は、pipe 自身が入力値を渡して一度呼び出します。
Lazy 引数位置では、取得した値を正規化したあと、選ばれた branch を一回呼び出します。
pipe は入力値を callable に渡すため、引数の契約が異なります。

## pipe による Lazy parameter への注入は禁止

`|>`、`|*>`、`|*|`、`|>=` は、Lazy parameter を注入先に選べません。次をすべて含みます。

- RHS call への implicit first-argument injection
- `_1` placeholder による明示的な注入
- `|*>` / `|>=` の context value 注入
- `|*|` の contextual mapper / value の適用
- trait helper や partial call を経由する注入

Lazy parameter は callee が評価時点を制御します。pipe が値を注入すると、その制御境界が曖昧になるためです。

値を先に作りたい場合は、binding または closure で評価順を明示してください。

```surtr
prepared = expensive()
special(flag, prepared)

# または、special form が必要な値を受け取る closure を書く
value |> {|item| ordinary_function(item)}
```

`_1` と capture placeholder は別物です。pipe 用は `_1`、capture 用は `&1`, `&2`, ... です。

## 関連ページ

- special form の一覧: `./kernel.md`
- pipe 構文と `_1`: `./pipe-operators.md`
- closure / callable 値: `./callables.md`
- `Result` の error handling: `./error-handling.md`
- 標準定義の一次情報: `../../lib/kernel.srt`
