# Lens

`Lens` は runtime value ではなく、compile-time にだけ存在する path capability です。  
正本の `@@doc` は `../../lib/lens.srt` にあります。

## まず API を見る

- `Lens::view(lens, source)`
- `Lens::set(lens, source, value)`
- `Lens::over(lens, source, update_fun)`
- `Lens::compose(outer, inner)`

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

## compose

ネストした path は `Lens::compose(...)` でつなぎます。

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

profile_name = Lens::compose(User.profile, Profile.name)
# or
profile_name = User.profile.name
```

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
