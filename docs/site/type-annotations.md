# Type Annotations

Surtr の型注釈は、このページにまとめます。

## 基本形

よく使う形は次の 3 つです。

```surtr
score: Int = 10

def add(x: Int, y: Int) -> Int { x + y }

inc_fn: (Int -> Int) = &inc
```

- 束縛: `name: Ty = expr`
- 引数: `def name(arg: Ty) -> Ret`
- 戻り値: `-> Ret`
- 関数型: `(A -> B)`

## 使える型

- 基本型
  - `Int`
  - `Float`
  - `String`
  - `Boolean`
  - `Unit`
- 合成型
  - `List<T>`
  - `Result<T>`
  - 関数型 `(T1, T2, ...) -> R`
  - ユーザ定義型

## generic annotation

generic parameter は `$` 付きで書きます。

```surtr
def id<$A>(value: $A) -> $A { value }
```

trait bound を付けるときは次の形です。

```surtr
def twice<$N: Numeric>(x: $N) -> $N {
  x + x
}
```

## `Result<T>`

Result 系の戻り値注釈は日常的によく使います。

```surtr
def parse_bool(text: String) -> Result<Boolean> {
  match text {
    "true" => Ok(True),
    "false" => Ok(False),
    _ => Err(NoneError),
  }
}
```

補助表記として `Result<T, E>` が現れることがありますが、builtin type head の中心は `Result<T>` です。

## `impl Trait` parameter

parameter 位置では `impl Trait` が使えます。

```surtr
def show_abs(x: impl Numeric) -> String {
  inspect(Numeric::abs(x))
}
```

現時点では次は未対応です。

- `-> impl Trait`
- `where` clause

## `TypeRef<$T>`

`TypeRef<$T>` は ordinary value type ではありません。  
target-oriented trait method の witness としてだけ使います。

```surtr
@builtin type TypeRef<$T>

deftrait From<$To> {
  def from(self: Self, to: TypeRef<$To>) -> $To
}

deftrait TryFrom<$To> {
  def try_from(self: Self, to: TypeRef<$To>) -> Result<$To, Error>
}
```

### 使ってよい場所

- trait head で宣言した型引数に対応する trait method parameter
- それに対応する `impl Trait for Type` 側の method parameter

### 使わない場所

- 通常の `def` の引数型
- 通常の `def` の戻り値型
- field type
- local binding の型注釈
- first-class value としての生成・返却・保存

## 空リスト

空リストは要素型が見えないので、型注釈を付けるのが基本です。

```surtr
nums: List<Int> = []
names: List<String> = []
```

## `from(...)` / `try_from(...)`

呼び出し surface では target type を value ではなく型スロットとして読みます。

```text
xldr(1)> print(from(42, String))
42
xldr(2)>
```

この `String` は ordinary value ではなく、変換先型の指定です。

`TypeRef<$T>` の詳しい背景と利用境界は `./special-types.md` を参照してください。

## `_` / `Hole`

ignored-input callable を表すときは `_` が現れます。

```surtr
keep_one: (_ -> Int) = always(1)
```

この `_` は wildcard ではなく、internal な `Hole` marker の surface 表記です。

- callable input を 1 つ受ける
- その入力値は観測しない
- data type としては扱わない

許可されるのは、変数注釈や関数戻り値に現れる callable type など、
かなり限定された場所だけです。

詳しいルールは `./special-types.md` を参照してください。

## 関連ページ

- trait 実装側の話は `./trait-impls.md`
- 利用例は `./definitions-and-usage.md`
- 制約一覧は `./language-reference.md`
- special type 全体は `./special-types.md`

## 確認したソース

- ソース
  - `../../lib/trait/from.srt`
  - `../../lib/trait/try_from.srt`
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- `TypeRef<$T>` は通常の型注釈には使えず、target-oriented trait method parameter 専用です。
- 空リスト `[]` は要素型が見えないため、文脈が弱い場所では型注釈を付ける前提で読むと混乱しにくいです。
