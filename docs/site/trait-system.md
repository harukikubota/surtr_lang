# Trait システム

Surtr の Trait は、compile-time の型制約と method dispatch を表す。
runtime trait object や hidden dictionary は持たない。

## 基本形

```surtr
deftrait Add {
  def add(self: Self, rhs: Self) -> Self
}

impl Add for Int {
  def add(self: Int, rhs: Int) -> Int {
    self + rhs
  }
}
```

`Self` は Trait の実装対象型を表す。`Int` のように target が非 generic なら、`Self` に型引数を付けない。

FunParams は、型変数が value parameter の型から導入できない場合に使う。`Self` が `self` などの引数位置に現れる method は FunParams を必要としない。trait 宣言に FunParams がある場合だけ、impl 側で trait head と impl target による置換形を宣言する。

```surtr
deftrait TryFrom<$To> {
  def try_from::<$To>(self: Self) -> Result<$To, Error>
}

impl TryFrom<Int> for String {
  def try_from::<Int>(self: String) -> Result<Int, Error> { # ... }
}
```

引数位置ですでに導入されている型変数を同じ型で FunParams に重ねて指定するのはエラーである。`Eq::eq(self: Self, rhs: Self)` のような method は `::<Self>` を付けない。一方、`TryFrom` の `$To` は引数位置から導入されず、変換先を指定する FunParams として `::<$To>` に置く。

## 型スロット

通常の `def` に明示的な generic parameter list は書かない。型変数は signature に現れるポリモーフィックなスロットである。

```surtr
def id(value: $A) -> $A
```

```surtr
// 不正
def id<$A>(value: $A) -> $A
```

`defmod` 自体も型引数を持たない。

通常 callable の `id::<Int>(1)` や `&id::<Int>` は不正である。`::<...>` は `try_from::<Int>(value)`、`Decode::decode::<Target>(value)` のような Trait helper の target specialization にだけ使う。

型変数名は宣言ごとのローカル名であり、別の宣言と一致させる必要はない。`$A` と `$T` が同じ出現構造を持てば、同じ型スロットとして扱う。

## Trait の型引数

method の引数から導入できない Trait 固有の型は、Trait head に置く。

```surtr
deftrait TryFrom<$To> {
  def try_from::<$To>(self: Self) -> Result<$To, Error>
}
```

`try_from::<Int>(value)` の `::<Int>` は通常関数の generic 指定ではなく、`TryFrom<Int>` の dispatch target 指定である。

## 型コンストラクタ Trait

`Self` が型コンストラクタであることは `Type` constraint で宣言する。

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

`$A` は constructor slot、`$B` は `mapper` の型から導入されるスロットである。`$B` を `<$B>` として method 宣言に重ねて書かない。

実装対象が複数の型引数を持つ場合は、Trait slot への対応を `TraitName.$Slot` で指定する。

```surtr
impl Functor for Pair<$L, $R>
where
  $R: Functor.$A
{
  def fmap(
    self: Pair<$L, $R>,
    mapper: ($R -> $B)
  ) -> Pair<$L, $B> {
    // ...
  }
}
```

`$L` は capture parameter、`$R` は `Functor.$A` に対応する parameter である。

## impl の一致規則

`impl Trait for Target` の method は、Trait method を次の置換後に構造比較する。

- `Self` を target 型へ置換する
- TraitParams を impl head の引数へ置換する
- constructor slot を target parameter へ対応付ける

比較対象は method 名、引数、戻り値、型変数の同一性、constructor slot、`where` 制約である。型変数の名前そのものは比較しない。

```surtr
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

impl Functor for List<$T> {
  def fmap(self: List<$T>, mapper: ($T -> $U)) -> List<$U> {
    List::map(self, mapper)
  }
}
```

`$A` と `$T`、`$B` と `$U` の名前は異なるが、型構造は一致している。

`impl Compare for ($A, $B)` のように target の構成要素を制約する場合は、`Self` ではなく各 parameter を `where` で制約する。

```surtr
impl Compare for ($A, $B)
where
  $A: Compare
  $B: Compare
{
  // ...
}
```

## default method

body のない method は実装必須、body のある method は default implementation である。impl 側は default method を override できる。

標準 Trait の具体的な API は、各 `lib/traits/*.srt` の `@doc` を参照する。
