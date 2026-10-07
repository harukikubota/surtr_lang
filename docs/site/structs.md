# Structs

`defstruct` の利用者向けルールをこのページに集約します。  
宣言と型検査の現行挙動は compiler source と fixtures、Facet の詳細は `./facet.md` を参照してください。

## 定義

```surtr
defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name, age }
  }
}
```

`defstruct` は名前付きフィールドを持つデータ型です。  
`impl User` は `User` 専用の namespace で、構築 helper や分解 helper を置きます。

型parameterのbinderとcapability constraintは分離します。`Monad`のようなTypeCtorTraitを
要求するparameterは、`where` constraintを通じてfield型のconstructorとして適用できます。

```surtr
defstruct OptionT<$M, $A>
where
  $M: Monad
{
  inner: $M<Option<$A>>,
}
```

`OptionT<Result, Int>`の`Result`はcompile-timeのbare constructor headです。runtimeには
`Result<Option<Int>>`のfield値だけが入り、Trait objectやdictionaryは保持しません。受理するのは
`Result` / `Option` / `List`と、要求されたTypeCtorTrait implを持つuser-defined headです。
通常のnominal型注釈では部分適用や型lambdaは使えません。generic関数から`OptionT<$M, $A>`を使う場合は、
`where $M: Monad`のようにconstraintを明示します。call-site ReturnTypeArgumentでは、完全・部分型applicationと
`_`を含むapplicationを通常のTypeCtorTrait入力として使えます。

欠損可能 field を持たせるときは、`T?` または `Option<T>` を使います。
`T?` は `Option<T>` に下がる sugar です。
`Result` を返す helper 関数とつなぐときは、必要に応じて
`to::<Result>(value)` / `to::<Option>(value)` を明示します。

## 構築ルール

`defstruct` には `new` が必須です。

この必須条件により、式位置の `Type(...)` から呼出し先 `Type::new` をコンパイル時に解決できます。実行時に構築が必ず成功するという意味ではありません。`new` は `Self` または `Result<Self, E>` を返せます。`Result` を返す場合、`Type(...)` もその `Ok` / `Err` をそのまま返します。

- `impl User { def new(...) -> Self { ... } }` を定義する
- `User(...)` は `User::new(...)` の糖衣として解決される
- `User::new` は import 対象外
- `User` という構造体 head 自体も import 対象外

```surtr
user = User("alice", 30)
# 上と同じ意味
user2 = User::new("alice", 30)
```

Struct の constructor も capture できます。bare capture の引数順は field 順ではなく
`Type::new` の引数宣言順です。

```surtr
make_user: (String, Int -> User) = &User
make_fixed: (String -> User) = &User(&1, 30)
```

constructor capture の引数は位置指定だけで、named argument と placeholder のない引数付き
capture は拒否されます。詳細は [`capture-operator.md`](./capture-operator.md) を参照してください。

引数規約は関数呼び出しと同じです。

- 名前付き引数は使える
- 位置引数と名前付き引数の混在は禁止

```surtr
user = User(name: "alice", age: 30)
```

## 構造体リテラル

```surtr
User { name, age }
```

`Type { ... }` 形式の構造体リテラルは、`impl Type` の同型メソッド本体内でのみ使えます。  
外側の通常コードから `User { ... }` を直接作るのではなく、`User(...)` または `User::new(...)` を通します。

この制約により、構築の公開入口は `new` に固定されます。

ただし、`@derive Default` を指定した型には、型所有者側の自動生成として field default から直接構築する `Default::default() -> Self` が追加されます。この経路は `new` を呼ばず、構造体リテラルを使います。`Default` を指定しない型には、この経路は追加されません。

`Default` は型定義者による明示的な許可であり、不変条件の検査ではありません。`new` が入力値を検証して `Result<Self, Error>` を返す型では、field の default 値も妥当である場合にだけ `@derive Default` を指定してください。

field 名と同じ名前のローカル変数・引数・`self` 由来の値を入れるだけなら、shorthand を使えます。

```surtr
impl User {
  def new(name: String, age: Int) -> Self {
    User { name, age }
  }

  def with_age(self: Self, next_age: Int) -> Self {
    User { name: self.name, age: next_age }
  }
}
```

- `User { name }` は `User { name: name }` の sugar
- shorthand と明示 field は混在可能
- shorthand は struct literal 専用で、`User(...)` の named argument や pattern には広がらない

`inspect(...)` は構造体の全フィールドを定義順に表示します。private フィールドも省略せず、呼び出し側のスコープや `Show` の有無によって表示を変えません。

- `inspect(User("alice", 30))` は `User(name: "alice", age: 30)` と表示される
- フィールド値は再帰的に `inspect` の規則で表示し、入れ子の String も引用する
- 内部専用の `User { ... }` 構造体リテラルは表示に使わない

`to_string(...)` は対象型の `Show` 実装に従います。上の `User` に `@derive Show` を付けた場合は `User(name: alice, age: 30)` と表示されます。`Show` がない型の `to_string` は型検査で拒否し、`inspect` で暗黙に代用しません。手書きの `Show` で明示的に `inspect(self)` を呼ぶことはできます。

`inspect` は診断用の String を返します。表示されたフィールド列と `new` の引数は一致するとは限らず、表示全体を再入力できることは保証しません。構築と分解の契約は変わりません。

## `new` と `deconstruct` の関係

Surtr では、構造体の構築と分解は別の入口です。

- 式位置の `User(...)` は `User::new(...)`
- `match` / `=?` の MatchBlock 位置の `User(...)` は `User::deconstruct(...)`

つまり同じ surface でも、式なのか pattern なのかで意味が変わります。

### `deconstruct` を使う例

```surtr
defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name, age }
  }

  defextractor deconstruct(self: Self) -> MatchResult<(String, Int), Error> {
    MatchResult::Ok((self.name, self.age))
  }
}

user = User("alice", 30)

print(match user {
  User(name, age) => name ++ ":" ++ to_string(age),
  _ => "fallback",
})

User(name, age) =? user
print(name)
# => "alice"
```

押さえる点は次のとおりです。

- `new` は常に必須
- `deconstruct` は constructor pattern を使いたいときに定義する
- `match user { User(...) => ... }` は attached extractor `User::deconstruct` を要求する
- `deconstruct` が未定義なら compile error になる

`new` の定義だけでは Pattern による分解は可能になりません。`deconstruct` がない場合に、フィールドの直接分解や別の Extractor で補うことはありません。

`deconstruct` の一般的な extractor 契約は `./extractors.md`、pattern 全体は `./pattern-matching.md` を参照してください。

## プライベートフィールド

構造体フィールドは `private` を付けられます。

```surtr
defstruct User {
  name: String,
  private password: String,
}
```

private フィールドへの直接アクセスと Facet の導出は、所有者の `impl User` 内だけで許可されます。

- `User.password` のような type-root access は所有者の `impl User` の外では不可
- `user.password` のような value access も同じく 所有者の `impl User` の外では不可
- closure の中かどうかで特別扱いはされず、field access が path segment を作る時点で同じ規則が適用される

```surtr
impl User {
  def password_via_reader(self) -> String {
    password = self.password
    reader = {|| password}
    reader()
  }
}
```

上のように owner impl の内側で一度 plain value として取り出してから closure に渡す形は許可されます。  
一方で impl の外側にある `{|| user.password}` は compile error です。

Facet の `User.password` path も同じ private 境界に従います。path と更新 API の詳細は `./facet.md` を参照してください。

`impl Trait for User` も外側スコープです。private フィールドを使うトレイト実装では、`impl User` に公開関数を定義し、その関数に委譲します。

`private` はフィールドの扱い方を型の定義側で管理するための境界であり、情報の秘匿は保証しません。たとえば `inspect` は private な password の名前と内容も表示します。表示された String を解析しても、元のフィールド参照や Facet、更新権限は得られません。型の定義側が公開関数で値を返すことはできます。

## プロパティアクセス

構造体の読み取りは `value.field` です。

```surtr
print(user.name)
print(to_string(user.age))
```

- `value.field` は `defstruct` / `defrecord` で使える
- enum 値に対する field access はない
- field の更新は代入ではなく、新しい値を組み立てる helper か Facet API で扱う

### optional field の値変換と Facet 更新

`T?` は `Option<T>` の短い表記で、使える操作は同じです。
field の値を取り出して `Result` を返す関数へ渡す場合は、明示的な変換を組み合わせます。

```surtr
defstruct User {
  nickname: Option<String>,
}

next =
  user.nickname
  |> to::<Result>()
  |>= normalize_name
  |> to::<Option>()
```

この例の `next` は変換後の `Option<String>` です。`user` 自体は更新しません。
構造体の中の `Some` の値を更新するなら、Facet の selector を使えます。

```surtr
defstruct User {
  nickname: String?,
}

next =? Facet::case_over(User.nickname.Some, user, normalize_name)
```

この例の `next` は更新後の `User` です。`nickname: Option<String>` と宣言しても、
同じ `User.nickname.Some` と `Facet::case_over` / `Facet::case_set` を使えます。
値の変換と構造体の更新の違いであり、型の表記による機能差はありません。

たとえば `impl User` 内で `with_age` を定義して再構築できます。

```surtr
impl User {
  def with_age(self: Self, age: Int) -> Self {
    User { name: self.name, age }
  }
}
```

ネストした path や `Facet::set` / `Facet::over` は、このページでは重複させず `./facet.md` へ委ねます。

## パターンマッチ

構造体そのものに専用の field pattern があるのではなく、attached extractor を経由して分解します。

```surtr
match user {
  User(name, age) => name,
  _ => "fallback",
}
```

この `User(name, age)` は、surface 上は constructor に見えても pattern 側では `User::deconstruct` 呼び出しです。  
そのため、struct pattern の設計は `deconstruct` の返す payload shape に従います。

よくある読み方は次の通りです。

- 1値だけ取り出したいなら `MatchResult<Int, Error>` の成功 payload にその値を返す
- 複数値を取り出したいなら tuple にして `MatchResult<(A, B), Error>` を返す
- pattern 側はその shape に合わせて `User(x)` または `User(x, y)` のように書く

## 関連ページ

- Facet と path update は `./facet.md`
- extractor 契約は `./extractors.md`
- pattern 全体は `./pattern-matching.md`
- compact な一覧は `./language-reference.md`

## 確認したソース

- ソース
  - `../../crates/spire/src/parser/decl.rs`
  - `../../crates/scar/src/checker/definitions.rs`
  - `../../tests/integration/language_features/core_language.rs`
  - `../../tests/fixtures/modules/pass/private_visibility_*`
  - `../../tests/fixtures/modules/fail/private_field_*`
