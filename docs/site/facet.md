# Facet

`Facet` は同一スコープ内でのみ使用可能な path capability です。  
正本の `@doc` は `../../lib/facet.srt` にあります。

## まず API を見る

- `Facet::view(facet, source)`
- `Facet::preview(facet, source)`
- `Facet::set(facet, source, value)`
- `Facet::over(facet, source, update_fun)`
- `Facet::over_result(facet, source, update_fun)`
- `Facet::case_set(facet, source, value)`
- `Facet::case_over(facet, source, update_fun)`
- `Facet::bulk_update(source) { ... }`
- `outer / inner`
- `Facet::chain(outer, inner)`

`T?` は `Option<T>` に下がります。
optional field は `Option.Some` / `Option.Some?` を通じて
`Facet::case_set` / `Facet::case_over` で短く扱えます。
`Result` を返す helper と直接つなぎたい field では、
`Result<T, NoneError>` を明示的に使います。

また、source を伴う API では `~source.path` shorthand が使えます。
これは source 実体と structural path の組を compiler-managed に expand する sugar で、
source expression はちょうど 1 回だけ評価されます。
API 固有の制約自体は `Facet::*` 側が通常どおり判定します。

## tuple path

tuple path は REPL でそのまま試しやすいです。

`Tuple._N` は `Facet` 文脈の中だけでなく、同一スコープの一時 binding としても
保持できます。binding は deferred path として扱われ、あとから
`Facet::view/set/over/over_result` や `/` に渡した時点で concrete な
`Facet<S, A>` に specialize されます。

### get

```text
xldr(1)> pair = ("alice", 42)
> pair: (String, Int) = ("alice", 42)
xldr(2)> print(Facet::view(Tuple._0, pair))
alice
xldr(3)>
```

### set

```text
xldr(1)> pair = ("alice", 42)
> pair: (String, Int) = ("alice", 42)
xldr(2)> pair2 =? Facet::set(Tuple._1, pair, 99)
> pair2: (String, Int) = ("alice", 99)
xldr(3)> print(inspect(pair2))
("alice", 99)
xldr(4)>
```

### deferred binding

```text
xldr(1)> pair = ("alice", 42)
> pair: (String, Int) = ("alice", 42)
xldr(2)> second = Tuple._1
> second: Facet<_, _> = Tuple._1
xldr(3)> print(Facet::view(second, pair))
42
xldr(4)>
```

## `var_name.lenspath` での参照

`Facet` では `var_name.lenspath` 形式の sugar が使えます。  
これは `value.segment` をその値へ適用する read sugar です。

```surtr
name = user.name
first = pair._0
```

概念的には次と同じです。

```surtr
name = Facet::view(User.name, user)
first = Facet::view(Tuple._0, pair)
```

更新系では shorthand も使えます。

```surtr
name = Facet::view(~user.name)
user2 =? Facet::set(~user.name, "bob")
pair2 = Facet::put(~pair._1, 99)
```

`~source.path` は first-class `Facet` 値にはなりません。
binding、関数引数、戻り値、container 格納には使えず、
`Facet::view/preview/put/set/over/over_result` の第1引数位置でだけ消費できます。

## `_.path` inferred capture

`_.path` は field / Facet path を unary function として扱うための推論付き capture です。
`|*>`、`Function::on`、関数引数など、期待型が `A -> B` になる場所で使えます。

```surtr
users |*> _.name
users |*> _.profile.name
pairs |*> _._0
List::sort_by(users, &compare `Function::on` _.age)
```

source 型を明示したい場合は `&Type.path` を使います。

```surtr
users |*> &User.name
```

`_.path` は standalone の Facet 値ではありません。文脈から source 型を推論できない
場所では compile error になります。

- 文脈で source 型が決まるなら `_.name`
- source 型も source 上で明示したいなら `&User.name`

## `&Type.path` explicit capture

`&User.name` は type-root の FacetPath を unary capture として使う明示形です。
`User.name` 自体は `Facet<User, String>` の path ですが、`&User.name` は
`(User -> String)` が期待される場所で使う callable になります。

```surtr
users |*> &User.name
List::sort_by(users, &compare `Function::on` &User.age)
```

- `Facet` 値そのものがほしいときは `User.name`
- unary function として渡したいときは `&User.name`

## struct path

```surtr
defstruct User {
  name: String,
  age: Int,
}

name_facet = User.name
```

- `User.name` で path を作る
- `Facet::view(User.name, user)` で読む
- `Facet::set(User.name, user, "bob")` で置き換える
- `Facet::over(User.age, user, {|age| Ok(age + 1) })` で更新する

`Result` field に対しては次も使えます。

- `Facet::set(User.nickname, user, "bob")`
- `Facet::over(User.nickname, user, normalize)`
- `Facet::over_result(User.nickname, user, rewrite_result)`

`nickname: Result<String, NoneError>` のような field なら、
`Facet::set(...)` の plain `"bob"` は `Ok("bob")` として格納されます。

## container path

List と HashMap の bracket segment は、通常の Facet path では runtime 式を受けられます。

- `List.[expr]` の `expr` は plain `Int`
- `List.[-1]` のような負 index は末尾基準
- `List.[start..end]` は inclusive な slice を選ぶ
- slice endpoint も plain `Int` で、負数は同じく末尾基準
- `HashMap.[expr]` の `expr` は plain `String`
- `Result<Int>` / `Result<String>` はそのまま使えないので、先に `=?` や `match` で unwrap する
- `const Facet<...>` だけは compile-time 固定のままで、bracket segment は literal のみ

```surtr
score =? Facet::view(List.[index + 1], scores)
last =? Facet::view(List.[-1], scores)
window =? Facet::view(List.[1..-1], scores)
talk =? Facet::view(HashMap.[String::trim(kind)], score_by_kind)

score2 =? Facet::set(User.scores.[slot], user, 99)
scores2 =? Facet::set(List.[1..2], scores, [99])
scores3 =? Facet::over(List.[0..1], scores2, {|slice| Ok(List::append(slice, [77]))})
user2 =? Facet::over(User.score_by_kind.[kind], user, {|n| Ok(n + 1)})
```

value-side sugar でも同じです。

```surtr
score =? scores.[index + 1]
talk =? score_by_kind.[kind]
```

1 回の Facet operation 中では、bracket expression は先に 1 回だけ評価され、
read と rebuild の両方で同じ index / key が再利用されます。

single index の focus は要素 `A`、slice の focus は `List<A>` です。
そのため `Facet::set(List.[1..2], source, replacement)` は slice 置換になり、
`replacement` の長さは元と同じでなくてもかまいません。
`Facet::over(List.[1..2], source, update_fun)` も `update_fun: (List<A> -> Result<List<A>>)`
として slice 全体を受け取ります。

`put` は引き続き infallible structural path 専用です。
そのため `List.[expr]` / `List.[start..end]` / `HashMap.[expr]` は literal かどうかに関係なく `put` では使えません。

## record path

```surtr
defrecord Config(host: String, port: Int)

host_facet = Config.host
```

record でも読み方は同じです。

- `Facet::view(Config.host, config)`
- `Facet::set(Config.port, config, 8080)`
- `Facet::over(Config.port, config, {|port| Ok(port + 1) })`

## enum path

enum variant selector は fallible path です。

```surtr
defenum Expr {
  IntLit(Int),
  Add((Expr, Expr)),
}

add_facet = Expr.Add
```

- `Facet::view(Expr.Add, expr)` は `Result<...>` になる
- `Facet::preview(Expr.Add, expr)` は variant path 専用の明示 API
- 現在値が別 variant なら `Err(...)` になる
- `set` / `over` でも同じく variant mismatch が失敗になる
- `over_result` は `Result` focus 全体を書き換えたいときに使う
- `case_set` / `case_over` は「最後の enum case payload を更新する」専用 API

```surtr
next =? Facet::case_set(Expr.Add, expr, (left2, right2))
next2 =? Facet::case_over(Expr.Add, expr, {|pair| Ok(rewrite_pair(pair))})
```

## bulk_update

`Facet::bulk_update(source) { ... }` は、1 つの state に対する複数の Facet 更新を
source order でまとめて書くための special form です。

- 返り値は常に `Result<S>`
- block は通常 block ではなく、`match` に近い専用 surface
- entry は改行区切りで、`,` は使わない
- 許可される entry は次だけ
- `path <- set(value)`
- `path <- over(update_fun)`
- `path <- over_result(update_fun)`
- `path <- case_set(payload)`
- `path <- case_over(update_fun)`
- `path { nested_entries... }`

```surtr
updated =? Facet::bulk_update(user) {
  name <- set("taro")
  age <- over({|age| Ok(age + 1)})
  account {
    score <- over_result({|score: Result<Int>| Ok(Ok(9))})
  }
}
```

ネスト block は path prefix を積む sugar です。
そのため `address.country <- set("Tokyo")` と
`address { country <- set("Tokyo") }` は同じ更新へ lower されます。

`bulk_update` は `Facet::set` / `Facet::over` / `Facet::over_result` の並びへ
lower される範囲に限定されています。`case_*` も同じ要領で通常の Facet API へ展開されます。
`S -> Result<S>` の whole-state updater を
混ぜたい場合は、普通の関数として bulk の外で `|>=` 合成します。

container path も通常の Facet API と揃っています。

```surtr
updated =? Facet::bulk_update(user) {
  scores.[index + 1] <- over({|score| Ok(score + 10)})
  score_by_kind.[kind] <- set(9)
}
```

## chain

ネストした path は `outer / inner` でつなぎます。`Facet::chain(...)` も同じ意味で使えます。

```surtr
defstruct Profile {
  name: String,
}

defstruct User {
  profile: Profile,
}

impl Profile {
  def new(name: String) -> Self {
    Profile { name: name }
  }
}

impl User {
  def new(profile: Profile) -> Self {
    User { profile: profile }
  }
}

profile_name = User.profile / Profile.name
# or
profile_name = Facet::chain(User.profile, Profile.name)
# or
profile_name = User.profile.name
```

chain した path は REPL や inspect 表示で canonical path に圧縮されます。
つまり `User.profile / Profile.name` と `User.profile.name` は同じ path として
扱われ、chain の履歴は表示に残りません。

この canonical 化では、つなぎ目で root path が重複していたら落とします。
たとえば `outer = User.profile` と `inner = Profile.name` を chain した結果は
`User.profile.Profile.name` ではなく `User.profile.name` です。

```text
xldr(1)> outer = User.profile
> outer: Facet<_, _> = User.profile
xldr(2)> inner = Profile.name
> inner: Facet<_, _> = Profile.name
xldr(3)> path = outer / inner
> path: Facet<_, _> = User.profile.name
xldr(4)>
```

同じ規則は tuple segment や variant segment を含む path にも適用されます。

- `Config.entrypoint / Tuple._0` は `Config.entrypoint._0`
- `Expr.Add / Tuple._1` は `Expr.Add._1`

`/` は path を組み立てる surface であり、表示時には canonical path へ正規化されます。

## REPL で path を確認する

`Facet` binding は同一スコープ内で使う path capability なので、REPL では
`:type` / `:info` / `:facet` を役割分担して使うのが自然です。

- `:type path`
  - `Facet<S, A>` または未解決なら `Facet<_, _>` を見る
- `:info path`
  - type と canonical `full path` を軽く確認する
- `:facet path`
  - segment 一覧と、`Result` 化しうる停止点を詳しく確認する

### `:facet` の例

```text
xldr(1)> :facet Expr.Add.value
type: Facet<Expr, Int>
full path: Expr.Add.value
segments:
1. Expr.Add
   kind: variant
   source: Expr
   focus: (Int)
   fallible: yes
   reason: variant mismatch returns Result
2. value
   kind: field
   source: Add
   focus: Int
   fallible: no
   reason: plain field access
may stop at:
1. Expr.Add - variant mismatch returns Result
```

`Result<T>` source から始める facet binding でも、停止点は `:facet` にまとまります。

```text
xldr(1)> :facet $result_user_name
type: Facet<Result<User>, String>
full path: User.profile.name
may stop at:
1. source - input already starts in Result context
```

dynamic container path の任意式は command query surface では扱いません。REPL では値側で作った facet binding を `$name` で指定します。

```text
xldr(1)> :facet $next_score
type: Facet<User, Int>
full path: User.scores.[index + 1]
```

## `Result` focus の更新

`Facet::set` と `Facet::over` は `Result<A>` focus に対して少し ergonomic です。

- `set` は plain `A` も受け取り、`Ok(A)` を格納する
- `over` は `A -> Result<A>` updater を受け取り、`Ok(value)` の payload だけ更新する
- `over_result` は `Result<A> -> Result<Result<A>>` updater を受け取り、`Ok(...)` / `Err(...)` をまとめて更新する

```surtr
defstruct User {
  nickname: Result<String, NoneError>,
}

normalized =? Facet::over(User.nickname, user, {|name|
  Ok(String::trim(name))
})
```

`String?` / `Option<String>` field で同じことをしたい場合は、
`Facet::case_over(User.nickname.Some?, user, {|name| Ok(String::trim(name))})`
のように enum case payload を更新します。

## 制約

- `Facet` は同一スコープ内でのみ使用可能
- 関数引数として渡したり、戻り値にしたり、`List` や `Result` に入れたりしない
- private field path は、その private field が見えるスコープの外では compile error になる
- readonly は path 作成ではなく mutating Facet operation に対して判定される

## readonly

- `readonly profile: Profile` のような readonly field は read 用 path としては使えます
- `Facet::view(User.profile.name, user)` のような read は許可されます
- `Facet::set(User.profile.name, user, "bob")` のような深い mutable traversal は拒否されます
- owner の `impl User` 本体では `Facet::set(User.profile, self, next_profile)` のような property そのものの置換だけが許可されます
- `@readonly defstruct Profile { ... }` は readonly root になり、`Facet::set(Profile.name, profile, ...)` のような mutable Facet operation を owner を含めて拒否します

## private field path

private field を path root にした `Facet` は、スコープ外では作れません。

```surtr
facet = User.password
```

これは `User.password` が private のとき、外側スコープでは compile error です。  
同様に `Facet::view(User.password, user)` のような参照も拒否されます。  
また `user.password` のような value access も同じ field access lowering を通るため、owner の `impl Type` / `impl Trait for Type` の外では拒否されます。

詳しい外部契約は `../../lib/facet.srt` と `./standard-library.md` の `Facet` 節を参照してください。

## 確認したソース

- ソース
  - `../../lib/facet.srt`
  - `../../crates/scar/src/lib.rs`

## 躓きやすいポイント

- `var_name.lenspath` は read sugar であって、field access 一般の許可とは同義ではありません。private field は見える範囲でしか path にできず、`value.private_field` も同じ境界で拒否されます。
- `Tuple._0` のような tuple root は、同一スコープの local binding として保持できます。`Facet::view(...)` や `/` で同じスコープ内に消費してください。
- chain した path は canonical 表示へ圧縮されるので、`User.profile / Profile.name` を inspect すると `User.profile.name` に見えます。`/` の組み立て履歴そのものは残りません。
- variant path や `Result<T>` source を含むと、どこで `Result` 化しうるかは `:facet <FacetPath|binding>` で確認するのが一番わかりやすいです。
- スコープをまたぐときは `Facet` ではなく、`Facet::view(...)` 済みの値を渡します。
- `Result` を返す updater とつなぐ field には、`Option<T>` より `T?` の方が更新パイプが短くなります。
- `List.[expr]` / `List.[start..end]` / `HashMap.[expr]` は普通の path では runtime 式を許可しますが、`const Facet<...>` では literal だけに絞られます。
- `bulk_update` は DSL ですが path 能力は通常の Facet API と揃っているので、dynamic bracket や `case_*` も同じ感覚で使えます。
