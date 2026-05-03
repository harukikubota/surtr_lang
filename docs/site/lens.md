# Lens

`Lens` は runtime value ではなく、compile-time にだけ存在する path capability です。  
正本の `@@doc` は `../../lib/lens.srt` にあります。

## まず API を見る

- `Lens::view(lens, source)`
- `Lens::set(lens, source, value)`
- `Lens::over(lens, source, update_fun)`
- `Lens::over_result(lens, source, update_fun)`
- `outer / inner`
- `Lens::compose(outer, inner)`

`T?` は `Result<T, NoneError>` に下がるため、optional-looking な field でも
Lens では `Result` focus として扱われます。
そのため `Result` を返す helper とつないで更新したい field には
`Option<T>` より `T?` の方が自然です。

## tuple path

tuple path は REPL でそのまま試しやすいです。

### get

```text
xldr(1)> pair = ("alice", 42)
> pair: (String, Int) = ("alice", 42)
xldr(2)> print(Lens::view(Tuple._0, pair))
alice
xldr(3)>
```

### set

```text
xldr(1)> pair = ("alice", 42)
> pair: (String, Int) = ("alice", 42)
xldr(2)> pair2 =? Lens::set(Tuple._1, pair, 99)
> pair2: (String, Int) = ("alice", 99)
xldr(3)> print(inspect(pair2))
("alice", 99)
xldr(4)>
```

## `var_name.lenspath` での参照

`Lens` では `var_name.lenspath` 形式の sugar が使えます。  
これは `value.segment` をその値へ適用する read sugar です。

```surtr
name = user.name
first = pair._0
```

概念的には次と同じです。

```surtr
name = Lens::view(User.name, user)
first = Lens::view(Tuple._0, pair)
```

## struct path

```surtr
defstruct User {
  name: String,
  age: Int,
}

name_lens = User.name
```

- `User.name` で path を作る
- `Lens::view(User.name, user)` で読む
- `Lens::set(User.name, user, "bob")` で置き換える
- `Lens::over(User.age, user, {|age| Ok(age + 1) })` で更新する

`Result` field に対しては次も使えます。

- `Lens::set(User.nickname, user, "bob")`
- `Lens::over(User.nickname, user, normalize)`
- `Lens::over_result(User.nickname, user, rewrite_result)`

`nickname: String?` なら、`Lens::set(...)` の plain `"bob"` は `Ok("bob")` として格納されます。

## record path

```surtr
defrecord Config(host: String, port: Int)

host_lens = Config.host
```

record でも読み方は同じです。

- `Lens::view(Config.host, config)`
- `Lens::set(Config.port, config, 8080)`
- `Lens::over(Config.port, config, {|port| Ok(port + 1) })`

## enum path

enum variant selector は fallible path です。

```surtr
defenum Expr {
  IntLit(Int),
  Add((Expr, Expr)),
}

add_lens = Expr.Add
```

- `Lens::view(Expr.Add, expr)` は `Result<...>` になる
- 現在値が別 variant なら `Err(...)` になる
- `set` / `over` でも同じく variant mismatch が失敗になる
- `over_result` は `Result` focus 全体を書き換えたいときに使う

## compose

ネストした path は `outer / inner` でつなぎます。`Lens::compose(...)` も同じ意味で使えます。

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
profile_name = Lens::compose(User.profile, Profile.name)
# or
profile_name = User.profile.name
```

## `Result` focus の更新

`Lens::set` と `Lens::over` は `Result<A>` focus に対して少し ergonomic です。

- `set` は plain `A` も受け取り、`Ok(A)` を格納する
- `over` は `A -> Result<A>` updater を受け取り、`Ok(value)` の payload だけ更新する
- `over_result` は `Result<A> -> Result<Result<A>>` updater を受け取り、`Ok(...)` / `Err(...)` をまとめて更新する

```surtr
defstruct User {
  nickname: String?,
}

normalized =? Lens::over(User.nickname, user, {|name|
  Ok(String::trim(name))
})
```

`Option<T>` field でも同じ更新はできますが、`Result` helper とつなぐたびに
`Option -> Result -> Option` の往復変換が必要になります。
`./structs.md` と `./standard-library.md` の `Option` 節も参照してください。

## 制約

- `Lens` は compile-time only
- 同一スコープ内で消費する
- 関数引数として渡したり、戻り値にしたり、`List` や `Result` に入れたりしない
- private field path は、その private field が見えるスコープの外では compile error になる

## private field path

private field を path root にした `Lens` は、スコープ外では作れません。

```surtr
lens = User.password
```

これは `User.password` が private のとき、外側スコープでは compile error です。  
同様に `Lens::view(User.password, user)` のような参照も拒否されます。

詳しい外部契約は `../../lib/lens.srt` と `./standard-library.md` の `Lens` 節を参照してください。

## 確認したソース

- ソース
  - `../../lib/lens.srt`
  - `../../crates/scar/src/lib.rs`

## 躓きやすいポイント

- `var_name.lenspath` は read sugar であって、field access 一般の許可とは同義ではありません。private field は見える範囲でしか path にできません。
- `Tuple._0` のような tuple root は、`Lens` 文脈なしで単独に置くと失敗します。
- `Lens` を closure capture や runtime container に運ぶのではなく、`Lens::view(...)` 済みの値を運ぶのが基本です。
- `Result` を返す updater とつなぐ field には、`Option<T>` より `T?` の方が更新パイプが短くなります。
