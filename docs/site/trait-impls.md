# Trait Impls

Surtr の trait system は V1 です。  
公開 surface では `deftrait` と `impl Trait for Type` を使います。

## 分類

標準 trait は大きく 3 層に分けて読むと分かりやすいです。

- capability trait
  - `Show`, `Compare`, `Default`, `From`, `TryFrom`
- operator dispatch trait
  - `Add`, `Sub`, `Mul`, `Eq`, `Neq`, `Concat`
  - `Functor`, `Applicative`, `Monad`, `PipeApply`, `Compose`, `Composable`, `LiftComposable`, `KleisliComposable`

`Compare` は新しい API が三値比較を要求するときの正本です。`< <= > >=` も公開 surface では `Compare` によって意味づけられます。  
数値 helper は generic trait ではなく、`Int::abs` / `Float::safe_div` のような concrete type owner surface として提供します。

`Default::default::<T>() -> T` は runtime value parameter を取らず、expected return type または明示型引数から target type を決めます。`@derive Default` は field / payload の default 値を使う実装を生成しますが、constructor の検証処理や型固有の不変条件を代替しません。

## 宣言側

trait と impl は file-oriented に書きます。

```surtr
deftrait Describable {
  def describe(self: Self) -> String
}

impl Describable for Int {
  def describe(self: Int) -> String { to_string(self) }
}
```

FunParams は、型変数が value parameter の型から導入できない場合に使い、その型変数は戻り値にも現れなければならない。`self: Self` のように型変数が引数位置に現れる method は FunParams を省略する。trait 宣言に FunParams がある場合、impl method は trait head と impl target で置換した同じ構造を宣言し、個数・順序・型構造を一致させる。引数位置で導入済みの型変数を同じ型で重ねて指定するのはエラーである。

### 型引数を持つ Trait と `where`

Trait 引数は impl head で明示しますが、`where` の capability は bare trait 名で書きます。

```surtr
deftrait Encode<$Format> {
  def encode(self: Self) -> $Format
}

impl Encode<String> for List<$A> { # ... }
```

実装本体が `$A` の値に `Encode::encode::<String>(value)` のような式を使う場合だけ、impl の `where` に `$A: Encode` を宣言する。この bare capability は注釈ではなく、その式が消費する proof である。消費しない impl に bound を置くと `UnusedTraitConstraint` になる。`Encode<String>` のような完全な identity は impl head と expression dispatch が保持し、`$A: Encode<String>` は where RHS として不正である。candidate が target に一致しても、式が発行する完全 obligation を満たさなければ dispatch されない。

親 Trait は bare capability として継承します。child impl の `where` が親 capability を包含していれば利用できます。

### default method と同名 method

body を持つ Trait method は default method です。impl は 1 回だけ override できます。body のない method は各 impl で実装が必要です。

同一の `defmod`、`impl Type`、`impl Trait for Type` block に同名の `def` / `defp` を複数書くことはできません。引数や visibility を変えて overload を作ることもできません。一方、別 Trait の同名 method は別の契約なので、Trait 名を付けて呼び分けます。

```surtr
T1::f(value)
T2::f(value)
```

## 呼び出し側

利用者がまず触るのは helper surface です。

```text
xldr(1)> print(to_string(Int::abs(-4)))
4
xldr(2)>
```

### `Functor`, `Applicative`, `Monad`

この3つは文脈付き計算を段階的に扱う標準 trait です。

- `Functor::fmap` / `|*>` は文脈の中身だけを変換します。
- `Applicative::pure` は値を文脈へ持ち上げ、`Applicative::ap` / `|*|` は文脈内の callable と文脈内の値を組み合わせます。
- `Monad::return` は値を文脈へ持ち上げ、`Monad::bind` / `|>=` は文脈内の値を次の文脈付き計算へ渡します。

これら3 trait は auto import 対象です。そのため、qualified call の代わりに
`fmap(...)`, `pure(...)`, `ap(...)`, `return(...)`, `bind(...)` と書けます。
演算子は文脈の流れを読みやすくし、helper の直接呼び出しは flat な式や高階関数で
便利です。

複数引数の mapper は `curry()` で明示的にカリー化します。

```surtr
Ok(curry(&Add::add)) |*| Ok(1) |*| Ok(2) # => Ok(3)

value: Result<Int> = pure(1)
mapped: Result<Int> = fmap(value, {|n| n + 1})
```

変換系は `From` / `TryFrom` trait が裏側の coherence を担います。

impl coherence は型変数名や宣言順ではなく、Trait 引数と target 型の構造で決まります。generic は任意の型と一致し、型コンストラクタの内側も再帰照合するため、`List<$A>` と `List<Int>` は重複です。Surtr V1 は specialization の優先順位を持たず、overlap は compile error にします。`List<Int>` と `List<String>` のように同時成立しない pattern は併存できます。

この判定は method body を実行・生成する前の typecheck で行われます。user code の impl conflict が runtime や codegen error になることはありません。

`From` / `TryFrom` の排他も同じ照合を使うため、generic parameter を `$A` から `$T` へ改名して回避することはできません。

```text
xldr(1)> print(match try_from::<Int>("42") { Ok(value) => to_string(value), Err(err) => inspect(err), })
42
xldr(2)>
```

## 読み方

- `deftrait Name { ... }`
  - method signature だけを持つ契約 namespace
- `impl Trait for Type { ... }`
  - concrete 実装
- `impl Trait<Concrete> for Type { ... }`
  - target type つき trait 実装

型注釈と明示型引数のルールは `./type-annotations.md` にまとめています。

## 関連ページ

- 変換の呼び出し surface は `./definitions-and-usage.md`
- 型注釈は `./type-annotations.md`
- Trait の制約・親 Trait・coherence の契約は `./trait-system.md`
- 標準定義ソース内での位置づけは `./standard-modules.md`
- 制約一覧は `./language-reference.md`

## 確認したソース

- ソース
  - `../../lib/traits/from.srt`
  - `../../lib/traits/try_from.srt`
  - `../../lib/types/int.srt`

## 躓きやすいポイント

- 匿名 `impl Trait` 型は使わず、名前付き型変数と `where` clause で制約します。
- `where` clause と multi-trait bound は signature slot の制約として使えます。
- `+`, `-`, `*` は `Add` / `Sub` / `Mul` の dispatch です。
- `|*>`, `|*|`, `|>=` はそれぞれ `Functor::fmap`, `Applicative::ap`, `Monad::bind` の dispatch です。
- `|*|` は未カリー化 callable を暗黙変換しません。複数引数では `curry()` を明示します。
- `From` / `TryFrom` の呼び出し surface は簡潔でも、coherence 自体は trait 実装側で管理されています。
- 1 つの `defmod` / `impl` block に同名 method を複数定義できません。signature や `def` / `defp` を変えても overload にはなりません。
