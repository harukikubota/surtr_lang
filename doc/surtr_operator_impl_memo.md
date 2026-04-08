# Surtr 演算子仕様と実装メモ

2026-04-08 注記:

- 本メモは将来的に実装する演算子拡張のための資料とし、今回の改善作業では直接実装対象にしない
- 数値モデル変更 (`Int=BigInt`, `Float` 別紙化) に合わせて、将来着手時に前提を見直す

## 概要

本メモは、Surtr における関数関係の演算子について、

- 演算子分類
- 右辺の制約
- 基本的な意味
- lowering / 展開方針

をまとめた実装メモである。

特に `Apply` / `Compose` 系については、**単純なインライン展開ができるかどうか** を基準として実装する。

---

## OpKind

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Expr,
    Logical,
    Bind,    // =, =?
    Apply,   // |>, *>, >>=
    Compose, // >>, >=>
}
```

---

## 演算子表

| 演算子 | OpKind | 概要 | 型のイメージ | 基本 lowering |
|---|---|---|---|---|
| `=` | `Bind` | 通常束縛 | `pattern = expr` | 束縛専用ノードへ lower |
| `=?` | `Bind` | SafeBind / 失敗時早期リターン | `pattern =? Result<A>` | `match` 展開 |
| `|>` | `Apply` | 値を 1 引数 Callable に適用 | `A |> (A -> B) => B` | 直接 call へ展開 |
| `*>` | `Apply` | 文脈値に map 的適用 | `Result<A> *> (A -> B) => Result<B>` | `match Ok/Err` へ展開 |
| `>>=` | `Apply` | 文脈値に bind 的適用 | `Result<A> >>= (A -> Result<B>) => Result<B>` | `match Ok/Err` へ展開 |
| `>>` | `Compose` | 通常関数合成 | `(A -> B) >> (B -> C) => A -> C` | 単純ならインライン展開 |
| `>=>` | `Compose` | Kleisli 合成 | `(A -> Result<B>) >=> (B -> Result<C>) => A -> Result<C>` | 単純なら `match` 連結へ展開 |

---

## Apply 右辺の制約

Surtr の現行文法では、裸の関数参照は許可しない。  
関数参照は **関数コール** または **キャプチャ** で明示する。

### 許可する例

```surtr
value |> &parse_int
value |> parse_int()
value |> {|x| parse_int(x)}

result *> &normalize
result *> normalize()
result *> {|x| x + 1}

result >>= &validate
result >>= validate()
result >>= {|x| check(x)}
```

### 許可しない例

```surtr
value |> parse_int
result *> normalize
result >>= validate
```

---

## Apply 系の意味

### `|>`

`|>` は左辺の値を、右辺の **1 引数 Callable に解決可能な式** へ適用する。

```text
lhs : A
rhs : UnaryCallable(A -> B)
---------------------------
lhs |> rhs : B
```

### `*>`

`*>` は `Result<A>` に対して、右辺の 1 引数 Callable を map 的に適用する。

```text
lhs : Result<A>
rhs : UnaryCallable(A -> B)
---------------------------
lhs *> rhs : Result<B>
```

### `>>=`

`>>=` は `Result<A>` に対して、右辺の 1 引数 Callable を bind 的に適用する。

```text
lhs : Result<A>
rhs : UnaryCallable(A -> Result<B>)
-----------------------------------
lhs >>= rhs : Result<B>
```

---

## Compose 系の意味

### `>>`

```text
lhs : A -> B
rhs : B -> C
----------------
lhs >> rhs : A -> C
```

### `>=>`

```text
lhs : A -> Result<B>
rhs : B -> Result<C>
-------------------------
lhs >=> rhs : A -> Result<C>
```

---

## 実装メモ: lowering 方針

### 基本方針

`Apply` / `Compose` 系演算子は、**単純なインライン展開ができるかどうか** を基準に lowering する。

優先順位は次の通り。

1. **単純なインライン展開ができるなら展開する**
2. 展開できないが capture 不要なら、synthetic function 相当へ落とす
3. capture が必要な場合のみ closure を生成する

つまり、**クロージャ生成は最後の手段** とする。

---

## 単純なインライン展開

ここでいう「単純なインライン展開」とは、演算子式をその場で

- 通常 call
- `match`
- `match` の連結
- それに相当する branch chain

へ落とせることを指す。

### 例: `|>`

```surtr
value |> parse_int()
```

は単純に

```surtr
parse_int(value)
```

へ展開できる。

### 例: `*>`

```surtr
result *> normalize()
```

は

```surtr
match result {
  Ok(v) => Ok(normalize(v))
  Err(e) => Err(e)
}
```

へ展開できる。

### 例: `>>=`

```surtr
result >>= validate()
```

は

```surtr
match result {
  Ok(v) => validate(v)
  Err(e) => Err(e)
}
```

へ展開できる。

### 例: `>=>`

```surtr
&parse >=> &validate
```

が即時適用される場合、

```surtr
input |> (&parse >=> &validate)
```

は直接

```surtr
match parse(input) {
  Ok(v) => validate(v)
  Err(e) => Err(e)
}
```

へ展開してよい。

---

## 単純展開できる場合の基準

以下のようなケースは、原則として単純展開対象とする。

- 演算子結果がその場で消費される
- 右辺が `&fun`
- 右辺が `fun()`
- 右辺が単純なクロージャ
- 合成結果が一時値として即時適用される

### 例

```surtr
input |> parse()
parse(str) >>= validate()
parse(str) *> normalize()
input |> (&parse >=> &validate)
```

これらは基本的に dedicated closure object を作らず、その場で lower する。

---

## 単純展開できない場合

以下のようなケースは、即時展開せず callable として残す余地がある。

- 合成結果を変数へ束縛する
- 関数値として他関数へ渡す
- データ構造へ格納する
- 何度も再利用する
- capture が必要で、その場展開では表現しづらい

### 例

```surtr
pipeline = &parse >=> &validate
run_with(input, pipeline)
```

この場合は、まず callable として保持する設計を優先する。

---

## callable 化の優先順位

単純展開できない場合の優先順位は次の通り。

1. **capture なし synthetic function**
2. **軽量な composed callable 表現**
3. **closure**

### 理由

- capture なしなら、関数 ID や合成 callable として保持できる
- closure は allocation や実行コストが増えやすい
- `Result` 合成は多用されるため、closure 前提にすると負担が大きい

---

## `*>` の基準形

```surtr
lhs *> rhs
```

基準形:

```surtr
match lhs {
  Ok(v) => Ok(rhs(v))
  Err(e) => Err(e)
}
```

実装上は `rhs` が

- `&fun`
- `fun()`
- 単純な unary closure

のどれであるかを見て、call へ下ろす。

---

## `>>=` の基準形

```surtr
lhs >>= rhs
```

基準形:

```surtr
match lhs {
  Ok(v) => rhs(v)
  Err(e) => Err(e)
}
```

`rhs` は `A -> Result<B>` の unary callable である必要がある。

---

## `>=>` の基準形

```surtr
lhs >=> rhs
```

即時適用時の基準形:

```surtr
match lhs(input) {
  Ok(v) => rhs(v)
  Err(e) => Err(e)
}
```

複数連結時は `match` のネスト、またはそれに相当する branch chain に lower する。

### 例

```surtr
&parse >=> &validate >=> &normalize
```

は概念的には

```surtr
match parse(input) {
  Ok(v1) =>
    match validate(v1) {
      Ok(v2) => normalize(v2)
      Err(e) => Err(e)
    }
  Err(e) => Err(e)
}
```

に相当する。

---

## branch chain への平坦化

最終 IR / VM では、`match` の入れ子をそのまま保持する必要はない。  
必要に応じて branch chain に平坦化してよい。

概念例:

```text
t1 = parse(input)
if is_err(t1) goto err
v1 = unwrap_ok(t1)

t2 = validate(v1)
if is_err(t2) goto err
v2 = unwrap_ok(t2)

t3 = normalize(v2)
return t3

err:
return current_error
```

つまり、

- **ソース上の意味** は `match`
- **実装上の表現** は jump / branch chain

でよい。

---

## SafeBind との整合

`=?` は `Bind` に属する。  
ただし lowering の感覚としては `Result` の `match` 展開と同系統である。

```surtr
value =? parse(str)
```

は概念的には

```surtr
match parse(str) {
  Ok(v) => bind value = v
  Err(e) => return Err(e)
}
```

に相当する。

このため、`SafeBind` と `>>=` は意味的に近いが、

- `=?` は **束縛構文**
- `>>=` は **Apply 演算子**

として分離して扱う。

---

## 実装判断ルール

実装時の判断ルールを簡潔に書くと次の通り。

- **その場で消費されるなら、まず展開を試みる**
- **展開できるなら closure を作らない**
- **値として保持する必要がある場合のみ callable 化を考える**
- **capture がなければ synthetic function / function id を優先する**
- **capture がある場合のみ closure を生成する**

---

## メモ

- `Result` 文脈の合成は、ソース上は composable に見せつつ、内部では制御フローへ落とす
- `>=>` を毎回素朴な closure にする必要はない
- `|>` / `*>` / `>>=` / `>=>` は、可能な限り専用 lowering で吸収する
- 最適化というより、まずは **不要な callable object を作らない** ことを優先する

---

## 今後の拡張余地

将来 `FuncSection` が入る場合でも、Apply 右辺の規則は維持できる。

追加後の想定:

```surtr
value |> (`eq` 10 _)
result *> (_ + 1)
result >>= check(_)
```

このときも基本方針は同じで、**1 引数 Callable に解決できる式** として扱う。
