# Trait システム更改案

## 目的

Trait の型引数と、通常の関数 signature に現れるポリモーフィックな型スロットを整理する。
通常の関数に明示的な generic parameter list を持たせず、signature の型表現から型スロットを導入する。
Trait の target 型選択、constructor slot、method signature の対応関係は厳密に検証する。

本書は修正案と実装変更の完了条件を兼ねる。下記の surface 規則、標準 SRT、コンパイラ、テスト、表示系を本書に合わせて更新する。

## 実装状態

通常 `def` / `defextractor` の明示型引数は禁止し、signature slot は引数・receiver から導入する。通常 callable の `::<...>` も禁止し、Trait helper の target specialization に限定する。標準 SRT の対象関数と開発者向けの型システム説明はこの規則へ移行済みである。

## 1. 確定する surface 規則

### 1.1 通常の `def` / `defextractor`

通常の関数定義に明示的な型引数リストを置かない。

```surtr
def id(value: $A) -> $A
```

```surtr
// 不正
def id<$A>(value: $A) -> $A
```

型変数は signature の引数型、receiver 型、または外側の TraitParams / constructor slot から導入する。
戻り値にしか現れず、他の宣言からも導入されない型変数は不正とする。

`defmod` は型引数を持たない。

```surtr
// 不正
defmod MyFun<$A> { ... }
```

### 1.2 TraitParams

Trait 固有の型を method の引数位置から導入できない場合は、Trait head の型引数に置く。

```surtr
deftrait TryFrom<$To> {
  def try_from(self: Self) -> Result<$To, Error>
}
```

`::<...>` は通常関数の generic 呼び出しではなく、Trait helper の target specialization を指定する。

```surtr
try_from::<Int>(value)
```

### 1.3 Constructor slot

型コンストラクタとしての polymorphism は Trait の `where` clause で宣言する。

```surtr
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}
```

`$A` は constructor slot、`$B` は `mapper` の引数型から導入される signature slot である。
`$B` を method の明示型引数リストに書かない。

### 1.4 `Self`

- `Self` は Trait の実装対象型全体を表す。
- target が `Int` のような非 generic 型なら `Self` をそのまま使う。
- `Self<$A>` は `Self: Type<$A>` が宣言された Trait の signature でのみ使う。
- `impl Trait for ($A, $B)` の `$A` / `$B` に対する制約は `where` clause に直接書く。
- `impl Target` の receiver を持つ method は `self` を owner target に対応させる。`new` など receiver を持たない関数の第一引数を owner 型に強制しない。
- `impl Trait` は匿名 bounded type の短縮表記として使用しない。引数位置でも `val: impl Trait` を禁止し、名前付き型変数と `where` clause を使用する。

#### `Self` 全体と構成要素の扱い

`Self` 全体を一つの値・型として扱うだけなら、実装対象型の構成要素に対する Trait 制約は要求しない。

```surtr
impl Show for ($A, $B) {
  def to_string(self: Self) -> String {
    inspect(self)
  }
}
```

この例では `$A` と `$B` に対する制約は不要である。`Self` をそのまま `inspect` へ渡しており、各要素に対する型依存操作を行っていないためである。

構成要素を参照するだけの場合も、要素型に対する制約は要求しない。

```surtr
impl Show for ($A, $B) {
  def to_string(self: Self) -> String {
    inspect(self._0)
  }
}
```

一方、構成要素に Trait method、演算子、または Trait helper を適用する場合は、対象の型変数に制約を付ける。

```surtr
impl Show for ($A, $B)
where
  $A: Show
  $B: Show
{
  def to_string(self: Self) -> String {
    to_string(self._0) ++ to_string(self._1)
  }
}
```

`Self` に対して許可する操作は、次のように区別する。

| 操作 | 条件 |
|---|---|
| `self: Self`、`-> Self`、`Result<Self>` | 常に許可 |
| `inspect(self)`、`inspect(self._0)` | 要素の型依存操作がなければ許可 |
| `self._0`、pattern matching による構造参照 | impl target の構造から解決できれば許可 |
| `to_string(self._0)`、`self._0 + self._1` | 各型変数への Trait bound が必要 |
| `Self<$A>`、`Self<$B>` | `Self: Type<...>` の constructor slot が必要 |
| `callable::<Self>(value)` | 不許可 |

したがって、型遷移表現と Trait 制約以外に `Self` 全体を禁止する一般規則は設けない。`Self` を値全体として扱えるか、構造を参照できるか、構成要素へ型依存操作を行うかを別々に判定する。

#### 匿名 `impl Trait` の禁止

`impl Trait` は、型位置における匿名 bounded type の shorthand として使用しない。次の形式をすべて不許可とする。

```surtr
// 不正: 引数位置の匿名 bounded type
def render(value: impl Show) -> String

// 不正: impl target の構成要素に匿名 bounded type
impl Compare for (impl Compare, impl Compare) { ... }

// 不正: generic type の引数位置に匿名 bounded type
impl Functor for List<impl Show> { ... }
```

必要な制約は、名前付き型変数を導入して `where` clause に記述する。

```surtr
def render(value: $T) -> String
where
  $T: Show
{
  to_string(value)
}
```

```surtr
impl Compare for ($A, $B)
where
  $A: Compare
  $B: Compare
{
  def compare(self: Self, rhs: Self) -> Ordering {
    // ...
  }
}
```

この禁止は Trait impl の `impl Trait for Target` に限定されない。通常関数の引数型、Trait method の引数型、`impl` target を構成する型引数のいずれにも適用する。
ただし、`impl Type { ... }` は型 owner namespace を定義する別構文であり、この禁止対象ではない。

制約の適用範囲は、ユーザーが `where` の記述位置によって選択する。

| 記述位置 | 適用範囲 |
|---|---|
| `def ... where` | その関数または method |
| `deftrait ... where` | Trait の実装契約全体 |
| `impl Trait for Target where` | impl block 全体 |
| method の `where` | その method のみ |

`impl Trait for Target` では、Trait 定義側の制約と impl 側の制約が、`Self` と target parameter を置換した結果として整合しなければならない。制約を method 単位へ移す場合も、必要な制約を省略してよいという意味ではない。

### 1.5 型変数名の一致

宣言間で型変数名を一致させない。比較するのは名前ではなく、型変数の出現構造と対応関係である。

```surtr
deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

impl Functor for List<$T> {
  def fmap(self: List<$T>, mapper: ($T -> $U)) -> List<$U> { ... }
}
```

Trait impl では、`Self`、TraitParams、constructor slot、引数と戻り値の同一性、`where` 制約の構造を置換後に厳密一致させる。
通常の `defmod` 関数では、signature 内で型が完結していればよく、外部定義の型変数名は参照しない。

## 2. SRT 改修候補

### 2.1 明示型引数を除去するファイル

現行の通常 `def` / `defextractor` にある `<...>` を除去し、型変数を引数型または `where` から導入する。

| ファイル | 対象 | 方針 |
|---|---|---|
| `lib/types/range.srt` | `new`, `normalized`, `deconstruct` | 引数型 / receiver 型から `$A` を導入。`normalized` の `Compare` は `where` へ移す |
| `lib/types/generator.srt` | `unfold`, `next`, `idx`, `with_index`, `map`, `take`, `to_list`, `iterate` | `Generator<...>`、mapper、seed の型から導入 |
| `lib/types/tuple.srt` | `swap`, `zip_with2` ～ `zip_with8` と result 系 | 引数型から導入 |
| `lib/types/list.srt` | `max`, `min`, `min_max`, `sort` | `<$A: Compare>` を除去し、`where $A: Compare` へ移行 |
| `lib/types/json.srt` | `encode` | `value: $T` から導入 |
| `lib/kernel.srt` | `uncons<$Head, $Tail>` | 現在の untyped 引数では型スロットの導入元がないため、引数型または builtin 専用契約を再設計 |

`lib/traits/*.srt` の `From<$To>`、`TryFrom<$To>`、`Encode<$To>`、`Decode<$To>`、および operator trait の head 型引数は TraitParams のため維持する。
`impl Trait<$A, ...> for ...` の型引数も Trait specialization であり、通常関数の generic list とは区別する。

### 2.2 impl method の確認対象

`lib/types/result.srt`、`lib/types/list.srt` などの trait impl method は、現在すでに method 宣言の `<...>` を使わず `$A` / `$B` を signature に書いている箇所がある。
これらは新規方針に近いが、次を確認する。

- `$A` / `$B` が引数型、receiver 型、TraitParams、constructor slot のいずれかから導入されていること
- Trait 宣言と impl method の型変数出現構造が一致すること
- `Self` と target 型の置換後に `where` 制約が一致すること
- `Result<$T>` の target parameter 名と method 内の `$A` を名前一致させないこと

## 3. コンパイラ改修候補

### 3.1 Spire

主対象は `crates/spire/src/parser/decl.rs` と `crates/spire/src/ast.rs`。

- 通常 `def` / `defextractor` の宣言位置にある `<...>` を parse error にする。
- 引数型および `impl` target の構成型に現れる `AstTy::ImplTrait` を parse error にする。`impl Type { ... }` の owner impl は別構文として維持する。
- `deftrait`、struct、enum、`impl Trait<...>` の型引数は引き続き受理する。
- `Ast::Def` / `Ast::ExtractorDef` に保持している通常関数用 `type_params` の扱いを、空固定または signature slot metadata へ置き換える。
- `$A` がどの signature 部分から導入されたかを検証できる AST / metadata が必要。
- `::<...>` の call/capture parse は維持するが、通常 callable 用途と Trait helper specialization を区別する。

関連テスト:

- `crates/spire/src/parser/tests.rs` の `test_function_def_parses_bounded_type_params`
- `crates/spire/src/parser/tests.rs` の explicit type argument 系テスト
- 通常 `def<$A>` を拒否する parse error テストの追加

### 3.2 Sigil

主対象は `crates/sigil/src/resolver/expr.rs`、`crates/sigil/src/resolver/declarations.rs`、`crates/sigil/src/resolved.rs`。

- 通常 `Def` / `ExtractorDef` の明示 `type_params` 解決を廃止する。
- `impl Trait` の匿名 bounded type を通常の型変数へ変換する fallback を設けず、名前付き型変数と `where` clause を要求する。
- signature から型スロットを収集し、引数型・receiver 型・戻り値との同一性を保持する。
- TraitParams、constructor slot、impl target parameter を通常関数の slot と混同しない。
- `defmod` に型引数を持ち込まない。
- explicit type application の target が通常 callable か Trait helper かを resolver metadata で判別する。

### 3.3 Scar

主対象は `crates/scar/src/checker/expr.rs`、`crates/scar/src/checker/predeclare.rs`、`crates/scar/src/checker/specialize.rs`、`crates/scar/src/checker/mod.rs`。

- `check_explicit_type_apply` の通常 generic callable 分岐を廃止し、Trait helper specialization のみ許可する。
- `impl Trait` が引数型または impl target の構成型に残っていないことを検証する。必要な制約は名前付き型変数と `where` clause で解決する。
- 通常関数の型変数を、signature から導入された rigid slot として typecheck する。
- 型変数が引数または外側の TraitParams / constructor slot から導入されているか検証する。
- 戻り値にしか現れない未導入の型変数を拒否する。
- `def<$A: Trait>` の bound 処理を、signature slot と `where` 制約の処理へ統合する。
- Trait method と impl method は α 変換後の型構造で比較する。名前の一致は要求しない。
- `Self` の bare / constructor application を、Trait の `Self: Type<...>` 宣言に基づいて検証する。
- constructor slot の `TraitSlot` mapping と impl target capture parameter を既存の `TypeConstructor` / `TraitSlot` metadata に接続する。
- specialization key は型変数名ではなく、signature 上の slot ordinal と concrete substitution で構成する。

### 3.4 Typed IR / Forge

主対象は `crates/scar/src/typed.rs` と `crates/forge/src/codegen.rs`。

- 通常関数の `TypedTypeParam` を、宣言 `<...>` の保存ではなく signature slot metadata として再定義または縮小する。
- `format_function_signature` などの表示処理から、通常関数の明示型引数表示を除去する。
- TraitParams と Trait impl specialization の表示は維持する。
- Forge の monomorphization 入力を、通常関数の declaration type parameter ではなく call-site で確定した slot substitution に変更する。
- runtime に型引数を渡す設計は導入しない。既存どおり compile-time specialization とする。

### 3.5 診断・解析・文書

- `crates/diagnostics` の explicit type argument 診断を、通常 callable と Trait helper で分ける。
- `crates/surtr-analysis`、LSP、REPL の signature 表示から通常関数の `<...>` を除去する。
- `crates/xldr` の `:sig` / `:doc` 表示が TraitParams と通常 signature slot を区別することを確認する。
- `docs/site/trait-impls.md` の explicit type arguments の説明を Trait helper specialization に限定する。
- `doc/要件定義v9.md` の Trait System V1 に本書の確定規則を反映する。

## 4. テスト改修方針

### 4.1 削除または書き換え

```surtr
// 旧
def id<$A>(value: $A) -> $A

// 新
def id(value: $A) -> $A
```

対象候補は次のとおり。

- `crates/scar/tests/typecheck_surface.rs` の `def new<$A>` / `def new<$A, $B>` 系
- `crates/spire/src/parser/tests.rs` の function type parameter parse テスト
- `lib/types/generator.srt`、`lib/types/tuple.srt` 由来の integration fixture
- `lib/types/range.srt`、`lib/types/list.srt` の bound 付き generic function

### 4.2 維持するテスト

- `deftrait From<$To>` / `TryFrom<$To>` の TraitParams
- `impl Trait<$A, ...> for Target` の Trait specialization
- `Self: Type<$A>` と `TraitName.$A` の constructor slot mapping
- `try_from::<Int>(value)` など Trait helper の explicit target 指定
- 通常 callable の `id::<Int>(value)` が拒否されること

### 4.3 追加する最小ケース

- 型変数名が異なる通常関数 signature の α 同値
- 型変数名が異なる trait method / impl method の有効例
- method 内の型変数出現関係が変わる impl の拒否
- `Self` と `Self<$A>` の誤用の拒否
- 引数に現れない未導入戻り値型変数の拒否
- `defmod MyFun<$A>` の拒否
- `def id<$A>(...)` の拒否
- `try_from::<Int>(...)` の維持

## 5. 実装順序

1. 仕様と標準 SRT の surface を確定する。
2. Spire で通常 `def` / `defextractor` の explicit type parameter を拒否する。
3. Sigil / Scar で signature slot の導入・rigid 化・α 同値比較を実装する。
4. explicit type application を Trait helper 専用へ限定する。
5. specialization / Forge / 表示系を新 metadata に合わせる。
6. 標準 SRT と fixture を移行し、workspace テストを実行する。

`lib/kernel.srt` の `uncons` は、現行 signature が型変数の導入元を持たないため、単純な置換より先に契約を決定する。
