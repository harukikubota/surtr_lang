# xldr REPL Command Query 外部API整理

## 0. 目的

この文書は、`xldr` REPL の `:` command と command query の外部APIを整理するための仕様案である。

主眼は query parser の内部実装ではなく、ユーザーが入力する command surface と、その入力から導出する対象を明確にすることにある。

特に次を固定する。

- command query は Surtr 式ではない
- command query の引数は runtime value ではなく、型・binding・定義を引くための索引である
- literal / 任意式 / generic type variable は受けない
- REPL binding を明示する必要がある箇所では `$name` を使う
- 関数演算子の RHS では、実コードの引数注入規則に沿って限定的に query を許可する
- capture は command query 専用の `CaptureQuery` として扱い、通常 literal や任意式は許可しない

---

## 1. 全体モデル

### 1.1 command parser と query resolver の責務

REPL command は大きく次の段階で扱う。

```text
input line
  -> command head parser
  -> command payload parser
  -> command query parser
  -> semantic resolver
  -> command renderer
```

責務は次の通り。

| 層 | 責務 |
|---|---|
| command head parser | `:doc`, `:sig`, `:info` などの command 名を切り出す |
| command payload parser | command ごとの raw payload を取得する |
| command query parser | payload を command query token に分類する |
| semantic resolver | scope / binding / type / impl / lens 情報と照合する |
| command renderer | doc / sig / info / type / lens 表示へ落とす |

query parser は Surtr 式 parser ではない。

`1 + 2`, `to_string(x)`, `map(xs, to_string())` のような式・値評価を伴う入力は、原則として command query では扱わない。

---

## 2. コマンド一覧と役割

| Command | 目的 | 主な引数 |
|---|---|---|
| `:doc` | `@doc` を引く。定義 doc、impl doc、binding 起点 doc を表示する | `DocTarget` |
| `:sig` | 関数、constructor、extractor、enum variant surface、impl specialization の signature を表示する | `SigTarget` |
| `:info` | 定義、binding、dispatch、operator query の解決情報を表示する | `InfoTarget` |
| `:type` | REPL binding の型と TypeIdentity を表示する | `TypeTarget` |
| `:lens` | LensPath の型遷移、停止点、fallible segment を表示する | `LensTarget` |
| `:v` | history value または binding value を表示する | `HistoryIndex` / `BindingKey` |
| `:help` | command help を表示する | `Topic?` |
| `:error` | error 表示モードを確認・変更する | `full` / `summary` |
| `:save` | REPL session を保存する | `Path` |
| `:quit` | REPL を終了する | なし |

この文書では、主に `:doc`, `:sig`, `:info`, `:type`, `:lens` を対象にする。

---

## 3. 共通トークン設計

### 3.1 CommandQueryArg

command query の基本引数は次に限定する。

```text
CommandQueryArg
  = ConcreteTypeKey
  | BindingKey
  | ForcedBindingKey
  | CaptureQuery
```

### 3.2 ConcreteTypeKey

具象型を表す。

有効例:

```text
Int
String
Boolean
BitWidth
StringEncoding
Result<Int>
List<String>
(Int -> String)
(Int -> Result<String>)
```

無効例:

```text
$T
List<$T>
Result<$T>
($T -> String)
impl Numeric
```

`Result<Int>` のようにすべての型パラメータが具象型で確定している型式は許可する。

一方、型変数や `impl Trait` のように具体型が未確定な型式は command query では受けない。

理由:

- Surtr の型推論は具象型が決まった時点で行う
- command query で型変数を受けると、型推論を遅延できるように見える
- command query は型推論を抽象型のまま進める surface ではない

### 3.3 BindingKey

REPL 上の visible binding 名を表す。

```text
ret
up
xs
user
formatter
```

引数位置では、`name` は `ConcreteTypeKey` または `BindingKey` として解決される。

### 3.4 ForcedBindingKey

binding lookup を強制する。

```text
$name
```

例:

```text
:doc $b
:sig gt($left, $right)
:sig $ret |>= $up
:info $xs
:type $xs
```

`$name` は REPL command query 専用 token であり、Surtr 本体の変数記法ではない。

### 3.5 CaptureQuery

capture は command query の中では特例的に許可する。

ただし、通常 literal や任意式を許すわけではない。

```text
CaptureQuery
  = "&" CallableRef
  | "&" CallableRef "(" [CaptureQueryArg ("," CaptureQueryArg)*] ")"
```

```text
CaptureQueryArg
  = ConcreteTypeKey
  | BindingKey
  | ForcedBindingKey
  | CaptureSlot
  | PlaceholderFreeCapture
```

```text
CaptureSlot
  = "&1"
  | "&2"
  | "&3"
  | ...
```

```text
PlaceholderFreeCapture
  = "&" CallableRef
```

有効例:

```text
&to_string
&add(Int, &1)
&add(num, &1)
&add($num, &1)
&replace(String, &1, String)
&replace($from, &1, $to)
&List::map(&1, &to_string)
```

無効例:

```text
&add(10, &1)
&add("x", &1)
&add(&1 + &2, &1 * &3)
&pad_left(&1, width + 1)
&List::map(&1, &add(Int, &1))
```

無効理由:

- `10`, `"x"` は literal
- `&1 + &2` は expression
- `width + 1` は expression
- placeholder 付き capture の再帰は禁止

`&add(Int, &1)` は値 `Int` を渡すのではない。`Int` 型を索引として使う型導出用の capture query である。

---

## 4. パイプ・関数演算子 query

### 4.1 基本方針

関数演算子の query は、実コードの引数注入規則に合わせる。

Elixir と同じく、RHS が top-level 関数 call の場合、LHS は RHS call の引数に注入される。

```text
xs |> map(&to_string)
```

は次のように解釈される。

```text
map(xs, &to_string)
```

一方、次は closure 生成ではない。

```text
xs |> map(to_string())
```

これは次のように解釈される。

```text
map(xs, to_string())
```

`to_string()` は `to_string/0` であり、`to_string(_)` ではない。

### 4.2 TypedOperatorQuery

```text
TypedOperatorQuery
  = LhsKey OP OperatorRhs
```

```text
LhsKey
  = ConcreteTypeKey
  | BindingKey
  | ForcedBindingKey
```

```text
OperatorRhs
  = CallableKey
  | RhsTopLevelCall
```

```text
CallableKey
  = BindingKey
  | ForcedBindingKey
  | CaptureQuery
```

```text
RhsTopLevelCall
  = CallableRef "(" [RhsCallArg ("," RhsCallArg)*] ")"
```

```text
RhsCallArg
  = CommandQueryArg
  | PipePlaceholder
```

```text
PipePlaceholder
  = "_1"
```

### 4.3 `_1` の意味

`_1` はパイプ LHS の注入位置を表す marker である。

許可位置:

```text
RHS top-level function call の直接引数位置のみ
```

有効例:

```text
:sig text |> replace(_1, $from, $to)
:sig text |> replace($from, _1, $to)
:sig text |> replace($from, $to, _1)
```

正規化:

```text
replace(text, $from, $to)
replace($from, text, $to)
replace($from, $to, text)
```

無効例:

```text
:sig xs |> map(to_string(_1))
:sig xs |> map(add(Int, _1))
:sig n |> add(double(_1))
:sig n |> add(_1 + $x)
```

`_1` は closure を作る記法ではない。

### 4.4 `&1` との違い

| Token | 意味 | 位置 |
|---|---|---|
| `_1` | pipeline LHS の注入位置 | RHS top-level call の直接引数のみ |
| `&1`, `&2`, ... | CaptureQuery の引数 slot | `&Callable(...)` 内のみ |

例:

```text
:sig text |> replace($from, _1, $to)
```

これは LHS 注入位置指定。

```text
:sig xs |> map(&replace($from, &1, $to))
```

これは capture query。

### 4.5 有効な operator query

```text
:sig Int |> &to_string
:sig Int |> to_string()
:sig text |> replace($from, $to)
:sig text |> replace($from, _1, $to)
:sig xs |> map(&to_string)
:sig xs |> map(&add(Int, &1))
:sig xs |> map(&List::map(&1, &to_string))
:sig ret |>= up
:sig Result<Int> |>= &parse_int
```

### 4.6 無効な operator query

```text
:sig xs |> map(to_string())
:sig xs |> map(to_string(_1))
:sig xs |> map(add(Int, _1))
:sig xs |> map(&add(10, &1))
:sig xs |> map(&add(&1 + &2, &1 * &3))
:sig xs |> map(&List::map(&1, &add(Int, &1)))
:sig text |> replace("a", "b")
:sig List<$T> |> map(&to_string)
```

---

## 5. `:doc`

### 5.1 目的

`:doc` は `@doc` を引く command である。

入力形により、次の doc を引く。

- 定義 doc
- 型 doc
- constructor doc
- extractor doc
- trait / helper / operator doc
- trait impl doc
- binding 起点 doc

### 5.2 DocTarget

```text
DocTarget
  = DefinitionDocTarget
  | BindingDocTarget
  | ConstructorDocTarget
  | ExtractorDocTarget
  | TypedCallQuery
  | TypedOperatorQuery
```

```text
BindingDocTarget
  = "$" BindingName
```

### 5.3 定義 doc

有効例:

```text
:doc print
:doc Kernel::print
:doc +
:doc Add
:doc User
:doc StringEncoding
```

導出:

| 入力 | 導出対象 |
|---|---|
| `:doc print` | function/helper doc |
| `:doc Kernel::print` | qualified function doc |
| `:doc +` | operator alias / trait helper doc |
| `:doc Add` | deftrait doc |
| `:doc User` | struct / record type doc |
| `:doc StringEncoding` | enum doc |

### 5.4 impl doc dispatch

`:doc` は typed query により trait impl doc を引ける。

有効例:

```text
:doc gt(Int, Int)
:doc gt(left, right)
:doc gt($left, $right)
:doc ret |>= up
:doc Result<Int> |>= &parse_int
:doc xs |> map(&to_string)
```

導出順:

```text
1. selected impl method @doc
2. selected impl block @doc
3. trait method @doc
4. trait @doc
5. operator alias doc
6. no docs
```

### 5.5 constructor doc

構造体 / Record constructor は一意であるため、寛容に受ける。

有効例:

```text
:doc User()
:doc User(String, Int)
:doc User(name, age)
:doc User::new
:doc User::new()
:doc User::new(String, Int)
```

`User` 単体は `:doc` では type doc を意味する。

```text
:doc User        # type doc
:doc User()      # constructor doc
```

`User(String, Int)` の引数は runtime value ではなく、constructor 入力位置の型照合用 token である。

### 5.6 extractor doc

実コードでは出現位置により constructor / extractor を判定できるが、command では文脈がない。

そのため extractor は `!` を付ける。

有効例:

```text
:doc User!
:doc User!()
:doc User!(User)
:doc User!($user)
```

### 5.7 binding 起点 doc

binding から doc を引く場合は `$name` を使う。

```text
:doc $b
:doc $user
:doc $formatter
```

`$b` が Enum variant value の場合:

```text
b
kind: enum variant binding
type: BitWidth
identity: TypeIdentity::Enum
variant: BitWidth::Any(Int)

doc source:
  defenum BitWidth

<BitWidth の @doc>
```

### 5.8 `:doc name` と `:doc $name`

| 入力 | 解釈 |
|---|---|
| `:doc name` | definition lookup |
| `:doc $name` | binding lookup |

`:doc name` が定義として見つからず、同名 binding がある場合は案内する。

```text
No docs found for symbol `b`.

A REPL binding named `b` exists:
  b : BitWidth :: TypeIdentity::Enum

Try:
  :doc $b
```

---

## 6. `:sig`

### 6.1 目的

`:sig` は callable surface の signature を表示する。

対象:

- function/helper
- trait helper
- operator helper
- struct / record constructor
- struct / record extractor
- enum variant surface list
- callable binding
- impl specialization
- operator application query

### 6.2 SigTarget

```text
SigTarget
  = SymbolRef
  | QualifiedRef
  | OperatorRef
  | StructConstructorTarget
  | RecordConstructorTarget
  | EnumTarget
  | ExtractorTarget
  | TypedCallQuery
  | TypedOperatorQuery
  | CallableBindingTarget
```

### 6.3 function / helper signature

有効例:

```text
:sig add
:sig Add::add
:sig Kernel::print
:sig +
```

### 6.4 impl specialization signature

有効例:

```text
:sig gt(Int, Int)
:sig gt(left, right)
:sig gt($left, $right)
:sig ret |>= up
:sig Result<Int> |>= &parse_int
```

出力例:

```text
gt(self: Int, rhs: Int) -> Boolean
selected_impl: impl Gt for Int
```

### 6.5 struct / record constructor signature

構造体 constructor は一意であるため、次をすべて許可する。

```text
:sig User
:sig User()
:sig User(String, Int)
:sig User(name, age)
:sig User::new
:sig User::new()
:sig User::new(String, Int)
```

`User` は型でもあるが、`:sig User` では constructor signature として解釈する。

例:

```text
User(name: String, age: Int) -> User
```

`User(String, Int)` は constructor overload 選択ではない。constructor は一意であり、引数列は照合・診断用である。

### 6.6 extractor signature

有効例:

```text
:sig User!
:sig User!()
:sig User!(User)
:sig User!($user)
```

`!` は command 文脈で extractor を明示する token である。

### 6.7 enum signature

Enum は variant 単体を query target にしない。

受けるのは Enum 定義側のみ。

有効例:

```text
:sig StringEncoding
:sig BitWidth
```

出力例:

```text
* StringEncoding::Utf8
* StringEncoding::Ascii
```

```text
* BitWidth::W8
* BitWidth::W16
* BitWidth::W32
* BitWidth::W64
* BitWidth::W128
* BitWidth::Any(Int)
```

無効例:

```text
:sig StringEncoding::Utf8
:sig BitWidth::Any
:sig BitWidth::Any(Int)
```

診断:

```text
Enum variants are not query targets.

Variant constructors are displayed from the enum definition.

Try:
  :sig BitWidth
```

### 6.8 binding の signature

callable binding なら signature を表示してよい。

```text
:sig $formatter
```

値 binding なら無効。

Enum variant value の例:

```text
No signature found for b

`b` is an enum variant value:
  BitWidth::Any(Int)

Try:
  :info b
  :doc $b
  :sig BitWidth
```

---

## 7. `:info`

### 7.1 目的

`:info` は定義・binding・dispatch query の解決情報を表示する。

`:doc` や `:sig` が期待通りに解決されない時の調査 command としても使う。

### 7.2 InfoTarget

```text
InfoTarget
  = BindingKey
  | ForcedBindingKey
  | SymbolRef
  | QualifiedRef
  | OperatorRef
  | TypeRef
  | ConstructorTarget
  | ExtractorTarget
  | TypedCallQuery
  | TypedOperatorQuery
```

### 7.3 binding info

```text
:info b
:info $b
```

Enum variant binding の出力例:

```text
b
kind: binding
origin: repl
type: BitWidth
identity: TypeIdentity::Enum
value_kind: enum_variant
variant: BitWidth::Any
payload:
  0: Int

related:
  :doc $b
  :sig BitWidth
  :doc BitWidth
```

### 7.4 enum info

```text
:info StringEncoding
```

推奨表示:

```text
StringEncoding
kind: enum
origin: stdlib
defined: defenum StringEncoding { Utf8, Ascii }
variants:
  * StringEncoding::Utf8
  * StringEncoding::Ascii
```

`kind: function` は避ける。

### 7.5 dispatch info

```text
:info gt(Int, Int)
```

出力例:

```text
target: gt(Int, Int)
kind: function specialization

callable:
  gt

arg keys:
  Int
  Int

resolved arg types:
  Int
  Int

selected:
  impl Gt for Int

available:
  doc: yes
  sig: yes

related:
  :doc gt(Int, Int)
  :sig gt(Int, Int)
```

operator query:

```text
:info xs |> map(&to_string)
```

出力例:

```text
target:
  xs |> map(&to_string)

kind:
  operator application query

normalized:
  map(xs, &to_string)

resolved:
  xs: List<Int>
  &to_string: Int -> String

result:
  List<String>

related:
  :sig xs |> map(&to_string)
  :doc xs |> map(&to_string)
```

---

## 8. `:type`

### 8.1 目的

`:type` は REPL binding の型と TypeIdentity を表示する。

### 8.2 TypeTarget

```text
TypeTarget
  = BindingKey
  | ForcedBindingKey
```

有効例:

```text
:type b
:type $b
:type user
```

出力例:

```text
BitWidth :: TypeIdentity::Enum
```

無効例:

```text
:type StringEncoding
:type User
:type add
:type gt(Int, Int)
:type 1 + 2
```

`StringEncoding` は enum 定義であり binding ではないため、次のように失敗してよい。

```text
No binding found for StringEncoding
```

ただし案内を付けてもよい。

```text
`StringEncoding` is an enum definition, not a binding.

Try:
  :info StringEncoding
  :sig StringEncoding
  :doc StringEncoding
```

---

## 9. `:lens`

### 9.1 目的

`:lens` は LensPath の詳細を表示する。

表示すべき情報:

- canonical LensPath
- 各 segment の型遷移
- stop point
- Result を返しうる segment
- 最終的な Lens 型

### 9.2 LensTarget

```text
LensTarget
  = LensPathRef
  | ForcedBindingKey where binding is LensPath / Lens
```

有効例:

```text
:lens User.address.name
:lens Tuple._1
:lens $address_name_lens
```

無効例:

```text
:lens 1 + 2
:lens user.address.name   # value access expression として扱うなら無効
:lens map(xs, &to_string)
```

出力例:

```text
target: User.address.name

path:
  User
    .address : User -> Address
    .name    : Address -> String

result:
  Lens(User, String)

fallible:
  no
```

Result を返しうる path:

```text
target: Config.database.host

path:
  Config
    .database : Config -> Result<Database>
    .host     : Database -> String

result:
  Lens(Config, Result<String>)

fallible:
  yes

fallible segments:
  .database : Config -> Result<Database>
```

---

## 10. 定義種別ごとの導出内容

### 10.1 `def` / 通常関数

| Command | 入力 | 導出 |
|---|---|---|
| `:doc` | `:doc add` | function `@doc` |
| `:sig` | `:sig add` | defined signature |
| `:info` | `:info add` | callable info / origin / owner |
| `:type` | `:type add` | binding ではないため無効 |

通常関数は同一 scope で arity overload を持たない前提。

### 10.2 trait

| Command | 入力 | 導出 |
|---|---|---|
| `:doc` | `:doc Add` | trait `@doc` |
| `:doc` | `:doc +` | operator alias から trait/helper doc |
| `:sig` | `:sig +` | operator/helper signature |
| `:info` | `:info Add` | trait info |

### 10.3 trait impl

| Command | 入力 | 導出 |
|---|---|---|
| `:doc` | `:doc gt(Int, Int)` | selected impl method/block doc |
| `:sig` | `:sig gt(Int, Int)` | specialized signature |
| `:info` | `:info gt(Int, Int)` | dispatch 解決情報 |
| `:doc` | `:doc ret |>= up` | selected operator impl doc |
| `:sig` | `:sig ret |>= up` | operator application signature |

### 10.4 struct / record

| Command | 入力 | 導出 |
|---|---|---|
| `:doc` | `:doc User` | type doc |
| `:doc` | `:doc User()` | constructor doc |
| `:doc` | `:doc User(String, Int)` | constructor doc + arg照合 |
| `:sig` | `:sig User` | constructor signature |
| `:sig` | `:sig User()` | constructor signature |
| `:sig` | `:sig User(String, Int)` | constructor signature + arg照合 |
| `:doc` | `:doc User!()` | extractor doc |
| `:sig` | `:sig User!()` | extractor signature |
| `:info` | `:info User` | type info |

### 10.5 enum

Enum variant は直接 query target にしない。

| Command | 入力 | 導出 |
|---|---|---|
| `:doc` | `:doc StringEncoding` | enum doc |
| `:sig` | `:sig StringEncoding` | variant constructor surface 一覧 |
| `:info` | `:info StringEncoding` | enum definition info |
| `:type` | `:type StringEncoding` | binding ではないため失敗 |

無効:

```text
:doc StringEncoding::Utf8
:sig StringEncoding::Utf8
:info BitWidth::Any(Int)
```

### 10.6 enum variant binding

variant が入っている binding からは、binding 経由で doc / info を引ける。

```text
b = BitWidth::Any(32)
```

| Command | 入力 | 導出 |
|---|---|---|
| `:type` | `:type b` | `BitWidth :: TypeIdentity::Enum` |
| `:info` | `:info b` | binding info + variant detail |
| `:doc` | `:doc $b` | enum doc + variant detail |
| `:sig` | `:sig b` | 無効。`:sig BitWidth` へ誘導 |

### 10.7 builtin type

| Command | 入力 | 導出 |
|---|---|---|
| `:doc` | `:doc Int` | builtin type doc |
| `:info` | `:info Int` | builtin type info |
| `:sig` | `:sig Int` | constructor surface がないなら無効 |
| `:type` | `:type Int` | binding ではないため無効 |

### 10.8 callable binding

```text
f = &print
```

| Command | 入力 | 導出 |
|---|---|---|
| `:type` | `:type f` | closure/callable type |
| `:info` | `:info f` | binding provenance + callable info |
| `:doc` | `:doc $f` | callable provenance doc / type doc fallback |
| `:sig` | `:sig $f` | callable binding signature |

---

## 11. コマンド別許可トークン表

| Token / Shape | `:doc` | `:sig` | `:info` | `:type` | `:lens` |
|---|---:|---:|---:|---:|---:|
| `SymbolRef` | yes | yes | yes | no | no |
| `QualifiedRef` | yes | yes | yes | no | no |
| `OperatorRef` | yes | yes | yes | no | no |
| `ConcreteTypeKey` | yes | context | yes | no | LensPath 起点のみ |
| `BindingKey` | no for doc binding | callable/context | yes | yes | no |
| `$BindingKey` | yes | callable/context | yes | yes | yes if Lens |
| `TypedCallQuery` | yes | yes | yes | no | no |
| `TypedOperatorQuery` | yes | yes | yes | no | no |
| `ConstructorTarget` | yes | yes | yes | no | no |
| `ExtractorTarget` | yes | yes | yes | no | no |
| `EnumType` | yes | yes | yes | no | no |
| `EnumVariantRef` | no | no | no | no | no |
| `CaptureQuery` | in dispatch | in dispatch | in dispatch | no | no |
| literal | no | no | no | no | no |
| arbitrary expr | no | no | no | no | no |
| generic type variable | no | no | no | no | no |
| RHS top-level call | in operator | in operator | in operator | no | no |
| RHS arg function call | no | no | no | no | no |
| pipe placeholder `_1` | in operator RHS call | in operator RHS call | in operator RHS call | no | no |

---

## 12. エラーパターンと案内
コンパイラ、ランタイムが表示できる内容は継承しつつ、コマンドクエリから推察できる内容を案内する。 

### 12.1 unknown command

```text
:docs User
```

```text
Unknown command: :docs

Try:
  :doc User
  :help
```

### 12.2 missing argument

```text
:doc
```

```text
:doc requires a target.

Try:
  :doc print
  :doc User
  :doc User()
  :doc gt(Int, Int)
  :doc $binding
```

### 12.3 literal argument

```text
:sig gt(1, 2)
```

```text
literal arguments are not accepted in command queries.

Command queries use concrete types, bindings, or capture queries.

Try:
  :sig gt(Int, Int)

Or bind values first:
  left = 1
  right = 2
  :sig gt($left, $right)
```

### 12.4 arbitrary expression

```text
:sig gt(left + 1, right)
```

```text
expressions are not accepted in command queries.

Query arguments must be:
  - a concrete type
  - a visible binding
  - a forced binding: $name
  - a capture query where allowed

Try:
  tmp = left + 1
  :sig gt($tmp, right)
```

### 12.5 generic type variable

```text
:sig map(List<$T>, ($T -> String))
```

```text
generic type variables are not accepted in command queries.

Command queries are resolved with concrete types or REPL bindings.

Try:
  :sig map(List<Int>, (Int -> String))
  :sig map($xs, &to_string)
```

### 12.6 binding doc without `$`

```text
:doc b
```

定義 `b` がなく、binding `b` がある場合:

```text
No docs found for symbol `b`.

A REPL binding named `b` exists:
  b : BitWidth :: TypeIdentity::Enum

To inspect docs through the binding value, use:
  :doc $b
```

### 12.7 `$` binding not found

```text
:doc $missing
```

```text
No binding found for missing
```

### 12.8 enum variant direct query

```text
:sig BitWidth::Any(Int)
```

```text
Enum variant constructor queries are not supported.

Variant constructors are displayed from the enum definition.

Try:
  :sig BitWidth
  :doc BitWidth
  :info BitWidth
```

### 12.9 `:sig` on enum variant binding

```text
:sig b
```

```text
No signature found for b

`b` is an enum variant value:
  BitWidth::Any(Int)

Try:
  :info b
  :doc $b
  :sig BitWidth
```

### 12.10 RHS argument function call

```text
:sig xs |> map(to_string())
```

```text
function calls are not accepted as RHS call arguments.

`to_string()` is parsed as a zero-argument call.
It does not create a closure.

Try:
  :sig xs |> map(&to_string)
```

### 12.11 `_1` invalid position

```text
:sig xs |> map(to_string(_1))
```

```text
`_1` is only allowed as a direct argument of the RHS top-level call.

It marks where the pipeline subject is injected.
It does not create a closure.

Try:
  :sig xs |> map(&to_string)
```

### 12.12 capture query literal

```text
:sig xs |> map(&add(10, &1))
```

```text
literal values are not accepted in command capture queries.

Found:
  &add(10, &1)

Command capture queries use concrete types, bindings, or capture slots.

Try:
  :sig xs |> map(&add(Int, &1))

Or bind the value first:
  num = 10
  :sig xs |> map(&add($num, &1))
```

### 12.13 capture query expression

```text
:sig xs |> map(&add(&1 + &2, &1 * &3))
```

```text
expressions are not accepted in command capture queries.

Capture queries are type-index patterns, not executable closures.

Try:
  :sig xs |> map(&add(Int, &1))
```

### 12.14 recursive placeholder capture

```text
:sig xs |> map(&List::map(&1, &add(Int, &1)))
```

```text
nested placeholder captures are not supported.

A capture query that uses &1, &2, ... cannot contain another placeholder capture.
Use a named helper or a placeholder-free capture inside it.

Try:
  helper = ...
  :sig xs |> map($helper)
```

### 12.15 constructor argument mismatch

```text
:sig User(Int, String)
```

```text
constructor query does not match constructor input types.

constructor:
  User(name: String, age: Int) -> User

query:
  User(Int, String)

mismatch:
  argument 1: expected String, got Int
  argument 2: expected Int, got String

Try:
  :sig User(String, Int)
```

---

## 13. Help 文言案

### 13.1 `:doc`

```text
Usage:
  :doc <symbol>
  :doc <type>
  :doc <constructor>
  :doc <extractor>
  :doc <typed-call>
  :doc <typed-operator>
  :doc $<binding>

Examples:
  :doc StringEncoding
  :doc User
  :doc User()
  :doc User(String, Int)
  :doc User!()
  :doc gt(Int, Int)
  :doc ret |>= up
  :doc $b
```

### 13.2 `:sig`

```text
Usage:
  :sig <function>
  :sig <operator>
  :sig <constructor>
  :sig <extractor>
  :sig <enum>
  :sig <typed-call>
  :sig <typed-operator>

Examples:
  :sig add
  :sig +
  :sig User
  :sig User(String, Int)
  :sig User!()
  :sig StringEncoding
  :sig gt(Int, Int)
  :sig xs |> map(&to_string)
```

### 13.3 `:info`

```text
Usage:
  :info <symbol>
  :info <type>
  :info <binding>
  :info $<binding>
  :info <typed-call>
  :info <typed-operator>

Examples:
  :info b
  :info $b
  :info StringEncoding
  :info gt(Int, Int)
  :info xs |> map(&to_string)
```

### 13.4 `:type`

```text
Usage:
  :type <binding>
  :type $<binding>

Examples:
  :type b
  :type $b
```

### 13.5 `:lens`

```text
Usage:
  :lens <LensPath>
  :lens $<lens-binding>

Examples:
  :lens User.address.name
  :lens Tuple._1
  :lens $address_name_lens
```

---

# 付録A: パーサ設計

## A.1 設計方針

command query parser は Surtr 式 parser ではない。

目的は、次を軽量に分類することである。

- definition lookup
- binding lookup
- constructor lookup
- extractor lookup
- typed call dispatch
- typed operator dispatch
- capture query
- lens path lookup

評価・実行・任意式の型推論は行わない。

## A.2 推奨 AST

```text
CommandQuery
  = DefinitionLookup(DefinitionRef)
  | BindingLookup(BindingRef)
  | ConstructorLookup(ConstructorQuery)
  | ExtractorLookup(ExtractorQuery)
  | TypedCallDispatch(TypedCallQuery)
  | TypedOperatorDispatch(TypedOperatorQuery)
  | LensLookup(LensQuery)
```

```text
QueryArg
  = ConcreteType(TypeExpr)
  | Binding(Name)
  | ForcedBinding(Name)
  | Capture(CaptureQuery)
```

```text
CaptureQuery
  = CaptureRef(CallableRef)
  | CaptureCall {
      callable: CallableRef,
      args: Vec<CaptureArg>,
    }
```

```text
CaptureArg
  = ConcreteType(TypeExpr)
  | Binding(Name)
  | ForcedBinding(Name)
  | CaptureSlot(usize)
  | PlaceholderFreeCapture(CallableRef)
```

```text
TypedOperatorQuery
  = {
      lhs: QueryArg,
      op: OperatorToken,
      rhs: OperatorRhs,
    }
```

```text
OperatorRhs
  = Callable(QueryArg)        # callable binding / forced binding / capture query
  | TopLevelCall(RhsCall)
```

```text
RhsCall
  = {
      callable: CallableRef,
      args: Vec<RhsCallArg>,
    }
```

```text
RhsCallArg
  = QueryArg
  | PipePlaceholder
```

```text
PipePlaceholder
  = _1
```

## A.3 parse order

推奨順:

```text
1. command head を切る
2. command ごとの scalar payload か query payload かを決める
3. `$name` なら ForcedBindingKey / BindingLookup
4. operator token を top-level で探索する
5. constructor / extractor pattern を見る
6. typed call pattern を見る
7. capture query を見る
8. lens path を見る
9. symbol / qualified ref として扱う
10. semantic resolver に渡す
```

## A.4 top-level operator 探索

top-level operator の探索では、次を考慮する。

- `(...)` depth
- `<...>` depth
- capture query 内部
- string literal は command query では基本的に無効だが、エラー位置を出すため lexer では認識してよい

operator tokens:

```text
|>=
|*>
|>
>=>
>*
>>
```

## A.5 call argument split

`Callable(args...)` の arg split では、次を考慮する。

- parentheses depth
- type expression depth
- capture query depth
- `,` は top-level のみ separator

ただし、arg に nested function call を許すわけではない。

例えば:

```text
map(to_string())
```

は split できても、その後の token validation で `RHS argument function call` として無効にする。

## A.6 token validation

parse 後に command ごとの validation を行う。

例:

| 検出 | エラー |
|---|---|
| literal token | literal arguments are not accepted |
| `$T` in type expr | generic type variables are not accepted |
| `to_string()` in RHS call arg | function calls are not accepted as RHS call arguments |
| `_1` outside RHS top-level direct arg | invalid pipe placeholder position |
| `&add(10, &1)` | literal in capture query |
| `&add(&1 + &2, &1 * &3)` | expression in capture query |
| nested placeholder capture | recursive placeholder capture unsupported |
| enum variant direct target | enum variants are not query targets |

## A.7 semantic resolver の入力

parser は意味を確定しない。

resolver に次を渡す。

```text
ResolvedQueryRequest
  = Doc(DocQuery)
  | Sig(SigQuery)
  | Info(InfoQuery)
  | Type(TypeQuery)
  | Lens(LensQuery)
```

resolver が見る情報:

- visible binding table
- current scope functions
- auto-import helpers
- explicit imports
- type owner table
- trait table
- impl table
- constructor / extractor metadata
- enum definitions
- lens metadata

## A.8 diagnostic span

diagnostic span は局所 token に寄せる。

例:

```text
:sig xs |> map(&add(10, &1))
                   ^^ literal token
```

```text
:sig xs |> map(to_string())
              ^^^^^^^^^^^ RHS argument function call
```

```text
:sig xs |> map(to_string(_1))
                        ^^ invalid pipe placeholder position
```

## A.9 parser を複雑化させないための境界

受けないものを明確にする。

```text
- literal value
- arbitrary expression
- nested function call as value
- generic type variable
- direct enum variant query
- pipe placeholder as expression
- capture query with literal/expression
- recursive placeholder capture
```

これにより、command query parser は小さく保てる。

---

# 付録B: 代表入力一覧

## 有効

```text
:doc StringEncoding
:sig StringEncoding
:info StringEncoding

:type b
:info b
:doc $b

:doc User
:sig User
:sig User()
:sig User(String, Int)
:doc User()
:doc User(String, Int)
:sig User!()

:doc gt(Int, Int)
:sig gt(Int, Int)
:info gt(Int, Int)

:sig ret |>= up
:doc Result<Int> |>= &parse_int
:info xs |> map(&to_string)
:sig xs |> map(&add(Int, &1))
:sig text |> replace($from, _1, $to)
```

## 無効

```text
:doc b                         # binding doc は :doc $b
:sig BitWidth::Any(Int)         # enum variant direct query 不可
:type StringEncoding            # binding ではない

:sig gt(1, 2)                   # literal 不可
:sig gt(left + 1, right)        # expression 不可
:sig List<$T>                   # generic type variable 不可

:sig xs |> map(to_string())     # RHS arg function call 不可
:sig xs |> map(to_string(_1))   # _1 nested 不可
:sig xs |> map(&add(10, &1))    # capture query 内 literal 不可
:sig xs |> map(&add(&1 + &2, &1 * &3)) # capture query 内 expression 不可
:sig xs |> map(&List::map(&1, &add(Int, &1))) # placeholder capture 再帰不可
```
