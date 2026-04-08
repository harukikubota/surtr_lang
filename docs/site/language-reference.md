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
- 関数型 `(T1, T2, ...) -> R`
- ユーザ定義型

### `Result<T>`

- 成功値: `Ok(value)`
- 失敗値: `Err(error)`

現時点では、`match` で主に `Ok(...)` / `Err(...)` を扱います。

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

- `True`
- `False`
- `Ok(x)`
- `Err(e)`
- `_`
- `Int` リテラル
- `String` リテラル

## 6. フィールドアクセス

```surtr
value.field
```

`defstruct` と `defrecord` の両方で使えます。

## 7. 組込み関数

| 名前 | 型 |
|---|---|
| `print` | `(String) -> Unit` |
| `to_string` | `($A) -> String` |
| `inspect` | `($A) -> String` |
| `safe_div` | `($A, $A) -> Result<$A>` |
| `safe_mod` | `(Int, Int) -> Result<Int>` |
| `eprint` | `(Error) -> Unit` |
| `set_exit_code` | `(Int) -> Unit` |

### 補足

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

## 9. モジュールと import

### 標準モジュール

現在の標準モジュール層は次の順序でロードされます。

```text
Bootstrap -> Kernel -> [他標準モジュール] -> ユーザ拡張
```

### auto import

- `Bootstrap` と `Kernel` は auto import 対象
- `Bootstrap` / `Kernel` の明示 `import` は compile error
- それ以外の標準モジュールは auto import しない

### import の重複

同一 file では、同じモジュールまたは同じメンバーの再 import を禁止します。

禁止例:

```surtr
import Kernel;
import Kernel;
```

```surtr
import Kernel;
import Kernel::add;
```

## 10. `@@builtin`

`@@builtin def ...` は標準モジュール source でのみ使えます。

- user script では使えない
- user module では使えない
- REPL でも使えない

これは「builtin をユーザーが追加するための構文」ではなく、「処理系内の共有 builtin テーブルを Surtr source 側から宣言するための構文」です。

## 11. 現在のスコープ外

このリファレンスでは扱わないもの:

- trait
- 型エイリアス / NewType
- パイプライン `|>`
- マクロシステム拡張
- 並列コンパイル
- 高度なモジュールシステム拡張

正本としての詳細仕様は [要件定義v9](../要件定義v9.md) を参照してください。
