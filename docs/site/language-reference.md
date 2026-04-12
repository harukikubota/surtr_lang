# Surtr Language Reference

このページは、現時点で確定している Surtr の言語仕様をコンパクトに引けるようにまとめたものです。

## 1. 基本構文

### 束縛

```surtr
name = expr
name: Ty = expr
name =? expr
```

### 関数

```surtr
def name(args...) -> Ty { expr }
```

### データ定義

```surtr
defstruct Name {
  field: Ty,
}

defrecord Name(field: Ty, ...)

deferror Name(field: Ty, ...) { "message" }

defenum Name { Variant, Variant(Ty), Variant = Int, ... }

impl Type {
  def method(...) -> ... { ... }
}
```

### 制御構造

```surtr
if(cond, then_expr, else_expr)
if_then(cond, expr)

match expr {
  pattern => expr,
  ...
}
```

## 2. 型

### 基本型

- `Int`
- `Float`
- `String`
- `Boolean`
- `Unit`

### 合成型

- `List<T>`
- `Result<T>`
- `Enum`
- 関数型 `(T1, T2, ...) -> R`
- ユーザ定義型

### `Enum`

- `defenum` で定義する
- 値生成は `Enum::Variant(...)`
- `match` は網羅必須
- enum 値への field access（例: `.idx`）は不可

### `impl` / `Self` / `self`

- `impl` 対象は `defstruct` / `defenum` のみ
- `Self` は `impl` 内の型位置でのみ使用可能
- `self` は `impl` メソッド第一引数専用（再束縛不可）
- メソッド呼び出しの正規形は `Type::method(...)`

### `Result<T>`

- 成功値: `Ok(value)`
- 失敗値: `Err(error)`

現時点では、`match` を中心に `Ok(...)` / `Err(...)` を扱います。  
variant 判定だけなら `Result::is_ok(...)` / `Result::is_err(...)` も使えます。  
考え方としては `Either<Err, Ok>` に近く、失敗も値として明示的に運びます。
内部表現は enum-like ですが、language surface では `defenum` と区別された専用 abstraction です。

### 戻り値位置の `Result<T, E>`

関数シグネチャでは `Result<T, E>` という表記が現れることがあります。

- builtin type declaration の canonical head は `Result<T>`
- `E` は `Err` 側の error contract を説明する補助表記
- 値として保持される型の中心は引き続き `Result<T>`

### `Error`

- recoverable failure を受ける抽象型
- `deferror` で定義した具体 error がここへ流れ込む
- `Error` 自体をユーザーが直接具体化する前提ではない

## 3. リテラル

### 数値

```surtr
1
10
1.5
```

### 真偽値

```surtr
True
False
```

### 文字列

```surtr
"hello"
"hello #{name}"
```

### リスト

```surtr
[1, 2, 3]
["a", "b", "c"]
[]
```

## 4. 演算子

### 算術

- `+`
- `-`
- `*`

### 比較

- `<`
- `<=`
- `>`
- `>=`
- `==`
- `!=`

### 文字列結合

- `++`

### パイプ / bind / compose

- `|>`
- `|*>`
- `|>=`
- `>>`
- `|=>`
- `=?`

#### `|>` 値 apply

`|>` は左辺の値を右辺へ流します。

- 右辺が capture / closure の場合は unary callable として適用する
- 右辺が call 式の場合は、左辺値を第一引数へ注入する

```surtr
value |> &normalize
value |> normalize(10)
user |> User::get_name()
```

意味:

```surtr
value |> normalize(10)      # => normalize(value, 10)
user |> User::get_name()    # => User::get_name(user)
```

#### `|*>` 文脈 map

`|*>` は `Result` または `List` の中の値だけを変換します。

- `Result<A> |*> (A -> B)` は `Result<B>`
- `List<A> |*> (A -> B)` は `List<B>`
- 右辺が call 式なら、文脈内部の値が第一引数へ注入される

```surtr
Ok(1) |*> add(2)            # => Ok(add(1, 2))
["a", "b"] |*> wrap("[", "]")
```

`|*>` の右辺は plain function である必要があります。  
`A -> Result<B>` や `A -> List<B>` のような文脈付き関数は受けません。

#### `|>=` 文脈 bind

`|>=` は `Result` / `List` の文脈を維持したまま次の段階へ接続します。

- `Result<A> |>= (A -> Result<B>)`
- `List<A> |>= (A -> List<B>)`

```surtr
Ok(11) |>= require_at_least(10)
[1, 2, 3] |>= expand()
```

`|>=` の右辺が call 式なら、文脈内部の値を第一引数へ注入します。

```surtr
Ok(11) |>= require_at_least(10)   # => require_at_least(11, 10)
```

#### `>>` 通常関数合成

`>>` は plain function / closure の合成です。

```surtr
pipeline = &trim >> &render
```

左右とも closure value でなければなりません。  
`trim() >> render()` のような call 式は不許可です。

#### `|=>` Kleisli 合成

`|=>` は `Result` / `List` を返す関数同士を合成します。

```surtr
pipeline = &parse |=> &validate
```

- `Result` なら `(A -> Result<B>) |=> (B -> Result<C>)`
- `List` なら `(A -> List<B>) |=> (B -> List<C>)`

これも compose なので、左右とも capture または closure に限ります。  
`parse() |=> validate()` は不許可です。

#### `=?` SafeBind

`=?` は「失敗したらそのまま伝播する束縛」です。

```surtr
value: Int =? parse_int("1")
[head, ..tail] =? [1, 2, 3]
[head, ..tail] =? Ok([1, 2, 3])
[first, ..tail] =? "source"
```

- `pattern =? Result<T, E>` は `Ok` を束縛し、`Err` を早期伝播する
- `pattern =? expr` は SafeBind 対象の失敗しうるパターン入力を扱う
- 現時点の対象は `Result`、`List`、`String`
- `[head, ..tail]` は MatchBlock では `List` / `String` の分解に使えるが、Expr 位置では list 構築のまま

#### 共通制約

- 裸の関数参照は許可しない
- `value |> normalize` は不許可
- `pipeline = parse |=> validate` も不許可
- 関数値として保持できるのは capture または closure
- backtick FuncLiteral は中置位置専用で、値にはならない
- ``left `name` right`` は `name(left, right)` に lower される
- ``left `operator` right`` は対応する通常演算に lower される
- V1 の FuncLiteral は unqualified name と symbolic operator のみを許可する
- `` `Type::method` `` のような qualified backtick path は未対応
- `&` 側で operator を capture する構文と placeholder capture (`&1`) は未実装
- bare capture を `inspect` / `to_string` すると、metadata があれば
  `FnCapture(module: M, name: f, signature: sig)` 形式で表示する
- `Result` と `List` を `|*>`, `|>=`, `|=>` で混在させない
- `|>`, `|*>`, `|>=`, `|=>`, `=?` は同一優先度・左結合
- 結合優先度は `Bind < Apply=Compose < Logical < Expr`
- `Expr` クラスの `+`, `-`, `*`, `++` は同列・左結合
- comparison 系 (`==`, `!=`, `<`, `>`, `<=`, `>=`) は `Logical` クラス

## 5. パターン

現時点で確定している `match` パターンは次のとおりです。

- binding pattern
- `True`
- `False`
- `Ok(x)`
- `Err(e)`
- `_`
- `Int` リテラル
- `String` リテラル
- list pattern
- 入れ子になった constructor pattern

## 6. フィールドアクセス

```surtr
value.field
```

`defstruct` と `defrecord` の両方で使えます。`defenum` では使えません。

### `defstruct` の構築規約

- `impl struct` では `new` を必須実装とする
- `Type(...)` は `Type::new(...)` の糖衣として解決される
- `Type { ... }` 構造体リテラルは `impl Type` の同型メソッド本体内でのみ使用可能
- `Type::new` は import 対象外

### 引数規約

- 名前付き引数は利用可能
- 位置引数と名前付き引数の混在は禁止

## 7. 組込み関数

| 名前 | 型 |
|---|---|
| `if` | `(Boolean, (-> $A), (-> $A)) -> $A` |
| `if_then` | `(Boolean, (-> Unit)) -> Unit` |
| `assert` | `(Boolean, Error) -> Result<Unit>` |
| `ensure` | `($A, ($A -> Boolean), Error) -> Result<$A>` |
| `and` | `(Boolean, Boolean) -> Boolean` |
| `or` | `(Boolean, Boolean) -> Boolean` |
| `eq` | `($A, $A) -> Boolean` |
| `neq` | `($A, $A) -> Boolean` |
| `lt` | `($A, $A) -> Boolean` |
| `lte` | `($A, $A) -> Boolean` |
| `gt` | `($A, $A) -> Boolean` |
| `gte` | `($A, $A) -> Boolean` |
| `concat` | `(String, String) -> String` |
| `print` | `(String) -> Unit` |
| `to_string` | `($A) -> String` |
| `inspect` | `($A) -> String` |
| `safe_div` | `($A, $A) -> Result<$A>` |
| `safe_mod` | `(Int, Int) -> Result<Int>` |
| `eprint` | `(Error) -> Unit` |
| `set_exit_code` | `(Int) -> Unit` |

### 補足

- `if` / `if_then` の branch が関数型で書かれているのは、選ばれた側だけを評価する special form であることを型で表しているため
- 普段の source では block を明示せず `if(flag, "ok", err_reason)` や `if_then(flag, print("ok"))` のように書ける
- `and` / `or` は宣言上は普通の 2 引数関数だが、コンパイラが short-circuit として解釈する
- `eq` / `neq` は call-style helper で、`==` / `!=` と同じ比較制約に従う
- `lt` / `lte` / `gt` / `gte` は call-style helper で、`<` / `<=` / `>` / `>=` と同じ比較制約に従う
- `concat` は call-style helper で、`++` と同じく `String` 同士だけを受ける
- `safe_div` / `safe_mod` は失敗時に `Err(ZeroDivisionError)` を返す
- `set_exit_code` は処理系側で使用位置制約を持つ

## 8. 標準エラー

標準モジュール層で最初から提供される汎用 error には、少なくとも次が含まれます。

```surtr
deferror NoneError { "None Value." }
deferror ZeroDivisionError { "division by zero" }
```

現在の実装には次も含まれます。

```surtr
deferror EmptyList { "Empty List." }
deferror IndexOutOfBounds(detail: String) { detail }
```

これらは `Error` 抽象に乗る具体 error です。

## 9. モジュールと import

### 標準モジュール

現在の標準モジュール層は次の順序でロードされます。

```text
Bootstrap -> [Kernel, Int, String, Boolean, Error, List, Result, Float] -> ユーザ拡張
```

### auto import

- `Bootstrap` と `Kernel` は auto import 対象
- `Bootstrap` / `Kernel` の明示 `import` は compile error
- それ以外の標準モジュールは auto import しない

### builtin type の置き場所

各 builtin type は、対応する標準 module file のトップレベルで宣言します。

```surtr
// kernel.srt
@@builtin type Unit

// int.srt
@@builtin type Int

// list.srt
@@builtin type List<$A>

// result.srt
@@builtin type Result<$T>
```

`unit.srt` は意図的に作らず、`Unit` だけは `kernel.srt` に置きます。

### import の重複

同一 file では、同じモジュールまたは同じメンバーの再 import を禁止します。

禁止例:

```surtr
import Kernel;
import Kernel;
```

```surtr
import Kernel;
import Kernel::print;
```

## 10. `@@builtin` と `@@doc`

`@@builtin def ...` は標準モジュール source でのみ使えます。

- user script では使えない
- user module では使えない
- REPL でも使えない

これは「builtin をユーザーが追加するための構文」ではなく、「処理系内の共有 builtin テーブルを Surtr source 側から宣言するための構文」です。

`@@builtin type ...` も同じく標準モジュール source 専用です。  
各標準 module file の top-level に置いて、compiler が canonical head と照合します。

`@@doc """..."""` は `defmod` / `def` / `deferror` / `@@builtin type` / `@@builtin def` の直前に置けます。  
標準ライブラリではこの仕組みを使って source に API 説明を埋め込みます。

`Result` には declaration-only の special constructor head もあります。

```surtr
@@builtin type Ok($T) -> Result<$T>
@@builtin type Err(Error) -> Result<$T>
```

これらは通常の関数本体付き `def` ではなく、標準モジュール `result.srt` で compiler が特別扱いする surface contract です。

## 11. 現在のスコープ外

このリファレンスでは扱わないもの:

- trait
- 型エイリアス / NewType
- マクロシステム拡張
- 並列コンパイル
- 高度なモジュールシステム拡張

正本としての詳細仕様は [要件定義v9](../要件定義v9.md) を参照してください。
