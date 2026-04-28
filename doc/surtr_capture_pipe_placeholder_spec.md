# Surtr: キャプチャプレースホルダ / パイププレースホルダ仕様メモ

Status: draft
Target: Phase 3 以降の `capture placeholder` / `pipe placeholder` 仕様化

Implementation note (2026-04-28):

- `_1` は lexer 専用 token を追加せず、引き続き `Ident("_1")` のまま tokenize する
- `pair._1` は `.` に続く field access / tuple path として従来どおり解釈する
- pipe RHS の最外 call の direct argument に現れた bare `_1` だけを Resolver で slot として lower する
- capture placeholder `&1`, `&2`, ... は outermost capture scope にのみ属する
- outer capture の内部では plain named capture `&pred` は許可するが、`&inner(...)` のような nested capture argument block は compile error とする

---

## 1. 目的

本メモは、以下を明確に分離するための仕様案である。

- キャプチャ演算子 `&` におけるプレースホルダ `&1`, `&2`, ...
- パイプ RHS における注入位置マーカー `_1`

設計方針は次の通り。

- コンパイラによる暗黙的な arity 補正をしない
- prefix partial application を導入しない
- Surtr / Elixir 系の「第一引数が self」という規約を維持する
- pipe chain によって変換段階を表面化する
- 誤用時に具体的な修正案を出せるようにする

---

## 2. 既存仕様との関係

V9 時点では、関数値として保持できるのは capture または closure のみであり、裸の関数参照は許可しない。
また、`|>` / `|*>` / `|>=` は apply 系であり、RHS が call 式なら LHS を第一引数へ注入する。

本メモでは、その前提に次を追加する。

- capture placeholder `&1`, `&2`, ... を導入する
- pipe placeholder `_1` を導入する
- 両者は構文上も意味上も別物として扱う

---

## 3. 用語

| 用語                  | 例              | 意味                                          |                         |     |       |                                    |
| ------------------- | -------------- | ------------------------------------------- | ----------------------- | --- | ----- | ---------------------------------- |
| named capture       | `&add`         | 名前付き関数を関数値として取り出す                           |                         |     |       |                                    |
| placeholder capture | `&add(&1, 10)` | placeholder を仮引数として関数値を作る                   |                         |     |       |                                    |
| anonymous capture   | `&(&1 + &2)`   | 関数名なしの capture shorthand。Surtr では禁止         |                         |     |       |                                    |
| pipe placeholder    | `_1`           | pipe RHS call の direct argument に置く注入位置マーカー |                         |     |       |                                    |
| call RHS            | `x             | > add(10)`                                  | pipe RHS が関数 call である形式 |     |       |                                    |
| callable RHS        | `x             | > &add` / `x                                | > {                     | v   | ...}` | pipe RHS が capture / closure である形式 |

---

## 4. キャプチャ演算子 `&` の基本ルール

### 4.1 `&` は名前付き関数 capture のみ

許可する基本形は次のみ。

```surtr
&func
&Module::func
&Type::method
&func(args...)
&Module::func(args...)
&Type::method(args...)
```

ただし、`&func(args...)` 形式には後述の placeholder 条件がある。

### 4.2 anonymous capture は禁止

Elixir 風の関数名なし capture は導入しない。

```surtr
&(&1)          # NG
&(&1 + &2)    # NG
&(add(&1, 1)) # NG
```

理由:

- `&` が「任意式を関数化する演算子」になってしまう
- Surtr には明示 closure `{|x| expr}` がある
- identity は `&id` で代替できる
- 変換処理は pipe chain か明示 closure で表面化する

代替:

```surtr
&id
{|x, y| x + y}
{|x| add(x, 1)}
```

---

## 5. capture placeholder `&1`, `&2`, ...

### 5.1 意味

`&1`, `&2`, ... は placeholder capture 内の仮引数変数として扱う。

```surtr
&add(&1, 10)
```

概念的には次へ lower する。

```surtr
{|__cap1| add(__cap1, 10)}
```

```surtr
&add(10, &1)
```

概念的には次へ lower する。

```surtr
{|__cap1| add(10, __cap1)}
```

```surtr
&add(&1 + 10, &2 * &3)
```

概念的には次へ lower する。

```surtr
{|__cap1, __cap2, __cap3|
  add(__cap1 + 10, __cap2 * __cap3)
}
```

### 5.2 placeholder capture は「単一で評価可能な関数 call 形」のみ

`&func(args...)` は、placeholder を仮引数変数として置いた後、`func(args...)` が通常の関数 call として成立する必要がある。

コンパイラは不足引数を補わない。

```surtr
&add(&1, 10)   # OK: add(_, 10) は arity 2 の call として成立
&add(10, &1)   # OK: add(10, _) は arity 2 の call として成立
&add(&1)       # NG: add の arity が 2 なら引数不足
&add(10)       # NG: prefix partial ではない
```

### 5.3 prefix partial application は導入しない

次は無効。

```surtr
inc: Int -> Int = &add(10) # NG
```

Surtr は第一引数を self / subject として扱うため、`&add(10)` を `x -> add(10, x)` と暗黙解釈しない。

引数位置を明示したい場合は placeholder を使う。

```surtr
inc: Int -> Int = &add(&1, 10)
add_to_10: Int -> Int = &add(10, &1)
```

### 5.4 placeholder index 規則

`&1..&N` は連続して出現しなければならない。

```surtr
&add(&1, &2)   # OK
&sub(&2, &1)   # OK: out-of-order は許可
&eq(&1, &1)    # OK: repeated use は許可

&add(&2, 10)   # NG: &1 がない
&add(&1, &3)   # NG: &2 がない
```

生成される関数の arity は、使用された最大 placeholder index `N` で決まる。

### 5.5 placeholder は変数扱い

capture placeholder は、placeholder capture の式内では通常の変数のように参照できる。

```surtr
&add(&1 + 10, &2 * &3)       # OK
&String::surround("[", &1)  # OK
&ensure(&1, &non_empty, err) # OK, ただし nested capture ルールに注意
```

ただし、`&1` 単体は関数値ではない。

```surtr
&1      # NG
x = &1  # NG
```

identity が必要な場合は `&id` を使う。

### 5.6 nested capture は禁止

placeholder capture の内側に、別の capture 式をネストしない。

```surtr
&List::map(&1, &add(&2, &1)) # NG: nested capture
```

必要なら明示 closure または named helper へ分離する。

```surtr
&List::map(&1, {|elm| add(&2, elm)}) # OK とする場合は closure lexical capture として扱う
```

実装をさらに単純化する段階では、placeholder capture 内の closure に `&N` が入るケースも禁止してよい。
その場合は次のように一度束縛するか、named helper に切り出す。

```surtr
# 方針次第で推奨される明示形
{|xs, base| List::map(xs, {|elm| add(base, elm)})}
```

---

## 6. pipe placeholder `_1`

### 6.1 `_1` は変数ではない

`_1` は pipe RHS call の direct argument にだけ置ける注入位置マーカーである。
通常の式ではない。
名前解決にも載せない。

```surtr
x = _1          # NG
add(10, _1)    # NG: pipe 外
{|x| add(x, _1)} # NG
```

### 6.2 `_1` は最外側 call の direct argument のみ

許可:

```surtr
A |> fn(_1, 2)   # fn(A, 2)
A |> fn(2, _1)   # fn(2, A)
A |> add(10, _1) # add(10, A)
```

禁止:

```surtr
A |> fn(add(10, _1), 2) # NG: nested expression 内
A |> fn(_1 + 10, 2)     # NG: _1 が式の一部
A |> fn(_1, _1)         # NG: 複数使用
```

`_1` は `PipeSlot` であり、`Expr` ではない。

### 6.3 `_1` がない call RHS は第一引数注入

```surtr
A |> fn(2)
```

は次と等価。

```surtr
fn(A, 2)
```

### 6.4 `_1` がある call RHS は slot 置換

```surtr
A |> fn(2, _1)
```

は次と等価。

```surtr
fn(2, A)
```

この場合、通常の第一引数注入は行わない。

```surtr
A |> fn(2, _1)
# fn(A, 2, A) ではない
```

### 6.5 nested `_1` は pipe chain へ展開する

禁止:

```surtr
A |> fn(add(10, _1), 2)
```

推奨される修正:

```surtr
A
|> add(10, _1)
|> fn(2)
```

この制約により、変換段階が pipe chain に表面化する。

---

## 7. pipe RHS の分類

`|>` の RHS は次のいずれかを受ける。

| RHS 種別 | 例 | pipe の処理 |
|---|---|---|
| call RHS | `A |> f(2)` | call AST へ第一引数注入 |
| call RHS + `_1` | `A |> f(2, _1)` | `_1` 位置へ slot 置換 |
| capture RHS | `A |> &f` / `A |> &f(&1, 2)` | RHS を unary callable として apply |
| closure RHS | `A |> {|x| f(x, 2)}` | RHS を unary callable として apply |

重要な区別:

- call RHS のみ、pipe が AST 注入を行う
- capture / closure RHS は、単体で `A -> B` として型検査される
- pipe は capture / closure の内部構造を書き換えない
- `_1` は capture / closure RHS 内では使えない

---

## 8. `num |> &add(10)` は無効

```surtr
num |> &add(10) # NG
```

理由:

- `&add(10)` は単体で評価可能な capture ではない
- prefix partial application は導入しない
- pipe が capture 式内部へ第一引数注入すると、暗黙的な AST 操作になる

代替:

```surtr
num |> add(10)       # add(num, 10)
num |> &add(&1, 10)  # capture RHS callable apply
```

逆順にしたい場合:

```surtr
num |> add(10, _1)      # add(10, num)
num |> &add(10, &1)     # capture RHS callable apply
```

---

## 9. 判定表

### 9.1 capture

| 式 | 判定 | 意味 |
|---|---:|---|
| `&add` | OK | `add` の全 arity を持つ関数値 |
| `&add(10)` | NG | prefix partial はしない |
| `&add(&1, 10)` | OK | `x -> add(x, 10)` |
| `&add(10, &1)` | OK | `x -> add(10, x)` |
| `&add(&1)` | NG | `add` が arity 2 なら引数不足 |
| `&add(&2, 10)` | NG | `&1` 欠番 |
| `&add(&1, &3)` | NG | `&2` 欠番 |
| `&sub(&2, &1)` | OK | 引数入れ替え |
| `&eq(&1, &1)` | OK | repeated use |
| `&(&1 + &2)` | NG | anonymous capture 禁止 |
| `&1` | NG | standalone placeholder 禁止 |

### 9.2 pipe

| 式 | 判定 | 意味 |
|---|---:|---|
| `A |> fn(2)` | OK | `fn(A, 2)` |
| `A |> fn(_1, 2)` | OK | `fn(A, 2)` |
| `A |> fn(2, _1)` | OK | `fn(2, A)` |
| `A |> fn(add(10, _1), 2)` | NG | nested `_1` |
| `A |> fn(_1 + 10, 2)` | NG | `_1` は Expr ではない |
| `A |> fn(_1, _1)` | NG | `_1` は一度だけ |
| `A |> &add` | 型次第 | `&add` が unary callable なら OK |
| `A |> &add(10)` | NG | capture として単体評価不能 |
| `A |> &add(&1, 10)` | OK | `add(A, 10)` |
| `A |> &add(10, &1)` | OK | `add(10, A)` |
| `A |> {|x| add(10, x)}` | OK | closure apply |
| `A |> {|x| add(10, _1)}` | NG | `_1` は closure 内で無効 |

---

## 10. 診断方針

### 10.1 pipe placeholder を式として使った場合

入力:

```surtr
A |> fn(add(10, _1), 2)
```

診断例:

```text
error: pipe placeholder `_1` cannot be used as an expression

  A |> fn(add(10, _1), 2)
                   ^^

`_1` is only allowed as a direct argument of the outermost call on the RHS of a pipe operator.

help: move the transformation to the previous pipe step

  A
  |> add(10, _1)
  |> fn(2)
```

### 10.2 `&add(10)` を関数値として使った場合

入力:

```surtr
inc: Int -> Int = &add(10)
```

診断例:

```text
error: capture call is missing placeholder arguments

  inc: Int -> Int = &add(10)
                    ^^^^^^^^

`&add(10)` is not a prefix partial application.
Use capture placeholders to specify the argument position explicitly.

help: pass the captured value as the first argument

  inc: Int -> Int = &add(&1, 10)

help: pass the captured value as the second argument

  add_to_10: Int -> Int = &add(10, &1)
```

### 10.3 anonymous capture を使った場合

入力:

```surtr
f = &(&1 + &2)
```

診断例:

```text
error: anonymous capture is not supported

  f = &(&1 + &2)
      ^^^^^^^^^^

`&` captures named functions only. Use an explicit closure or a named helper.

help:

  f = {|x, y| x + y}
```

---

## 11. lowering 方針

### 11.1 capture placeholder lowering

```surtr
&add(10, &1)
```

概念的には次へ lower する。

```surtr
{|__cap1| add(10, __cap1)}
```

ただし surface syntax として anonymous closure shorthand を許可するわけではない。
これは compiler internal lowering である。

### 11.2 pipe call RHS lowering

```surtr
A |> f(2)
```

```surtr
f(A, 2)
```

```surtr
A |> f(2, _1)
```

```surtr
f(2, A)
```

### 11.3 pipe capture / closure RHS lowering

```surtr
A |> &f(&1, 2)
```

概念的には次と同じ。

```surtr
(&f(&1, 2))(A)
```

型検査上は、RHS が `A -> B` の callable value であることを要求する。

---

## 12. AST / 実装メモ

### 12.1 AST ノード案

```rust
enum Expr {
    Call(CallExpr),
    Capture(CaptureExpr),
    Closure(ClosureExpr),
    Pipe(PipeExpr),
    CapturePlaceholder { index: usize, span: Span },
    PipeSlot { span: Span },
    // ...
}
```

ただし、`PipeSlot` は通常の式として型検査へ流さない。
`PipeSlot` は pipe lowering 後に AST へ残っていてはいけない。

### 12.2 capture validation

- `&func` は named capture として許可
- `&func(args...)` は args 内に `CapturePlaceholder` が必要
- `&func(args...)` は placeholder を仮引数変数に置き換えた後、通常 call として arity が一致する必要がある
- `&func(args...)` without placeholder は invalid
- `&(...)` は invalid
- nested `CaptureExpr` は invalid
- placeholder index は `1..N` 連続必須

### 12.3 pipe validation

pipe RHS が call の場合:

- direct positional argument に `_1` が 0 個なら第一引数注入
- direct positional argument に `_1` が 1 個なら slot 置換
- direct positional argument に `_1` が 2 個以上なら error
- nested expression に `_1` があれば error
- closure / capture / block の内側に `_1` があれば error

pipe RHS が capture / closure の場合:

- RHS を単体で型検査する
- RHS は unary callable `A -> B` でなければならない
- RHS 内に `_1` があれば error

---

## 13. テスト観点

### 13.1 成功ケース

```surtr
x |> add(10)
x |> add(10, _1)
x |> &add(&1, 10)
x |> &add(10, &1)
x |> {|v| add(10, v)}

f: (Int, Int) -> Int = &add
inc: Int -> Int = &add(&1, 1)
add_to_10: Int -> Int = &add(10, &1)
swap_sub: (Int, Int) -> Int = &sub(&2, &1)
```

### 13.2 失敗ケース

```surtr
&add(10)
&add(&1)
&add(&2, 10)
&add(&1, &3)
&(&1 + &2)
&1

x |> fn(add(10, _1), 2)
x |> fn(_1 + 10, 2)
x |> fn(_1, _1)
x |> &add(10)
x |> {|v| add(v, _1)}
```

### 13.3 診断テスト

- nested `_1` では「前の pipe に処理を移す」help を出す
- `&add(10)` では `&add(&1, 10)` / `&add(10, &1)` を help として出す
- anonymous capture では explicit closure を help として出す
- placeholder 欠番では missing index を明示する

---

## 14. 仕様要約

```text
Capture placeholder:
  - &1, &2, ... は placeholder capture 内の仮引数変数
  - & は named function capture のみ
  - &func(args...) は placeholder を含む場合だけ有効
  - &func(args...) は対象関数の full call shape でなければならない
  - prefix partial application はしない
  - anonymous capture &(...) は禁止
  - nested capture は禁止
  - placeholder index は 1..N 連続必須
  - repeated use / out-of-order use は許可

Pipe placeholder:
  - _1 は pipe RHS call の direct argument 専用 slot marker
  - _1 は変数ではない
  - _1 は Expr ではない
  - _1 は 1 回だけ使用可能
  - _1 がある場合、通常の第一引数注入は行わない
  - nested expression 内の _1 は禁止
  - capture / closure 内の _1 は禁止
  - pipe RHS が capture / closure の場合は unary callable apply のみ行う
```
