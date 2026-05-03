# Structs

`defstruct` の利用者向けルールをこのページに集約します。  
正本の surface 契約は `../../doc/要件定義v9.md`、Lens の詳細は `./lens.md` を参照してください。

## 定義

```surtr
defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}
```

`defstruct` は名前付きフィールドを持つデータ型です。  
`impl User` は `User` 専用の namespace で、構築 helper や分解 helper を置きます。

欠損可能 field を持たせるときは、まず `T?` を検討してください。
`T?` は `Option<T>` ではなく `Result<T, NoneError>` に下がるため、Lens 更新や
`Result` を返す helper 関数と直接つながります。

## 構築ルール

`defstruct` には `new` が必須です。

- `impl User { def new(...) -> Self { ... } }` を定義する
- `User(...)` は `User::new(...)` の糖衣として解決される
- `User::new` は import 対象外
- `User` という構造体 head 自体も import 対象外

```surtr
user = User("alice", 30)
# 上と同じ意味
user2 = User::new("alice", 30)
```

引数規約は関数呼び出しと同じです。

- 名前付き引数は使える
- 位置引数と名前付き引数の混在は禁止

```surtr
user = User(name: "alice", age: 30)
```

## 構造体リテラル

```surtr
User { name: name, age: age }
```

`Type { ... }` 形式の構造体リテラルは、`impl Type` の同型メソッド本体内でのみ使えます。  
外側の通常コードから `User { ... }` を直接作るのではなく、`User(...)` または `User::new(...)` を通します。

この制約により、構築の公開入口は `new` に固定されます。

`inspect(...)` / `to_string(...)` もこの公開 surface に合わせます。

- `to_string(User("alice", 30))` は `User(name: alice, age: 30)` と表示される
- `inspect(User("alice", 30))` は nested string を quote して `User(name: "alice", age: 30)` と表示される
- 内部専用の `User { ... }` 構造体リテラルは表示に使わない
- private field を含むときは `User(name: "alice", ..private)` や `User(name: alice, ..private)` のように hidden 部分を要約する

このため、private field を含む構造体の表示は人間向けの inspect であり、完全な round-trip code にはなりません。

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
    User { name: name, age: age }
  }

  defextractor deconstruct(self: Self) -> MatchResult<(String, Int), Error> {
    MatchResult::Success((self.name, self.age))
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

`deconstruct` の一般的な extractor 契約は `./extractors.md`、pattern 全体は `./pattern-matching.md` を参照してください。

## プライベートフィールド

構造体フィールドは `private` を付けられます。

```surtr
defstruct User {
  name: String,
  private password: String,
}
```

このときの可視性ルールは少し特徴的です。

- `User.password` のような type-root access は `impl User` / `impl Trait for User` の外では不可
- `user.password` のような value access 自体は許可される
- ただし `user.password` を外側 closure の中で直接読むことは不可

```surtr
password = user.password
reader = {|| password}
```

上のように、一度 plain value として取り出してから closure に渡す形は許可されます。  
一方で `{|| user.password}` は compile error です。

Lens の `User.password` path も同じ private 境界に従います。path と更新 API の詳細は `./lens.md` を参照してください。

## プロパティアクセス

構造体の読み取りは `value.field` です。

```surtr
print(user.name)
print(to_string(user.age))
```

- `value.field` は `defstruct` / `defrecord` で使える
- enum 値に対する field access はない
- field の更新は代入ではなく、新しい値を組み立てる helper か Lens API で扱う

### `Option<T>` field と `T?` field の使い分け

field を「値として optional に持つだけ」なら `Option<T>` でも問題ありません。
ただし、構造体 field を `Lens` で取り出して `Result`-returning helper へ流したい場合は
`T?` の方が更新パイプを短く保てます。

```surtr
defstruct User {
  nickname: Option<String>,
}

next =
  user.nickname
  |> from(Result)
  |>= normalize_name
  |> from(Option)
```

上のように `Option<T>` field は `Result` パイプへ入る前に `Option -> Result`、
戻すときに `Result -> Option` の変換が要ります。

```surtr
defstruct User {
  nickname: String?,
}

next =? Lens::over(User.nickname, user, normalize_name)
```

`nickname: String?` なら field 自体が `Result<String, NoneError>` と同じなので、
`Lens::set` / `Lens::over` / `Lens::over_result` とそのまま噛み合います。

たとえば `impl User` 内で `with_age` を定義して再構築できます。

```surtr
impl User {
  def with_age(self: Self, age: Int) -> Self {
    User { name: self.name, age: age }
  }
}
```

ネストした path や `Lens::set` / `Lens::over` は、このページでは重複させず `./lens.md` へ委ねます。

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

- 1値だけ取り出したいなら `MatchResult<Int, Error>` のように 1値を返す
- 複数値を取り出したいなら tuple にして `MatchResult<(A, B), Error>` を返す
- pattern 側はその shape に合わせて `User(x)` または `User(x, y)` のように書く

## 関連ページ

- Lens と path update は `./lens.md`
- extractor 契約は `./extractors.md`
- pattern 全体は `./pattern-matching.md`
- compact な一覧は `./language-reference.md`

## 確認したソース

- ソース
  - `../../doc/要件定義v9.md`
  - `../../tests/integration/language_features/core_language.rs`
  - `../../tests/spec/modules/private_visibility_*`
  - `../../tests/compile_errors/modules/private_field_*`
