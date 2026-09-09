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

signature slot は `$` 付きで書きます。通常の `def` に型引数リストは書きません。

```surtr
def id(value: $A) -> $A { value }
```

trait bound を付けるときは次の形です。

```surtr
def twice(x: $N) -> $N
where
  $N: Add
{
  Add::add(x, x)
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

## Trait 制約

匿名 `impl Trait` 型は使いません。名前付き signature slot と `where` clause で制約を記述します。

```surtr
def show_value(x: $T) -> String
where
  $T: Show
{
  to_string(x)
}
```

型引数を持つ trait でも制約は `$T: Encode` のように bare capability で書きます。`Encode<String>` は where RHS ではなく impl head または式の dispatch target です。generic receiver で Trait helper を使うには、その helper が要求する bound を明示します。詳しくは [`trait-system.md`](./trait-system.md) を参照してください。

## 明示型引数

値引数だけでは得られず戻り値に現れる型入力は、関数定義のReturnTypeArgumentとして宣言します。

```surtr
def make::<$A>() -> $A
def try_from::<$To>(value: $From) -> Result<$To, Error>
where
  $From: TryFrom
```

呼び出し側では、定義側の各ReturnTypeArgumentを同じ順序で`::<...>`へ指定できます。

```surtr
text = from::<String>(42)
number =? try_from::<Int>("42")
```

明示項目の数は定義側と一致させます。推論へ残す位置は`_`で書き、list全体を省略した呼び出しは
全項目を`_`にした場合と同じです。末尾だけを省略するpartial listは使えません。

```surtr
convert::<_, Int>(value)
convert::<_, _>(value)
convert(value) # 定義側が2項目なら直前の全`_`と同じ制約
```

ReturnTypeArgumentはruntimeの値でも、通常genericを任意指定する型parameter listでもありません。
`id(value: $A) -> $A`の`$A`は値引数から得られるため、`id::<Int>(1)`や`&id::<Int>`は書けません。
一方、定義側にReturnTypeArgumentがある通常関数・Trait helper・methodは、通常callとcaptureの両方で
対応する`::<...>`を使えます。expected returnから一意に決まる場合はcall-site指定を省略できますが、
最後まで決まらない場合はambiguityとして拒否されます。

## 空リスト

空リストは要素型が見えないので、型注釈を付けるのが基本です。

```surtr
nums: List<Int> = []
names: List<String> = []
```

既知の expected type は複合式の内側へ伝播します。list の各要素、tuple の各 slot、`if` の全 branch、`match` の全 arm が対象です。

```surtr
values: List<Option<Int>> = [pure(1), Option::Some(2)]
value: Option<Int> = if(flag, pure(1), Option::Some(2))
```

反対に expected type がまだ未束縛でも、scalar literal、closure、tuple、non-empty collection などは式自身から型を得て generic slot と照合します。これは通常 call、constructor、Trait helper、apply、compose で共通です。

numeric literal の種類はこの推論で変更しません。`Int` literal を `Float` として使う暗黙 coercion はありません。

型注釈で単相に固定していない local callable は、binding environment から独立した型スロットを call-site ごとに fresh にします。capture した外部値の型や外側 signature の rigid generic は一般化しません。

## `from(...)` / `try_from(...)`

呼び出し surface では target type を value ではなく型スロットとして読みます。

```text
xldr(1)> print(from::<String>(42))
42
xldr(2)>
```

この `String` は ordinary value ではなく、変換先型の指定です。

compiler-special type の利用境界は `./special-types.md` を参照してください。

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

- target-oriented trait の型入力は `::<...>` で指定し、値引数として型名を渡しません。
- 空リスト `[]` は要素型が見えないため、文脈が弱い場所では型注釈を付ける前提で読むと混乱しにくいです。
