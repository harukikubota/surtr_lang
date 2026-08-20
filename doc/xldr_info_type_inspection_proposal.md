# Xldr `:info` 型定義表示の修正案

## 目的

`:info` を、対象シンボルが何として定義されているかを調べる軽量な
definition inspection command とする。

- 関数一覧、enum variant 一覧、実装メソッド一覧は表示しない
- 関数や variant の詳細は `:doc` / `:sig` の責務とする
- 現在の `No signature found for <name>` を、未定義シンボルに対する
  正しい通知へ置き換える

この文書は実装前の修正案である。`docs/dev/Xldr_spec.md` は、承認後に
本提案を反映して更新する。

## 型・owner の `:info` 表示

型、module、trait の definition inspection は、名前、kind、origin だけを
表示する。`defined:` は表示しない。これは kind が既に宣言形態を表しており、
source 形式の重複表示に情報量がないためである。

### Kind の対応

| 宣言・状態 | `kind` |
|---|---|
| `defmod Name` | `module` |
| `@builtin type Name` | `builtin type` |
| `@builtin type Name` と `impl Name` | `builtin type, module` |
| `defstruct Name` | `struct` （`impl Name` があれば `struct, module`） |
| `defrecord Name` | `record` （`impl Name` があれば `record, module`） |
| `defenum Name` | `enum` （`impl Name` があれば `enum, module`） |
| `deferror Name` | `error` |
| `deftrait Name` | `trait` |
| `deftrait Name where Self: Type<...>` | `type constructor` |
| 未解決名 | `is not defined in the current REPL scope.` |

具象 error は module owner になれず trait の実装対象にもならない。そのため
`error` には `, module` も `implements:` も付けない。

### Trait 実装の要約

型が実装する trait は、メソッドを展開せず次の 1 行だけで示す。

```text
implements: Default, Add, Sub, Mul, Eq, Neq, Compare, Show, From, TryFrom
```

`implemented:` ではなく `implements:` を採用する。照会対象の型を主語として、
その型が列挙された trait を実装するという関係を直接示せるためである。

`implements:` は builtin type、struct、record、enum のみで表示する。trait、
module、concrete error には表示しない。該当する trait 実装がなければ行自体を
省略する。

### 例

```text
xldr(1)> :info Int
Int
kind: builtin type, module
origin: stdlib
implements: Default, Add, Sub, Mul, Eq, Neq, Compare, Show, From, TryFrom

xldr(2)> :info Kernel
Kernel
kind: module
origin: stdlib

xldr(3)> :info Add
Add
kind: trait
origin: stdlib

xldr(4)> :info Functor
Functor
kind: type constructor
origin: stdlib

xldr(5)> :info Undef
Undef is not defined in the current REPL scope.
```

## Concrete error の型と constructor

`deferror Name` は error type と constructor を同時に宣言する。既存の
struct / record owner-constructor 分離と同じ query 形式を用いる。

| Query | 対象 | 表示・解決規則 |
|---|---|---|
| `NoneError` | error type | error type を inspection する |
| `NoneError(...)` | constructor | 括弧内の値は query 解決で無視する |
| `:doc NoneError` | error type doc | canonical error doc を表示する |
| `:doc NoneError()` | constructor doc | constructor 専用 doc がなければ error type doc を再利用する |
| `:sig NoneError` | constructor | constructor signature を表示する |
| `:sig NoneError()` | constructor | constructor signature を表示する |
| `:info NoneError` | error type | `kind: error` を表示する |
| `:info NoneError()` | constructor | callable として表示する |

error type query の例:

```text
xldr(1)> :info NoneError
Global::NoneError
kind: error
origin: stdlib
```

constructor query は struct constructor と同じ callable レイアウトにする。
この場合の `defined:` は型定義の重複ではなく、constructor signature を示す行なので
維持する。

```text
xldr(2)> :info NoneError()
NoneError()
kind: function
origin: stdlib
defined: NoneError() -> Error
```

`deferror` の source 定義には constructor の戻り値を書かない。しかし REPL の
signature では concrete error constructor であることを明確にするため、戻り値を
常に `Error` として補う。

```text
xldr(3)> :sig NoneError
NoneError() -> Error

xldr(4)> :sig NoneError()
NoneError() -> Error
```

この表示では `NoneError() -> NoneError` としない。error constructor の public
return type は `Error` に固定する。

## 対象外

- `Ok/1`、`Err/1`、`True`、その他 builtin-special enum variant / function surface の
  現行分類は変更しない
- `Global::` の user-facing 表示からの隠蔽は別タスクとし、本変更では登録済みの
  canonical name をそのまま表示する
- `defmod NoneError` を受理してしまう問題は別タスクとする
- 関数、variant、trait 実装メソッドの列挙や source snippet の表示は追加しない
