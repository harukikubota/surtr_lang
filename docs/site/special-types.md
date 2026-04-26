# Special Types

Surtr には、通常の user-defined type とは少し性格の違う
compiler-special type があります。  
現在まとまっているのは次の 3 つです。

- `Unit`
- `TypeRef<$T>`
- `Hole`

canonical declaration の正本は
[`../../lib/special_types.srt`](/Users/haruca/work/rust/surtr/lib/special_types.srt)
です。

## 何が special なのか

これらは単に「標準ライブラリにある型」ではなく、compiler が特別扱いする
contract を持っています。

- canonical builtin type 名として予約される
- user-defined type と同じ自由度では使えない
- source surface と internal meaning が 1 対 1 ではない場合がある
- syntax, typecheck, REPL 表示の複数箇所で共通ルールを持つ

たとえば `TypeRef<$T>` は value として持ち運ぶ型ではありませんし、
`Hole` は `_` という surface marker の背後にある internal type です。

## `Unit`

`Unit` は 3 つの special type の中では、もっとも ordinary な型です。

- 値は `()`
- 「意味のある値は返さないが、式としては成立する」場面を表す
- effectful な builtin や statement-like な surface の返り値に多く現れる

例:

```surtr
def log_twice(text: String) -> Unit {
  print(text)
  print(text)
}
```

`Unit` は special type ではありますが、user-facing surface では通常の型として
もっとも自然に使えます。

## `TypeRef<$T>`

`TypeRef<$T>` は target-oriented trait method のための
"target type witness" です。

代表例は `From` / `TryFrom` です。

```surtr
deftrait From<$To> {
  def from(self: Self, to: TypeRef<$To>) -> $To
}

deftrait TryFrom<$To> {
  def try_from(self: Self, to: TypeRef<$To>) -> Result<$To, Error>
}
```

### どう読むか

`TypeRef<$T>` を ordinary value として読むと混乱しやすいです。  
実際には「この method はどの target type へ向かうのか」を signature 上で
明示する witness slot と考えるのが自然です。

```surtr
text = from(42, String)
value = try_from("42", Int)
```

この `String` / `Int` は runtime value ではなく、surface 上の target type 指定です。  
内部的には `TypeRef<String>` / `TypeRef<Int>` witness として扱われます。

### 許可される場所

- trait head で宣言した型引数に対応する trait method parameter
- 対応する `impl Trait for Type` method parameter
- `from(value, TargetTy)` / `try_from(value, TargetTy)` の target slot を表す内部解釈

### 許可されない場所

- 通常の `def` の引数型
- 通常の `def` の戻り値型
- local binding の型注釈
- field type
- tuple / function type の要素
- first-class value としての生成、返却、保存

### なぜ制限が強いのか

`TypeRef<$T>` を普通の値型として広げると、
「target type を指定する witness」と「実データ」が混ざってしまいます。  
Surtr ではそこを混ぜず、trait dispatch のための compile-time slot に限定しています。

この制約のおかげで、

- `from(value, TargetTy)` の surface が短い
- runtime に trait object 的な仕組みを持ち込まない
- conversion API の意図が signature だけで読み取りやすい

という利点が得られます。

## `Hole`

`Hole` は ignored-input callable を表す compiler-reserved marker です。  
user-facing surface では `Hole` という名前を直接使うのではなく、`_` と表示します。

代表例:

```surtr
always: (_ -> Int) = const(1)

def make() -> (_ -> Int) {
  const(2)
}
```

これは

- 「何か 1 つ受け取る」
- でもその入力値は観測しない
- 結果だけ返す

という callable surface を表しています。

### どういう問題を解決するのか

`const(1)` の本質は、任意の入力を受けても `1` を返す callable です。  
これを内部型変数のまま見せると、REPL や docs に `$B` のような実装都合の型変数が
漏れやすくなります。

`Hole` はその未観測入力を「ignored-input marker」として閉じるための型です。

```surtr
always = const(1)
```

REPL では次のように見えます。

```text
always: (_ -> Int)
```

### `Hole` の surface 表記

- internal type 名は `Hole`
- user-facing 表記は `_`

つまり `(_ -> Int)` は、内部では `Hole` を使って表現される callable type です。

### 許可される場所

`Hole` / `_` は unrestricted wildcard type ではありません。  
許可はかなり限定されています。

- 変数の callable type annotation
  - `always: (_ -> Int) = const(1)`
- 関数戻り値の中に現れる callable type
  - `def make() -> (_ -> Int) { const(1) }`
- ignored parameter を持つ closure literal の surface / 表示
  - `{|_| 10}`

### 許可されない場所

- plain value type
  - `x: _ = ...`
- data container の要素
  - `List<_>`
- return hole そのもの
  - `(Int -> _)`
- field type
- tuple の要素型
- 通常の関数引数型
  - `def apply_once(f: (_ -> Int)) -> Int`

### 多引数 wildcard ではない

`Hole` は「ignored input callable を自然に持ち運ぶ」ための marker です。  
一般の wildcard type system にしたいわけではありません。

そのため、`(_ -> Int)` はサポートしても、
`(_, _ -> Int)` のような surface は自然な const-like contract とみなさず、
通常の型不一致として弾かれます。

### `Hole` を user-defined type にできない理由

`Hole` は canonical builtin type 名として予約されています。  
そのため次のような定義はできません。

```surtr
defstruct Hole { value: Int }
defenum Hole { Filled }
deferror Hole { "reserved" }
```

これは「`Hole` がたまたま標準ライブラリにある名前」ではなく、
compiler-special type contract の一部だからです。

## `TypeRef` と `Hole` の違い

この 2 つはどちらも first-class value ではありませんが、役割はかなり違います。

- `TypeRef<$T>`
  - trait method signature で target type を指定する witness
  - dispatch / conversion surface のための marker
- `Hole`
  - ignored-input callable を表す marker
  - polymorphic に見える未観測入力を stable な surface へ閉じるための marker

言い換えると、

- `TypeRef` は "どこへ変換するか"
- `Hole` は "何かは受けるが見ない"

を表しています。

## 利用時の目安

普段の user code では、special type 名そのものを意識する場面は多くありません。

- `Unit` は普通に使ってよい
- `TypeRef<$T>` は `from(value, TargetTy)` / `try_from(value, TargetTy)` の背後にあるものと考える
- `Hole` は `const(1)` や `{|_| ...}` が `(_ -> T)` と見える理由だと考える

直接書く必要があるときは、次の感覚で十分です。

- `TypeRef<$T>` を直接書くのは trait signature を設計するとき
- `_` を書くのは ignored-input callable annotation を表したいとき

## 関連ページ

- target-oriented conversion surface は `./type-annotations.md`
- `Kernel::const` と ignored-input callable は `./kernel.md`
- 標準モジュール全体の配置は `./standard-library.md`
- language-wide ルール一覧は `./language-reference.md`
