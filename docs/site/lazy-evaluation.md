# Lazy evaluation と括弧

Surtr の `Lazy<T>` は、通常の関数引数を明示的な closure で包ませるための型ではありません。
これは compiler が評価順序を制御する special form の引数を表す契約です。

通常の `expr` は callee が必要になった時点で評価されます。一方、**`Lazy<T>` 引数位置の
`(expr)` は eager boundary** です。`expr` を一度だけ先に評価し、その値を special form
へ渡します。

```text
Lazy parameter + expr
    => callee が必要なときに expr を評価する

Lazy parameter + (expr)
    => expr を先に一度評価し、その値を callee が使う
```

このページでは、special form と pipe 演算子での評価順を説明します。

## まず覚える規則

- `Lazy<T>` は compiler 用の契約であり、通常は closure を書かない
- `Lazy<T>` 引数の `expr` は、選ばれた／必要になった場合だけ評価される
- `Lazy<T>` 引数の `(expr)` は、選択・短絡判定より前に一度だけ評価される
- eager boundary は closure 値を自動で `()` 呼び出ししない
- pipe RHS の `(make_closure())` は別の規則であり、式を評価して得た callable を pipe に使う
- `|>`, `|*>`, `|>=` は `Lazy<T>` parameter へ値を注入できない

## `if` と `if_then`

`if(flag, then_branch, else_branch)` の branch は `Lazy<T>` です。
括弧なしなら選ばれなかった branch は評価しません。

```surtr
def selected() -> Unit { print("selected") }
def skipped() -> Unit { print("skipped") }

if(True, selected(), skipped())
# selected
```

`skipped()` を括弧で囲むと、False branch でも条件判定より先に一度評価されます。

```surtr
def selected() -> Unit { print("selected") }
def eager() -> Unit { print("eager") }

if(True, selected(), (eager()))
# eager
# selected
```

`if_then(flag, branch)` でも同じです。通常は `flag` が `True` のときだけ branch を評価し、
`if_then(flag, (branch))` は branch を先に評価します。

`if_let` と `if_let_then` の branch にもこの規則が適用されます。pattern の match 成否だけで
branch を評価したいときは括弧を付けません。

## `and` と `or`

`and(left, right)` と `or(left, right)` の右辺は `Lazy<Boolean>` です。

```surtr
and(False, check_expensive()) # check_expensive は評価されない
or(True, check_expensive())   # check_expensive は評価されない
```

右辺を括弧で囲むと、短絡する場合でも先に一度評価されます。

```surtr
and(False, (check_expensive())) # check_expensive は評価される
or(True, (check_expensive()))   # check_expensive は評価される
```

短絡を利用して副作用やコストの高い処理を避けたい場合、右辺に不要な括弧を付けないでください。

## `assert` と `ensure`

`assert(flag, err)` と `ensure(value, predicate, err)` の `err` は `Lazy<Error>` です。
成功時には通常 error 値を構築しません。

```surtr
assert(user_is_valid, InvalidUser())
ensure(age, {|n| n >= 0}, InvalidAge())
```

diagnostic の構築やログ出力を、成功時にも必ず一度行う必要がある場合だけ括弧を使います。

```surtr
assert(user_is_valid, (InvalidUser()))
ensure(age, {|n| n >= 0}, (InvalidAge()))
```

この場合も error 値が最終結果に使われるのは失敗時ですが、括弧内の式そのものは先に評価されます。

## `Result` の error special form

次の `Result` API も Lazy 引数を持つ special form です。

| Form | Lazy 引数 | 括弧なしの評価 |
| --- | --- | --- |
| `Result::map_err(result, err)` | `err` | `result` が `Err` のときだけ評価 |
| `Result::cause(result, err)` | `err` | `result` が `Err` のときだけ評価 |
| `Result::recover_kind(result, marker, handler)` | `marker` | kind marker として扱い、通常は runtime 評価しない |

`map_err` / `cause` の `(err)` は、result の tag を判定する前に error 式を一度評価します。

```surtr
Result::map_err(result, (NetworkError("retry later")))
Result::cause(result, (ContextError("loading profile")))
```

`recover_kind` の `(marker)` も eager boundary です。括弧内を先に一度評価しますが、
どの kind を回復するかは marker の concrete `deferror` constructor で決まります。

```surtr
Result::recover_kind(result, (NetworkError()), {|err| recover(err)})
```

## closure 値は呼び出さない

eager boundary は「式の評価結果を保持する」だけです。結果が closure なら、その closure 値を
保持し、暗黙にゼロ引数呼び出しはしません。

```surtr
special_form(make_handler())
# make_handler() の評価は Lazy 境界の内側

special_form((make_handler()))
# make_handler() を先に評価し、得られた closure 値を渡す
# 得られた closure に暗黙の () は付かない
```

呼び出したい closure 値なら、通常どおり明示的に `handler()` と書きます。

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
これは special form の Lazy 引数に括弧を付けたときの「closure 値を自動呼び出ししない」規則とは
異なる文脈です。

## pipe による Lazy parameter への注入は禁止

`|>`、`|*>`、`|>=` は、Lazy parameter を注入先に選べません。次をすべて含みます。

- RHS call への implicit first-argument injection
- `_1` placeholder による明示的な注入
- `|*>` / `|>=` の context value 注入
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
