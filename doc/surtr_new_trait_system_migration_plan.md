# 新トレイトシステム導入までの改修案

## 1. 目的

新トレイトシステム導入前に、既存のジェネリック型変数・型推論・関数定義検査を整理し、以下を保証する。

- 定義側のジェネリック型変数を本体の具象型から勝手に確定しない
- 呼び出し側では型推論によってジェネリック型を具象化できる
- 型変数を含むコンテナ型を正しく扱える
- `null` 相当となる「任意型の値生成」を型検査上許さない
- 後続の `where` / 親トレイト / `Type<$A>` / 明示型引数を追加できる基盤を作る
- 現行の specialization と call-site 推論を壊さない

---

## 2. 現状

現在、ジェネリック型変数 `$A` は内部的に次で表現される。

```rust
Ty::Var(u32)
```

同じ `Ty::Var` が以下すべてに使用されている。

- 関数シグネチャで宣言されたジェネリック型
- 型推論中の unknown
- call-site で fresh 化された型変数

そのため `types_compatible` 上では、

```text
宣言 generic
推論 unknown
call-site generic
```

の意味的な区別が存在しない。

`Ty::Var` は通常の unification variable として、

```text
$A := String
```

のように具体型へ bind 可能である。

### 2.1 現在の安全策

次のような定義は、

```surtr
defmod MyFun {
  def nil() -> $A {
    ""
  }
}
```

unification 前の専用ガードにより拒否されている。

一方、

```surtr
defmod MyFun {
  def identity(value: $A) -> $A {
    value
  }
}
```

は受理される。

また、

```surtr
List<$A>
```

は、

```rust
Ty::List(Box<Ty::Var(...)>)
```

として外側の型構造と内側の型変数を区別できている。

---

## 3. 型変数の意味分類

`Ty::Var` の内部表現そのものを直ちに分割する必要はないが、型検査上は少なくとも次の3種類を区別する。

```text
Signature Generic
Inference Variable
Call-site Instance
```

### 3.1 Signature Generic

関数・trait・impl の定義に属する型変数。

```surtr
def identity(value: $A) -> $A {
  value
}
```

この `$A` は定義本体を検査している間は **rigid** とする。

```text
$A == $A       OK
$A := String   NG
$A := Int      NG
```

### 3.2 Inference Variable

式の型推論によって生成された unknown。

```text
?0
?1
...
```

これは通常どおり bind 可能。

```text
?0 := String
```

### 3.3 Call-site Instance

generic function を利用する際、宣言 generic から生成される fresh inference variable。

概念的には、

```text
definition:
    identity : $A -> $A

call:
    identity(1)

instantiate:
    $A -> ?0

unify:
    ?0 := Int
```

となる。

**宣言 `$A` 自体を書き換えない。**

---

## 4. Phase 1: 定義側 generic の rigid 化

新トレイトシステムより先に、この境界を明確にする。

### 4.1 基本規則

> 関数シグネチャによって導入された generic parameter は、その関数本体の型検査中は rigid とする。

例えば、

```surtr
def wrong() -> $A {
  ""
}
```

では、

```text
expected = $A (rigid)
actual   = String
```

なのでエラー。

`$A := String` は行わない。

### 4.2 正常例

```surtr
def identity(value: $A) -> $A {
  value
}
```

は、

```text
expected = $A
actual   = $A
```

なので受理。

### 4.3 異なる generic 同士

```surtr
def wrong(value: $A) -> $B {
  value
}
```

`$A` と `$B` が独立した signature generic なら、定義側で `$B := $A` と bind しない。

原則としてエラーとする。

### 4.4 型コンストラクタ内の generic

```surtr
def gen_nil() -> List<$A> {
  []
}
```

のように、`$A` 自体の値を生成せず、型コンストラクタのスロットとして保持するケースは許容可能である。

重要なのは、

```text
List<$A>
```

と

```text
$A
```

を同一視しないこと。

---

## 5. Phase 2: call-site generic の fresh 化を明確化

関数定義側の generic と、呼び出し側で推論される型を分離する。

### 5.1 期待するモデル

```surtr
def identity(value: $A) -> $A {
  value
}

left: Int = identity(1)
right: String = identity("")
```

各 call-site で、

```text
identity(1)
  $A -> ?0
  ?0 := Int

identity("")
  $A -> ?1
  ?1 := String
```

となる。

`?0` と `?1` は独立。

### 5.2 禁止事項

- 宣言時の `$A` に call-site の型を直接 bind しない
- ある call-site の substitution を別 call-site に漏らさない
- specialization のための mapping と定義側 generic を混同しない

---

## 6. Phase 3: bare generic return 専用ガードの整理

現在の `concrete_body_satisfies_bare_generic_return` は安全策として機能しているが、rigid generic が導入できれば責務を縮小できる。

### 6.1 現在

```surtr
def nil() -> $A {
  ""
}
```

を専用ガードで拒否。

### 6.2 将来

通常の return type compatibility で、

```text
rigid $A
vs
String
```

を不一致として拒否できるようにする。

### 6.3 方針

- Phase 1 完了までは既存ガードを維持
- rigid 検査導入後に重複判定を調査
- 同等保証が取れれば専用ガードを簡略化または除去
- 診断メッセージが有用なら、意味解析用の補助診断として残す

---

## 7. Phase 4: `where` 節の導入

型変数の宣言と制約を分離する。

### 7.1 基本構文

```surtr
def List::sum(list: List<$A>) -> $A
where
  $A: SumMonoid
{
  ...
}
```

`$A` はシグネチャへの出現によって導入される。

`where` は **型変数を宣言せず、制約だけを追加する**。

### 7.2 複数制約

```surtr
where
  $A: Eq + Concat
```

`+` は AND 制約。

```text
$A satisfies Eq
AND
$A satisfies Concat
```

### 7.3 同一対象への制約

推奨形:

```surtr
where
  Self: Functor
  $A: Eq + Concat
```

同一左辺を複数行に分けるより、1行 = 1制約対象へ寄せる。

`Functor` が `Self: Type<$A>` を要求する場合、`Self: Functor` は
`Self: Type<$A>` を推移的に含む。そのため、子 trait 側で同じ
`Type<$A>` 制約を繰り返さない。

---

## 8. Phase 5: 型コンストラクタ制約 `Type<$A>`

現行の std 専用型構文、

```surtr
Type List<$A>
```

と対応する形で、where 節では `Type` を型形状制約として扱う。

### 8.1 1引数型コンストラクタ

```surtr
where
  Self: Type<$A>
```

意味:

> `Self` は型引数を1つ取る型コンストラクタである。

### 8.2 多引数

将来的には、

```surtr
where
  Self: Type<$A, $B>
```

も可能。

ただし `Type<$A>` の arity は、実装型の宣言上の総 type parameter 数ではなく、
**trait に公開する型コンストラクタの arity** として扱う。

`Functor` は `Self: Type<$A>` により公開 arity がちょうど 1 であることを
要求する。`List<$T>` / `Option<$T>` / Surtr の `Result<$T>` のような unary
型はそのまま満たす。

複数 parameter を持つ型も、1 個だけを Functor の slot に対応付け、残りを
capture parameter として保存するなら実装できる。型定義の総 arity を 1 に
限定してはならない。

### 8.3 `Type` の位置付け

`Type<$A>` は通常 trait ではない。

where 右辺を、

```text
Constraint :=
    TraitConstraint
  | TypeConstructorConstraint
  | TraitSlotConstraint
```

のように一般化する。

### 8.4 trait slot 対応付け

`Type<$A>` の `$A` は trait が公開する constructor slot である。impl では
実装型の type parameter をこの slot へ対応付けられる。

```surtr
impl Functor for Result<$T>
where
  $T: Functor.$A
{
  def fmap(
    self: Result<$A>,
    mapper: ($A -> $B)
  ) -> Result<$B> {
    match self {
      Ok(value) => Ok(mapper(value)),
      Err(err) => Err(err),
    }
  }
}
```

`Functor.$A` は trait の constructor slot 参照であり、通常の trait bound ではない。
この例の `$T: Functor.$A` は `$T: Functor` を意味しない。`where` の
`Ty: Ty` 形式を保ちつつ、右辺が `TraitName.$TypeParam` の場合だけ
`TraitSlotConstraint` として解釈する。`.` は型制約文脈でのみ slot projection
として使い、値式の Facet access とは区別する。内部的な slot identity は名前ではなく
declaration order の ordinal で保持する。

unary 型では対応付けを省略できる。

```surtr
impl Functor for List<$T> {
  ...
}
```

これは `$T: Functor.$A` を補完したものとして扱う。総 arity が 2 以上の型では
対応付けを省略できない。

```surtr
impl Functor for Pair<$L, $R>
where
  $R: Functor.$A
{
  def fmap(
    self: Pair<$L, $A>,
    mapper: ($A -> $B)
  ) -> Pair<$L, $B> {
    ...
  }
}
```

この場合 `$L` は capture parameter であり input / output で不変に保つ。
Functor は slot が 1 個なので、impl ごとに `Functor.$A` への対応付けはちょうど
1 個必要である。同じ slot を複数 parameter に対応付けること、または同じ parameter を
複数 slot に対応付けることは compile error とする。`Self<$A>` / `Self<$B>` と impl
method signature の照合により、対応付けた slot だけが `$A -> $B` に置換され、capture
parameter が保存されることを検証する。

---

## 9. Phase 6: 親トレイトシステム

trait の階層関係は専用の継承構文ではなく `where` で表現する。

### 9.1 Functor

```surtr
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(
    self: Self<$A>,
    mapper: ($A -> $B)
  ) -> Self<$B>
}
```

### 9.2 Applicative

```surtr
deftrait Applicative
where
  Self: Functor
{
  def pure(value: $A) -> Self<$A>

  def apply(
    mapper: Self<($A -> $B)>,
    value: Self<$A>
  ) -> Self<$B>
}
```

### 9.3 Monad

```surtr
deftrait Monad
where
  Self: Applicative
{
  def bind(
    self: Self<$A>,
    mapper: ($A -> Self<$B>)
  ) -> Self<$B>
}
```

### 9.4 階層の意味

```surtr
Self: Applicative
```

は trait inheritance ではなく、

> Monad を実装する Self は Applicative も実装していなければならない

という constraint。

### 9.5 constraint composition

trait constraint は直接指定だけでなく推移 closure を取る。

```text
Monad
  requires Self: Applicative
  -> Applicative requires Self: Functor
  -> Functor requires Self: Type<$A>
```

したがって `impl Monad for List<$T>` の検査では、同じ型コンストラクタに対する
`Applicative` と `Functor` の impl が存在し、公開 arity が 1 であることを保証する。
親 trait の method を子 trait namespace に mixin はしない。たとえば `fmap` は常に
`Functor::fmap` として解決する。

closure の各 constraint は slot identity を ordinal で正規化して重複除去する。同じ
subject に対する parent trait constraint は、child の `Self` および constructor slot
対応付けへ substitute する。親 trait constraint の循環は trait 宣言時に compile error
とする。

---

## 10. Phase 7: trait implementation の検査

例:

```surtr
impl Functor for List<$T> {
  def fmap(
    self: List<$A>,
    mapper: ($A -> $B)
  ) -> List<$B>
  {
    ...
  }
}
```

impl 検査では以下を保証する。

1. `List` が `Type<$A>` を満たし、unary target では唯一の type parameter を
   `Functor.$A` へ自動対応付けできる
2. 型 parameter が複数の target では、`TraitName.$TypeParam` への明示対応付けが
   あり、trait が要求する公開 arity と厳密に一致する
3. trait の `Self<$A>` / `Self<$B>` を、対応付けた target slot のみ置換し
   capture parameter を保存して具体化する
4. trait method signature と impl signature を照合する
5. `$A`, `$B` および capture parameter は impl 本体検査中は rigid
6. 本体の具象値からこれらの型変数を勝手に bind しない
7. impl 内の値生成は通常の式型検査で保証する

---

## 11. Phase 8: TryFrom と明示型入力への準備

将来の構文:

```surtr
deftrait TryFrom<$To> {
  def try_from(self: Self) -> Result<$To, Error>
}
```

`TypeRef<$To>` のようなコンパイラ用マーカー値は最終的には不要にする。

### 11.1 明示型引数

将来的に、

```surtr
TryFrom::try_from::<Int>("1")
```

を導入可能にする。

ただし `::<...>` は **型を決定するための入力**であって値ではない。

### 11.2 `Self`

`Self` は `::<...>` に含めない。

```text
Self = trait dispatch target
trait/function generic = explicit type arguments
```

を分離する。

禁止方向:

```surtr
Concat::concat::<String>
TryFrom::try_from::<Int, String>
```

`String` を `Self` として渡す構文にはしない。

---

## 12. Phase 9: キャプチャ演算子との統合

キャプチャは単独で完全具象化するより、高階関数内の型推論を前提とする。

```surtr
fn: (String -> Result<Int, Error>) =
  &TryFrom::try_from::<Int>
```

期待型から `Self = String` を導出。

また、

```surtr
value |> map(&foo)
```

```surtr
value |>= &foo
```

のような文脈から型を推論する。

### 12.1 不要な拡張

`Self` をキャプチャ時に明示するための特殊構文は導入しない。

---

## 13. trait namespace

trait は実装型の module namespace へ mixin しない。

```surtr
impl Concat for String
```

によって、

```surtr
String::concat
```

は生成しない。

`String::concat` は String module 自身に定義された関数を意味する。

trait 呼び出しは、

```surtr
Concat::concat(...)
```

として trait namespace を経由する。

静的ディスパッチ:

```text
Concat::concat(...)
    ↓
Self を具象型へ推論
    ↓
impl Concat for Self を解決
    ↓
具象実装を呼び出す
```

---

## 14. Default の扱い

検証用に次の trait を定義可能。

```surtr
deftrait Default {
  def default() -> Self
}
```

実装:

```surtr
impl Default for String {
  def default() -> String {
    ""
  }
}
```

generic wrapper:

```surtr
def make() -> $A
where
  $A: Default
{
  Default::default()
}
```

ここで `make` が `$A` を無から生成するわけではない。

```text
make
  ↓
Default::default
  ↓
impl Default for concrete type
  ↓
具象値の構築
```

値生成能力を別システムとして追跡する必要はない。

型安全な式・trait dispatch・具象 impl の連鎖によって保証する。

---

## 15. 値生成に関する基本原則

`::<$A>` や型推論は、

> どの型か

を決定するだけであり、

> その型の値を生成する

能力を与えない。

例えば、

```surtr
def gen_nil() -> List<$A> {
  []
}
```

は `$A` の実値を必要としないため成立可能。

一方、

```surtr
def nil() -> $A {
  ""
}
```

では、rigid `$A` と `String` が一致しないため拒否する。

特別な `null` / zero-init / undefined / uninitialized を追加しない限り、通常の式型検査で値生成の安全性を維持できる。

---

## 16. 実装順序

### Step 1: generic の rigid / inference 境界を追加

最優先。

- 定義側 signature generic を識別
- return / expression compatibility で rigid generic への bind を禁止
- 既存 generic call-site の動作を維持

### Step 2: call-site fresh 化の回帰確認

- 同一 generic 関数を複数型で呼べる
- substitution が call-site 間で漏れない
- specialization と型推論が一致する

### Step 3: bare generic return ガード整理

- rigid 検査との重複を確認
- 必要なら診断専用へ縮小

### Step 4: `where` AST / parser / typed representation

- `Target: Constraint + Constraint`
- 型変数宣言機能は持たせない
- `TraitName.$TypeParam` を通常 bound と区別する `TraitSlotConstraint` として表現
- slot identity は source 上の名称ではなく trait declaration 内の順序で保持

### Step 5: `Type<$A>` constraint

- trait に公開する型コンストラクタ arity の厳密検査
- unary target では唯一の type parameter から trait slot 対応付けを自動補完
- 2 parameter 以上の target では `TargetParam: TraitName.$TypeParam` の
  明示対応付けを要求
- where constraint として実装

### Step 6: trait parent constraint

- `Self: Functor`
- 推移的 constraint 解決
- parent trait が導く `Type<$A>` を closure に含め、冗長な再指定を不要にする
- 同一 constructor と slot 対応付けに対して親 impl が存在することを検証
- parent constraint cycle を拒否

### Step 7: impl validation

- trait signature substitution
- trait slot と target type parameter の one-to-one 対応付け
- mapped slot だけを input `$A` から output `$B` へ置換し、capture parameter を保存
- rigid generic body checking
- parent trait constraint validation

### Step 8: Functor / Applicative / Monad を実装

ここで型コンストラクタ trait の実用検証を行う。

### Step 9: 明示型引数 `::<...>`

trait 基盤が安定してから追加。

- generic slot の明示指定
- `Self` は対象外
- call-site inference と併用

### Step 10: TypeRef の縮小・削除

`::<...>` で置換可能な用途から順次除去。

---

## 17. 回帰テスト

### NG: 具象値による signature generic の特殊化

```surtr
def nil() -> $A {
  ""
}
```

期待: error

### OK: generic のそのまま返却

```surtr
def identity(value: $A) -> $A {
  value
}
```

期待: OK

### NG: 独立 generic の統一

```surtr
def bad(value: $A) -> $B {
  value
}
```

期待: error

### OK: generic container

```surtr
def gen_nil() -> List<$A> {
  []
}
```

期待: `List<Ty::Var(_)>` を維持

### OK: call-site 分離

```surtr
def identity(value: $A) -> $A {
  value
}

left: Int = identity(1)
right: String = identity("")
```

期待: 両方 OK

### trait shape mismatch

```surtr
impl Functor for Int {
  ...
}
```

期待:

```text
Int does not satisfy Type<$A>
```

### OK: unary target の trait slot 自動補完

```surtr
impl Functor for List<$T> {
  def fmap(self: List<$A>, mapper: ($A -> $B)) -> List<$B> {
    ...
  }
}
```

期待: `$T: Functor.$A` を補完し、`Self<$A> = List<$A>` として検査する。

### OK: 複数 parameter target の明示 slot 対応付け

```surtr
impl Functor for Pair<$L, $R>
where
  $R: Functor.$A
{
  def fmap(self: Pair<$L, $A>, mapper: ($A -> $B)) -> Pair<$L, $B> {
    ...
  }
}
```

期待: `$L` を capture parameter として保存し、右側 slot だけを map する。

### NG: 複数 parameter target の slot 対応付け省略

```surtr
impl Functor for Pair<$L, $R> {
  ...
}
```

期待: Functor の `$A` に対応する target parameter が不明として error。

### NG: trait slot の重複対応付け

```surtr
where
  $L: Functor.$A
  $R: Functor.$A
```

期待: 同じ trait slot への複数対応付けとして error。

### parent trait missing

```surtr
impl Monad for Foo {
  ...
}
```

`Foo` が Applicative を持たない場合は error。

### impl generic rigid

Functor impl 本体で `$A` を String 等へ勝手に固定する実装を拒否する。

---

## 18. 最終仕様の骨格

```surtr
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(
    self: Self<$A>,
    mapper: ($A -> $B)
  ) -> Self<$B>
}

deftrait Applicative
where
  Self: Functor
{
  def pure(value: $A) -> Self<$A>

  def apply(
    mapper: Self<($A -> $B)>,
    value: Self<$A>
  ) -> Self<$B>
}

deftrait Monad
where
  Self: Applicative
{
  def bind(
    self: Self<$A>,
    mapper: ($A -> Self<$B>)
  ) -> Self<$B>
}

deftrait TryFrom<$To> {
  def try_from(self: Self) -> Result<$To, Error>
}

deftrait Default {
  def default() -> Self
}
```

impl:

```surtr
impl Functor for List<$T> {
  def fmap(
    self: List<$A>,
    mapper: ($A -> $B)
  ) -> List<$B>
  {
    ...
  }
}

impl Functor for Pair<$L, $R>
where
  $R: Functor.$A
{
  def fmap(
    self: Pair<$L, $A>,
    mapper: ($A -> $B)
  ) -> Pair<$L, $B> {
    ...
  }
}

impl TryFrom<Int> for String {
  def try_from(self: String) -> Result<Int, Error> {
    ...
  }
}

impl Default for String {
  def default() -> String {
    ""
  }
}
```

---

## 19. 最重要方針

新トレイトシステム導入前に保証すべき境界は次の1点に集約できる。

> **定義側 generic は rigid、call-site では fresh inference variable へ instantiate して bind する。**

現行の bare generic return 専用ガードは、この一般則がまだ型システムに存在しないことを局所的に補っている。

親トレイト、`where`、`Type<$A>`、Functor、明示型引数を追加する前にこの境界を明確にしておくことで、後続機能を同じ型変数モデル上に構築できる。
