# `@derive`

`@derive` は、データ型に標準 Trait の実装を自動生成するアノテーションです。比較や文字列表現のための定型的な `impl` を自分で書かずに済みます。

対象は `defstruct`、`defrecord`、`defenum` です。

## 基本の書き方

`@derive` は型定義の直前に一度だけ置き、Trait 名をカンマで区切ります。

```surtr
@derive Eq, Compare, Show
defstruct User {
  name: String,
  age: Int,
}
```

生成された実装は通常の Trait 実装と同じように利用できます。

```surtr
user = User("alice", 30)
same_user = User("alice", 30)
older_user = User("alice", 31)

print(inspect(user == same_user))
print(inspect(user != older_user))
print(inspect(user < older_user))
print(to_string(user))
```

## 生成される Trait

| 指定 | 生成されるメソッド | 生成規則 |
| --- | --- | --- |
| `Eq` | `eq(self, rhs) -> Boolean` | 全フィールドを宣言順に比較し、すべて等しければ `True` neqも使えるようになる|
| `Compare` | `compare(self, rhs) -> Ordering` | フィールドを宣言順に辞書順比較 |
| `Show` | `to_string(self) -> String` | `inspect(self)` を呼び出す |
| `Default` | `default::<Self>() -> Self` | 各 field / payload を `default()` で生成 |

### `Eq`

構造体・レコードでは、すべてのフィールドが `Eq` で比較されます。フィールドがない型は等しい値同士として扱われます。

```surtr
@derive Eq
defrecord Point(x: Int, y: Int)

p = Point(1, 2)
print(inspect(p == Point(1, 2)))
print(inspect(p != Point(1, 3)))
```

enum では、variant が異なれば `False`、同じ variant なら payload を順番に比較します。

### `Compare`

構造体・レコードは lexicographic order（辞書順）です。先頭のフィールドから比較し、`Ordering::Equal` のときだけ次のフィールドを比較します。

```surtr
@derive Compare
defrecord Version(major: Int, minor: Int)

print(inspect(Version(1, 9) < Version(2, 0)))
print(inspect(Version(1, 2) < Version(1, 3)))
```

enum は variant の宣言順を順序として使います。

```surtr
@derive Compare
defenum Color {
  Red,
  Green,
  Blue,
}

print(inspect(Color::Red < Color::Blue))
```

同じ variant に payload がある場合は、payload を宣言順に比較します。比較結果は `Ordering::Less`、`Ordering::Equal`、`Ordering::Greater` のいずれかです。`<` や `<=` などの演算子、`compare` / `lt` / `lte` / `gt` / `gte` helper はこの結果を利用します。詳細は [`trait-impls.md`](./trait-impls.md) と [`range.md`](./range.md) を参照してください。

### `Show`

`Show` は型の `to_string` を `inspect(self)` に委譲します。フィールドごとの `Show` 実装は要求しません。

```surtr
@derive Show
defstruct User {
  name: String,
  age: Int,
}

user = User("alice", 30)
print(to_string(user))
# => "User(name: alice, age: 30)"
```

`inspect` を直接呼び出した場合の quote を含む表示など、表示形式の詳細は [`structs.md`](./structs.md) を参照してください。

### `Default`

`Default` は、型定義者が各 field / payload の default 値で値を構築してよいことを明示する derive です。

```surtr
@derive Default
defstruct Config {
  retries: Int,
  label: String,
}

config: Config = default()
```

struct の生成コードは、概念的には次のような構造体リテラルになります。

```surtr
Config {
  retries: default(),
  label: default(),
}
```

`Default` derive は `Config::new(...)` や `Config(...)` を呼びません。したがって、`new` が `Self` を返すか `Result<Self, Error>` を返すかには影響されません。また、`Default::default` の戻り値は常に `Self` です。

record も各 public field の default 値から直接構築されます。

構造体リテラルは `impl Config` の同型メソッド本体内だけで許可されるため、derive は型所有者側の自動生成としてこの構築を行います。derive しない型に default 構築経路は追加されません。

ただし、`Default` は型固有の不変条件を検査しません。たとえば `new(value) -> Result<Self, Error>` が `value > 0` を検証していても、`Int` の default が `0` なら、その型に `@derive Default` を付けるのは不適切です。default 値自体が妥当な型だけに指定してください。


## struct・record・enum での使用

3 種類のデータ宣言で同じ構文を使えます。

```surtr
@derive Eq, Compare, Show
defstruct User {
  name: String,
  age: Int,
}

@derive Eq, Compare, Show
defrecord Point(x: Int, y: Int)

@derive Eq, Compare, Show
defenum ParseResult {
  Success(Int),
  Failure(String),
}
```

generic な struct / enum にも付けられます。生成された比較処理はフィールドや payload の Trait 実装を利用するため、実際に使う型が `Eq` または `Compare` を満たす必要があります。

## 制約

- `@derive` は `defstruct`、`defrecord`、`defenum` の直前にだけ置けます。
- 1 つの型に複数の `@derive` を置けません。
- Trait 名は bare identifier で指定します。`Eq<Int>` や qualified path は使えません。
- Trait 名は少なくとも 1 つ必要です。
- 同じリスト内で Trait 名を重複させられません。
- 指定できるのは `Eq`、`Compare`、`Show`、`Default` です。ユーザー定義 Trait の derive recipe はまだ登録できません。
- `deferror`、`deftrait`、`impl`、関数、module、`@builtin type` などには付けられません。
- derive で生成される Trait と同じ型の明示的な `impl` は書けません。

たとえば、次のコードは `Eq` の実装が重複するためエラーになります。

```surtr
@derive Eq
defstruct User {
  value: Int,
}

impl Eq for User {
  def eq(self: Self, rhs: Self) -> Boolean {
    self.value == rhs.value
  }
}
```

未知の Trait は `UnknownDerivedTrait`、明示 `impl` との衝突は `DerivedImplConflict` として報告されます。対象外の宣言は `DeriveNotAllowed`、リスト内の重複は `DuplicateDerivedTrait` です。

## 使い分け

定型的な構造比較・順序比較・表示だけが必要なら `@derive` を使います。比較規則を変更したい、フィールドの一部だけを比較したい、独自の表示形式にしたい場合は、derive せずに通常の `impl Trait for Type` を定義してください。

関連する Trait の契約は [`trait-system.md`](./trait-system.md)、具体的な実装と dispatch は [`trait-impls.md`](./trait-impls.md) にまとめています。
