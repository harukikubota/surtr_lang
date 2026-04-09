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

### `Result<T>`

- 成功値: `Ok(value)`
- 失敗値: `Err(error)`

現時点では、`match` で主に `Ok(...)` / `Err(...)` を扱います。  
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

## 7. 組込み関数

| 名前 | 型 |
|---|---|
| `if` | `(Boolean, (-> $A), (-> $A)) -> $A` |
| `if_then` | `(Boolean, (-> Unit)) -> Unit` |
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
- パイプライン `|>`
- マクロシステム拡張
- 並列コンパイル
- 高度なモジュールシステム拡張

正本としての詳細仕様は [要件定義v9](../要件定義v9.md) を参照してください。
