# Surtr: Result / List 向け Bind・Pipeline 演算子整理

## 概要

Surtr ではモナド的な文脈を一般化せず、**`Result` と `List` の 2 種類に限定**して扱う。

対象演算子は以下の 5 つとする。

- `|>` : 値パイプ
- `|*>` : 文脈 map
- `|>=` : 文脈 bind
- `|=>` : 文脈付き関数合成
- `=?` : SafeBind

この設計の目的は次の通り。

- 式を左から右へ読めるようにする
- 構文から期待される型を読み取りやすくする
- 型検査フローを単純に保つ
- 暗黙 `pure` / `wrap` や一般化されたモナド推論を導入しない
- `Result` と `List` の意味差を型規則に明示的に反映する
- lowering 時に不要な callable object / closure をなるべく作らない

---

## この文書の位置づけ

この文書は、既存の Result / List 演算子整理メモに、前身メモである「演算子仕様と実装メモ」の内容を統合し、**仕様 + 実装判断ルール** として補強した版である。

特に次の点を追記している。

- `OpKind` による演算子分類
- Apply / Compose 右辺の制約
- parser / type checker / lowering の責務分離
- 単純なインライン展開と callable 化の優先順位
- branch chain まで見据えた lowering 基準

---

## 実装タスク一覧

この節は、**この文書を元にした実装進捗の確認用チェックリスト**である。
2026-04-10 時点の状態を反映する。

記号の意味:

- `[x]` 実装済み
- `[-]` 部分実装
- `[ ]` 未実装

### 構文 / Parser

- `[x]` `|>`, `|*>`, `|>=`, `|=>`, `>>` を最長一致でトークナイズ
- `[x]` 上記演算子を左結合で parse
- `[x]` `Pipe` / `ContextMap` / `ContextBind` / `Compose` / `KleisliCompose` を AST に保持
- `[x]` `&qualified::path` を capture として parse
- `[x]` `&qualified::path(args...)` を partial capture として parse
- `[x]` apply 位置の `foo(...)` / `Type::method(...)` を第一引数注入 call として受理
- `[x]` `Result<List<Int>>` のようなネスト型で `>>` を `>` `>` と誤解しないよう補正

### 名前解決 / 型検査

- `[x]` 新演算子ノードを `sigil` / `scar` に伝播
- `[x]` `|>` の単項 callable 型検査
- `[x]` `|*>` の `Result` / `List` map 型検査
- `[x]` `|>=` の `Result` / `List` bind 型検査
- `[x]` `|=>` の `Result` / `List` Kleisli 合成型検査
- `[x]` `>>` の通常関数合成型検査
- `[x]` `Result` と `List` の混在チェインを拒否
- `[x]` `|*>` の右辺が文脈付き関数の場合を拒否
- `[x]` 裸の関数参照を apply / compose 右辺で拒否
- `[x]` 変数代入で裸の関数参照を禁止する既存ルールを維持
- `[x]` `SafeBind` を `Result` 専用ではなく `List` / `Result` 対象に整理

### Lowering / Runtime

- `[x]` `|>` を直接 apply に lower
- `[x]` `Result |*>` を `Ok/Err` 分岐に lower
- `[x]` `Result |>=` を `Ok/Err` 分岐に lower
- `[x]` `List |*>` を `List::map` 相当へ lower
- `[x]` `List |>=` を `List::flat_map` 相当へ lower
- `[x]` `|=>` / `>>` を synthetic callable で表現
- `[x]` 新 opcode を増やさず既存 `CaptureClosure` / `CallClosure` 系で実装
- `[-]` 即時適用される compose 式の全面インライン最適化

### 標準モジュール / Builtin

- `[x]` `List::wrap(T) -> List<T>` を追加
- `[x]` `List::map` を追加
- `[x]` `List::flat_map` を追加
- `[x]` `[]` を Nil とする前提で docs / semantics を整合
- `[x]` `pure` は追加しない
- `[x]` `List` helper の runtime 実装を builtin として追加
- `[x]` compose / map / bind lowering が `List` helper semantics と一致

### SafeBind

- `[x]` `ret: Int =? parse_int("1")`
- `[x]` `[head, ..tail] =? [1, 2, 3]`
- `[x]` `[head, ..tail] =? Ok([1, 2, 3])`
- `[x]` nested constructor を含む list pattern safebind
- `[x]` 既存 REPL の SafeBind 振る舞い維持

### テスト

- `[x]` parser unit test: qualified capture
- `[x]` parser unit test: flow 左結合
- `[x]` parser unit test: nested generic + `>>`
- `[x]` integration test: `|>` with capture / 第一引数注入 call
- `[x]` integration test: `|*>` / `|>=` / `|=>` for `Result`
- `[x]` integration test: `|*>` / `|>=` / `|=>` for `List`
- `[x]` integration test: naked function ref rejection
- `[x]` integration test: Result/List 混在 rejection
- `[x]` `tests/spec` / `tests/compile_errors` fixture への演算子ケース追加

### 文書反映

- `[x]` `=?` が Result 専用ではない点を反映
- `[x]` `&User::get_name` を許可する点を反映
- `[x]` apply 位置の `foo(...)` / `Type::method(...)` を第一引数注入として反映
- `[x]` `List::wrap` と `[]` ベースの説明へ更新

---

## 採用方針

### 1. モナド的文脈は `Result` と `List` のみ

Surtr では以下のみを map / bind / compose の対象とする。

- `Result<A, E>`
- `List<A>`

`Option` や `Task` など、その他のモナドインスタンスは導入しない。

### 2. `Result` と `List` は混合しない

`|>=` および `|=>` は、**同じ文脈型同士のみ**接続できる。  
`|*>` も同様に、対象文脈のまま値だけを変換する。

許可:

```surtr
parse |>= validate |>= normalize
# Result 系
```

```surtr
expand |>= dedup |>= sort
# List 系
```

不許可:

```surtr
parse |>= expand_many
# Result<A, E> |>= (A -> List<B>)
```

```surtr
values |>= validate
# List<A> |>= (A -> Result<B, E>)
```

`Result` と `List` の相互変換は、必要なら明示 API または別構文で扱う。`|>=` や `|=>` では吸収しない。

### 3. 暗黙 `pure` / `wrap` は行わない

以下のような接続を `|>=` では許可しない。

```surtr
result_fn |>= plain_fn
# (A -> Result<B, E>) |>= (B -> C)
```

```surtr
list_fn |>= plain_fn
# (A -> List<B>) |>= (B -> C)
```

この形を許可すると、右辺の戻り値をどの文脈へ持ち上げるかを決める必要がある。Surtr ではその判断を型推論や暗黙規則に委ねない。

そのため、**bind 後に値だけを持ち上げる専用演算子として `|*>` を導入する**。

許可:

```surtr
result_fn |*> plain_fn
# (A -> Result<B, E>) |*> (B -> C) -> (A -> Result<C, E>)
```

```surtr
list_fn |*> plain_fn
# (A -> List<B>) |*> (B -> C) -> (A -> List<C>)
```

---

## 演算子分類

前身メモの `OpKind` を、最新の演算子体系に合わせて更新する。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Expr,
    Logical,
    Bind,    // =, =?
    Apply,   // |>, |*>, |>=
    Compose, // >>, |=>
}
```

### 分類意図

- `Bind` は束縛と制御を含む構文
- `Apply` は左辺の値または文脈値に右辺 callable を適用する構文
- `Compose` は関数同士を連結し、新たな callable を得る構文

`=?` は見た目は演算子だが、意味上は `Bind` に属する専用制御構文とみなす。

---

## 演算子表

| 演算子 | OpKind | 概要 | 型のイメージ | 基本 lowering |
|---|---|---|---|---|
| `=` | `Bind` | 通常束縛 | `pattern = expr` | 束縛専用ノードへ lower |
| `=?` | `Bind` | SafeBind / 失敗時早期リターン | `pattern =? Expr` / `pattern =? Result<A, E>` | `match` 展開 |
| `|>` | `Apply` | 値を 1 引数 callable に適用 | `A |> (A -> B) => B` | 直接 call へ展開 |
| `|*>` | `Apply` | 文脈値に map 的適用 | `Result<A, E> |*> (A -> B) => Result<B, E>` / `List<A> |*> (A -> B) => List<B>` | `match Ok/Err` または `map` |
| `|>=` | `Apply` | 文脈値に bind 的適用 | `Result<A, E> |>= (A -> Result<B, E>) => Result<B, E>` / `List<A> |>= (A -> List<B>) => List<B>` | `match Ok/Err` または `flat_map` |
| `>>` | `Compose` | 通常関数合成 | `(A -> B) >> (B -> C) => A -> C` | 単純ならインライン展開 |
| `|=>` | `Compose` | Kleisli 合成 | `(A -> Result<B, E>) |=> (B -> Result<C, E>) => A -> Result<C, E>` / `(A -> List<B>) |=> (B -> List<C>) => A -> List<C>` | 単純なら `bind` 連結へ展開 |

---

## 演算子ごとの責務

## `|>` 値パイプ

値を関数またはクロージャへ左から右に流す。

```surtr
user |> &User::name
```

### 型規則

```text
A |> (A -> B) -> B
```

### 用途

- 通常値の変換
- モナド文脈に入る前の前処理
- 既存値を単項関数へ流す記法

### 備考

`|>` 自体は文脈を保存しない。右辺の戻り値が `Result` なら `Result` を返し、`List` なら `List` を返すが、それは単に右辺関数の戻り値による。

---

## `|*>` 文脈 map

同一文脈を維持したまま、中の値だけを通常関数で変換する。  
`bind + wrap` 相当を暗黙化せず、値変換専用として切り出した演算子とみなせる。

### Result の型規則

```text
Result<A, E> |*> (A -> B) -> Result<B, E>
(A -> Result<B, E>) |*> (B -> C) -> (A -> Result<C, E>)
```

### List の型規則

```text
List<A> |*> (A -> B) -> List<B>
(A -> List<B>) |*> (B -> C) -> (A -> List<C>)
```

### 意味

#### Result

- `Ok(v)` なら右辺を適用し `Ok(rhs(v))`
- `Err(e)` ならそのまま `Err(e)` を返す

#### List

- 各要素に右辺関数を適用し `List<B>` を返す
- flatten は行わない

### 用途

- `Result` 成功値の整形
- `List` 各要素の通常変換
- 文脈付き関数チェインの末尾を通常関数で閉じる場合

### 許可例

```surtr
parse_user(input) |*> User::name() |*> String::trim()
```

```surtr
expand_users |*> User::id()
```

---

## `|>=` 文脈 bind

同一文脈内で値を取り出し、次の文脈付き関数へ接続する。

`Result` と `List` で意味は異なるが、どちらも「文脈を保ったまま次の段階へ進める」演算子として扱う。

### Result の型規則

```text
Result<A, E> |>= (A -> Result<B, E>) -> Result<B, E>
(A -> Result<B, E>) |>= (B -> Result<C, E>) -> (A -> Result<C, E>)
```

### List の型規則

```text
List<A> |>= (A -> List<B>) -> List<B>
(A -> List<B>) |>= (B -> List<C>) -> (A -> List<C>)
```

### 意味

#### Result

- `Ok(v)` なら右辺へ進む
- `Err(e)` ならそのまま `Err(e)` を返す

#### List

- 各要素に右辺関数を適用する
- 得られた `List<List<B>>` を平坦化して `List<B>` にする

### 用途

- `Result` の段階的検証や変換
- `List` の flat_map 的処理
- 文脈付き関数チェイン

### 禁止事項

以下はすべてコンパイルエラーとする。

```surtr
Result<A, E> |>= (A -> List<B>)
List<A> |>= (A -> Result<B, E>)
(A -> Result<B, E>) |>= (B -> C)
(A -> List<B>) |>= (B -> C)
```

通常関数を右辺に取りたい場合は `|*>` を使う。

---

## `|=>` 文脈付き関数合成

文脈付き関数同士を合成し、新たな文脈付き関数を作る。

### Result の型規則

```text
(A -> Result<B, E>) |=> (B -> Result<C, E>) -> (A -> Result<C, E>)
```

### List の型規則

```text
(A -> List<B>) |=> (B -> List<C>) -> (A -> List<C>)
```

### 意味

#### Result

```surtr
f |=> g
```

は概念的に以下と等価。

```surtr
{|x| f(x) |>= g }
```

#### List

```surtr
f |=> g
```

は概念的に以下と等価。

```surtr
{|x| f(x) |>= g }
```

ただし `|>=` の意味が型で変わるため、Result と List で実際の lower 先は異なる。

### 構文制約

- 左右とも関数またはクロージャでなければならない
- 値は受け取らない

許可:

```surtr
&parse |=> &validate
```

不許可:

```surtr
value |=> &validate
```

---

## `=?` SafeBind

`=?` は `Result` 専用ではなく、**早期失敗伝播を伴う束縛構文** として扱う。

```surtr
ret: Int =? parse_int("1")
[head, ..tail] =? [1, 2, 3]
[head, ..tail] =? Ok([1, 2, 3])
```

### 右辺の 2 系統

1. `pattern =? Result<A, E>`

- 右辺が `Ok(v)` なら `pattern = v`
- 右辺が `Err(e)` なら現在の関数・ブロックから `Err(e)` を早期リターン

2. `pattern =? Expr`

- `Expr` が SafeBind 対象の失敗しうるパターン入力であれば、そのパターン失敗を既存失敗値として伝播する
- 現状の一般化対象は `List` と `Result` に限定する

### 左辺に許可するもの

- 変数
- 型注釈付き変数
- 分解パターン
- as-pattern

### 備考

`=?` は見た目は演算子だが、意味としては制御構文に近い。一般 `bind` と同列には扱わない。

---

## Result と List の意味差

同じ `|*>` / `|>=` / `|=>` を使っても、`Result` と `List` では役割が異なる。この違いは仕様上明示しておく必要がある。

## Result

`Result` は **失敗の短絡** を表す。

```surtr
parse |>= validate |>= build
```

どこか 1 段階で `Err` になれば、残りは評価されない。

## List

`List` は **複数値展開** を表す。

```surtr
expand |>= normalize |>= dedup
```

各段階で 0 個以上の値へ広がり得る。

## 設計上の原則

- `Result` は制御フロー寄り
- `List` はデータフロー寄り
- 同一記号を使っても、型規則と lowering で意味を分離する
- `=?` は SafeBind 対象にのみ属する構文とする

---

## Apply / Compose 右辺の制約

Surtr の現行文法では、裸の関数参照は許可しない。  
関数参照は **関数コール** または **キャプチャ** で明示する。

### 許可する例

```surtr
value |> &parse_int
value |> parse_int()
user |> &User::get_name
user |> User::get_name()
value |> {|x| parse_int(x)}

result |*> &normalize
result |*> normalize()
result |*> {|x| x + 1}

result |>= &validate
result |>= validate()
result |>= {|x| check(x)}

pipeline = &parse |=> &validate
```

### 許可しない例

```surtr
value |> parse_int
result |*> normalize
result |>= validate
```

### 共通ルール

`|>`, `|*>`, `|>=` は apply 系であり、右辺が関数コールなら
**左辺値を第一引数へ注入**する。

```surtr
x |> f(1, 2)          # => f(x, 1, 2)
user |> User::get_name() # => User::get_name(user)
```

一方で `>>` / `|=>` は compose 系であり、右辺・左辺ともに
**クロージャ値**である必要がある。
許可するのは capture と closure のみ。

```surtr
&parse |=> &validate
{|x| parse(x)} |=> {|y| validate(y)}
```

`parse() |=> validate()` のような関数コールは、callable ではなく
式の実行結果なので compose には使えない。

また、変数にそのまま保持できる関数値はクロージャかキャプチャに限る。
裸の関数参照は `|>`, `|*>`, `|>=`, `|=>`, `>>` の右辺でも代入でも許可しない。

```text
lhs : A
rhs : UnaryCallable(A -> B)
---------------------------
lhs |> rhs : B
```

```text
lhs : Result<A, E>
rhs : UnaryCallable(A -> B)
---------------------------------
lhs |*> rhs : Result<B, E>
```

```text
lhs : Result<A, E>
rhs : UnaryCallable(A -> Result<B, E>)
---------------------------------------------
lhs |>= rhs : Result<B, E>
```

List でも同様に、対象文脈に応じた unary callable 制約を課す。

---

## 優先順位と結合性

Surtr では以下の大分類で優先順位を管理する。

1. バインド演算子
2. フロー演算子
3. 論理演算子
4. 式演算子

ただし、`|>`, `|*>`, `|>=`, `|=>`, `=?` は同一レベルで扱う方針があるため、実装上は同じ binding power を持たせ、**左結合**で解決する。

```surtr
a |> f |> g
parse |>= validate |>= build
f |=> g |=> h
result |*> normalize() |*> trim()
```

### 注意

同一優先度でも、受け取る構文カテゴリは演算子ごとに固定する。

- `|>` : 左辺は値、右辺は関数/クロージャ
- `|*>` : 左辺は文脈値または文脈付き関数チェイン結果、右辺は通常関数
- `|>=` : 左辺は文脈値または文脈付き関数チェイン結果、右辺は同一文脈返却関数
- `|=>` : 左右とも関数/クロージャ
- `=?` : 左辺は束縛可能パターン、右辺は SafeBind 対象

この制約により、優先順位が同じでも不自然な式を構文または型検査で弾ける。

---

## AST / HIR / Lowering の責務分離

Surtr では、演算子を parser 段階で即座に desugar せず、段階的に扱う。

## 1. Parsed AST

構文をそのまま保持する。

例:

```text
PipeExpr(lhs, rhs)
MapExpr(lhs, rhs)
BindExpr(lhs, rhs)
KleisliExpr(lhs, rhs)
SafeBindExpr(lhs, rhs)
```

この段階ではまだ `Result` 用か `List` 用かを決めない。

### 理由

- エラーメッセージを元構文に対応させやすい
- 優先順位と結合規則の検証がしやすい
- `=?` を専用構文として保持できる

---

## 2. Typed HIR

型解決後の中間表現。

ここで初めて、演算子が `Result` 用か `List` 用かを決定する。

例:

```text
PipeExpr<TIn, TOut>
ResultMapValueExpr<TIn, TOut, E>
ResultMapFnExpr<TIn, TOut, TNext, E>
ListMapValueExpr<TIn, TOut>
ListMapFnExpr<TIn, TOut, TNext>
ResultBindValueExpr<TIn, TOut, E>
ResultBindFnExpr<TIn, TOut, TNext, E>
ListBindValueExpr<TIn, TOut>
ListBindFnExpr<TIn, TOut, TNext>
ResultKleisliExpr<TIn, TOut, TNext, E>
ListKleisliExpr<TIn, TOut, TNext>
SafeBindExpr<Pattern, T, E>
```

### この段階で行うこと

- `|*>` が `Result` か `List` かを確定
- `|>=` が `Result` か `List` かを確定
- `|=>` が `Result` か `List` かを確定
- 左右の文脈型一致確認
- `Result` のエラー型一致確認
- `|=>` の左右が関数であることを確認
- `=?` の右辺が `Result` であることを確認
- `=?` の左辺パターン妥当性を確認

### まだ行わないこと

- `match` や `flat_map` / `map` への完全展開

型エラーを演算子単位で出しやすくするため、この段階では意味解決までに留める。

---

## 3. Lowered IR / Core IR

ここで初めて desugar する。

## Result の `|*>`

```surtr
lhs |*> rhs
```

概念的には以下へ展開。

```surtr
match lhs {
  Ok(v) => Ok(rhs(v))
  Err(e) => Err(e)
}
```

## List の `|*>`

```surtr
lhs |*> rhs
```

概念的には以下へ展開。

```surtr
List::map(lhs, rhs)
```

または Core IR 上の専用反復命令列へ lower する。

List の公開 surface では `List::map` を使い、単位元側は `List::wrap(x)` と `[]` を使う。

## Result の `|>=`

```surtr
lhs |>= rhs
```

概念的には以下へ展開。

```surtr
match lhs {
  Ok(v) => rhs(v)
  Err(e) => Err(e)
}
```

## List の `|>=`

```surtr
lhs |>= rhs
```

概念的には以下へ展開。

```surtr
List::flat_map(lhs, rhs)
```

または Core IR 上の専用反復 + flatten 命令列へ lower する。

List の公開 surface では `List::flat_map` を使う。

## Result / List の `|=>`

```surtr
f |=> g
```

概念的には以下へ展開。

```surtr
{|x| f(x) |>= g }
```

その後さらに `|>=` 展開へ進む。

## `=?`

```surtr
pat =? expr
next
```

概念的には以下へ展開。

```surtr
match expr {
  Ok(v) => {
    pat = v
    next
  }
  Err(e) => return Err(e)
}
```

---

## 単純なインライン展開

ここでいう「単純なインライン展開」とは、演算子式をその場で

- 通常 call
- `match`
- `match` の連結
- `map` / `flat_map` 相当の専用 IR
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

### 例: `|*>`

```surtr
result |*> normalize()
```

は

```surtr
match result {
  Ok(v) => Ok(normalize(v))
  Err(e) => Err(e)
}
```

へ展開できる。

### 例: `|>=`

```surtr
result |>= validate()
```

は

```surtr
match result {
  Ok(v) => validate(v)
  Err(e) => Err(e)
}
```

へ展開できる。

### 例: `|=>`

```surtr
&parse |=> &validate
```

が即時適用される場合、

```surtr
input |> (&parse |=> &validate)
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
parse(str) |>= validate()
parse(str) |*> normalize()
input |> (&parse |=> &validate)
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
pipeline = &parse |=> &validate
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

## `=?` を専用ノードにする理由

`=?` は二項演算子に見えるが、性質としては通常の式演算子ではない。

### 理由

- 右辺 `Err` で早期リターンする
- 左辺に置ける構文が制限される
- 現在の関数やブロックの戻り型と結びつく
- `List` には適用されない

このため、parser では `SafeBindExpr` として専用ノードを立て、HIR/Lowering でも個別処理するのが望ましい。

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

このため、`SafeBind` と `|>=` は意味的に近いが、

- `=?` は **束縛構文**
- `|>=` は **Apply 演算子**

として分離して扱う。

---

## コンパイルエラー方針

エラーは「何が一致していないか」を演算子ごとに明示する。

### 例1: Result と List の混合

```surtr
parse |>= expand_many
```

想定エラー:

```text
`|>=` の左右で文脈型が一致していません
左辺: Result<User, AppError>
右辺: User -> List<Post>
`|>=` は Result と Result、または List と List のみ接続できます
```

### 例2: `|=>` の右辺が通常関数

```surtr
parse |=> normalize
```

想定エラー:

```text
`|=>` の右辺は文脈付き関数である必要があります
右辺: User -> NormalizedUser
期待: User -> Result<NormalizedUser, AppError>
```

### 例3: `=?` の右辺が SafeBind 対象でない場合

```surtr
x =? 1
```

想定エラー:

```text
`=?` の右辺は SafeBind 対象である必要があります
右辺: Int
期待: Result<T, E> または失敗しうるパターン入力
```

### 例4: `|*>` に文脈付き関数を与えた場合

```surtr
result |*> validate
```

想定エラー:

```text
`|*>` の右辺は通常関数である必要があります
右辺: User -> Result<User, AppError>
期待: User -> B
文脈付き関数を接続したい場合は `|>=` を使ってください
```

---

## 実装判断ルール

実装時の判断ルールを簡潔に書くと次の通り。

- **その場で消費されるなら、まず展開を試みる**
- **展開できるなら closure を作らない**
- **値として保持する必要がある場合のみ callable 化を考える**
- **capture がなければ synthetic function / function id を優先する**
- **capture がある場合のみ closure を生成する**

---

## 初期実装優先度

実装順は以下が望ましい。

### フェーズ1

- `|>`
- `Result` 向け `|*>`
- `Result` 向け `|>=`
- `Result` 向け `|=>`
- `=?`

### フェーズ2

- `List` 向け `|*>`
- `List` 向け `|>=`
- `List` 向け `|=>`

### 理由

`Result` は短絡制御とエラー伝播を担うため、`=?` と整合しやすい。言語の安全性・分かりやすさにも直結する。

一方 `List` は map / flat_map 的なデータ変換であり、制御構文とは切り離して後から導入してもよい。

---

## 最終仕様まとめ

### 採用する文脈

- `Result<A, E>`
- `List<A>`

### 採用する演算子

- `|>` : 値パイプ
- `|*>` : 同一文脈 map
- `|>=` : 同一文脈 bind
- `|=>` : 同一文脈の文脈付き関数合成
- `=?` : SafeBind

### 採用しないもの

- 一般化されたモナドインスタンス
- 暗黙 `pure` / `wrap`
- `Result` と `List` の混合 bind
- SafeBind 対象外の式に対する `=?`

### 核となる設計原則

- 文脈は型で決まる
- ただし文脈の種類は `Result` と `List` に限定する
- 演算子は共通でも、型規則と lowering で意味を分離する
- 暗黙変換ではなく明示的な接続のみ許可する
- `=?` は専用制御構文として扱う
- 可能な限り専用 lowering で吸収し、不要な closure を作らない

---

## 実装メモ

### Parser

- `|>` / `|*>` / `|>=` / `|=>` / `=?` を同一優先度・左結合で扱う
- `=?` の LHS 制約を構文段階で一部検出する
- call / closure / capture との結合順を明確に保つ
- `|*>` / `|>=` / `|=>` を最長一致でトークナイズする

### TypeChecker

- `MapExpr` を `ResultMap` / `ListMap` に解決
- `BindExpr` を `ResultBind` / `ListBind` に解決
- `KleisliExpr` を `ResultKleisli` / `ListKleisli` に解決
- `SafeBindExpr` は SafeBind 対象として検証
- 異種文脈混合を拒否
- `Result` 系はエラー型一致も確認する

### Lowering

- `ResultMap` → `match Ok/Err`
- `ListMap` → `map` 相当 IR
- `ResultBind` → `match Ok/Err`
- `ListBind` → `flat_map` 相当 IR
- `ResultKleisli` / `ListKleisli` → bind を使う callable へ変換
- `SafeBind` → `match + early return`
- 即時消費なら可能な限り branch chain へ直下ろしする

---

## 今後の拡張余地

将来 `FuncSection` が入る場合でも、Apply 右辺の規則は維持できる。

追加後の想定:

```surtr
value |> (`eq` 10 _)
result |*> (_ + 1)
result |>= check(_)
```

このときも基本方針は同じで、**1 引数 callable に解決できる式** として扱う。
