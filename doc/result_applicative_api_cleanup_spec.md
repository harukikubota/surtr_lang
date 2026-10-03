# Result Applicative API 整理仕様

日付: 2026-09-28
状態: 実装済み（2026-10-03）

## 目的

`Applicative::ap` / `|*|` 導入前から `impl Result` に残っている、Result 専用の contextual callable 適用 API を削除する。

同じ処理を Result 固有 API と型コンストラクタ Trait の両方で提供せず、通常の適用は `Functor` / `Applicative` / `Monad` の合成へ統一する。旧 API の互換 alias や fallback は残さない。

## 現状

`lib/types/result.srt` には現在、次の Result 専用 API がある。

- `lift`, `lift2`, `lift3`
- `with`, `with2`, `with3`
- `flat_lift`, `flat_lift2`, `flat_lift3`
- `flat_with`, `flat_with2`, `flat_with3`

これらは 2026-04-15 の Result helper 追加時から存在する。一方、現在の `Functor` / `Applicative` / `Monad` surface は 2026-08-11 に導入され、`Applicative::ap` は contextual callable と contextual value の適用、`Monad::bind` は contextual callable の逐次適用を既に担っている。

`lib/types/result.srt` の宣言と `@doc`、利用箇所の監査から、上記 lift/with 系は既存の
型コンストラクタ Trait と重複する。したがって今回は新しい言語意味論を追加せず、標準
surface を整理する level 1 の変更とする。

## 変更する公開 surface

次の 12 API を `impl Result` から削除する。

```text
lift, lift2, lift3
with, with2, with3
flat_lift, flat_lift2, flat_lift3
flat_with, flat_with2, flat_with3
```

削除した名前を deprecated alias、hidden helper、別 module の wrapper として残さない。qualified call、明示 import、`@autoimport impl Result` による bare call のいずれからも解決不能にする。

### 置換規則

- `Result::lift(mapper, value)` は `Applicative::ap(mapper, value)` または `mapper |*| value` に置き換える。
- 複数引数では mapper を Result に入れる前に明示的に `curry` し、`Ok(curry(&add)) |*| left |*| right` のように各 contextual value を左から順に適用する。
- `with*` の value-first 順序は残さない。Applicative の canonical な mapper-first 順序へ書き換える。
- Result を返す mapper は、通常は `value |>= mapper()` / `Monad::bind(value, mapper)` を使う。mapper 自体も Result に包まれている場合は Applicative 適用で得た nested Result を `Monad::bind(..., {|inner| inner})` で一段だけ平坦化できる。専用 `flat_lift*` は残さない。

既存 `lift*` と `Applicative::ap` は mapper を先に、値を左から右へ評価し、最初の `Err` を返す。上記置換ではこの順序を維持する。

### API ごとの置換例

以下では次の callable と値を使う。

```surtr
def inc(value: Int) -> Int { value + 1 }
def add2(left: Int, right: Int) -> Int { left + right }
def add3(a: Int, b: Int, c: Int) -> Int { a + b + c }

def safe_inc(value: Int) -> Result<Int> { Ok(value + 1) }
def safe_add2(left: Int, right: Int) -> Result<Int> { Ok(left + right) }
def safe_add3(a: Int, b: Int, c: Int) -> Result<Int> { Ok(a + b + c) }

value = Ok(1)
left = Ok(1)
right = Ok(2)
third = Ok(3)

mapper = Ok(&inc)
mapper2: Result<(Int, Int -> Int)> = Ok(&add2)
mapper3: Result<(Int, Int, Int -> Int)> = Ok(&add3)

safe_mapper = Ok(&safe_inc)
safe_mapper2: Result<(Int, Int -> Result<Int>)> = Ok(&safe_add2)
safe_mapper3: Result<(Int, Int, Int -> Result<Int>)> = Ok(&safe_add3)
```

#### `lift` / `with`

```surtr
# before
Result::lift(mapper, value)
Result::with(value, mapper)

# after: どちらも mapper-first の canonical Applicative 順序へ統一
mapper |*| value

# qualified form
Applicative::ap(mapper, value)
```

#### `lift2` / `with2`

旧 API は Result 内に未カリー化 callable を受け取る。置換時は Result 内で明示的に `curry` してから、各値を左から順に適用する。

```surtr
# before
Result::lift2(mapper2, left, right)
Result::with2(left, right, mapper2)

# after
(mapper2 |*> {|fun| curry(fun)}) |*| left |*| right

# mapper の生成箇所も変更できる場合は、先に curry する方が短い
Ok(curry(&add2)) |*| left |*| right
```

#### `lift3` / `with3`

```surtr
# before
Result::lift3(mapper3, left, right, third)
Result::with3(left, right, third, mapper3)

# after
(mapper3 |*> {|fun| curry(fun)}) |*| left |*| right |*| third

# mapper の生成箇所も変更できる場合
Ok(curry(&add3)) |*| left |*| right |*| third
```

#### `flat_lift` / `flat_with`

mapper 自体が Result に包まれたままなら、Applicative 適用が返す `Result<Result<B>>` を `bind` で一段だけ平坦化する。

```surtr
# before
Result::flat_lift(safe_mapper, value)
Result::flat_with(value, safe_mapper)

# after: wrapped mapper をそのまま移行する形
(safe_mapper |*| value) |>= {|inner| inner}

# 通常はこちらへ単純化する。safe mapper を Result に包む必要がない
value |>= safe_inc()
```

#### `flat_lift2` / `flat_with2`

```surtr
# before
Result::flat_lift2(safe_mapper2, left, right)
Result::flat_with2(left, right, safe_mapper2)

# after: wrapped mapper をそのまま移行する形
((safe_mapper2 |*> {|fun| curry(fun)}) |*| left |*| right)
|>= {|inner| inner}

# mapper を Result に包まない形へ変更できる場合
left |>= {|left_value|
  right |>= {|right_value|
    safe_add2(left_value, right_value)
  }
}
```

#### `flat_lift3` / `flat_with3`

```surtr
# before
Result::flat_lift3(safe_mapper3, left, right, third)
Result::flat_with3(left, right, third, safe_mapper3)

# after: wrapped mapper をそのまま移行する形
((safe_mapper3 |*> {|fun| curry(fun)}) |*| left |*| right |*| third)
|>= {|inner| inner}

# mapper を Result に包まない形へ変更できる場合
left |>= {|a|
  right |>= {|b|
    third |>= {|c|
      safe_add3(a, b, c)
    }
  }
}
```

`flat_lift2` / `flat_lift3` の Applicative 形式は入力 Result を独立に集めてから nested Result を平坦化する。`bind` だけの形式も左から右へ short-circuit する。どちらも現行 Result Applicative/Monad では最初の `Err` を保持する。

## 維持するもの

### MonadT

`MonadT::lift` は削除対象ではない。これは base carrier `$M<$A>` を transformer carrier `Self<$A>` へ一段埋め込む Trait method であり、Result 専用の contextual callable 適用とは意味も dispatch 境界も異なる。

`lift_compose` / `>*` も callable composition の Trait surface であり対象外とする。

### Result 固有 API

次は Applicative の成功側適用では代替できないため維持する。

- variant 判定: `is_ok`, `is_err`
- error の置換・cause 構築・連結: `map_err`, `cause`, `chain`
- error recovery: `recover`, `recover_kind`
- Error の non-escaping 観測: `tap_err`
- `List<Result<A>>` の順序付き収集: `all`

特に `tap_err` は `Error` を通常値として外へ出さない compiler 契約の許可 surface でもあり、Functor/Applicative への置換対象にしてはならない。`all` は現在の標準 Trait に `Traversable` / `sequenceA` 相当がなく、単純な `ap` 一回では置換できない。

## 今回は削除しない追加候補

次も Trait 合成で表現できるが、今回の Applicative 導入に直接対応する lift/with family とは分ける。

- `zip`, `zip3`: curried tuple builder と Applicative で表現可能
- `flatten`, `then`: Monad の `bind` で表現可能
- `tap_ok`: Functor の mapper 内で観測して元値を返すことで表現可能

これらは直接扱える API として維持する。式の合成を毎回書く冗長さを避けるため、合成で表現できることだけを理由に削除しない。`then` は削除対象への内部依存をなくし、既存の動作を保つ。

### 追加候補ごとの置換例

#### `zip`

```surtr
def pair(left: $A, right: $B) -> ($A, $B) { (left, right) }

# before
Result::zip(left, right)

# after
Ok(curry(&pair)) |*| left |*| right
```

#### `zip3`

```surtr
def triple(a: $A, b: $B, c: $C) -> ($A, $B, $C) { (a, b, c) }

# before
Result::zip3(first, second, third)

# after
Ok(curry(&triple)) |*| first |*| second |*| third
```

#### `flatten`

```surtr
nested: Result<Result<Int>> = Ok(Ok(1))

# before
Result::flatten(nested)

# after
nested |>= {|inner| inner}
```

#### `then`

`next()` は左辺が `Ok` の場合だけ評価される。

```surtr
def next() -> Result<String> { Ok("next") }

# before
Result::then(value, &next)

# after
value |>= {|_| next()}
```

#### `tap_ok`

Functor mapper 内で観測後に同じ payload を返す。左辺が `Err` なら observer は呼ばれない。

```surtr
def observe(value: Int) -> Unit {
  print("observed:#{to_string(value)}")
}

# before
Result::tap_ok(value, &observe)

# after
value |*> {|inner|
  observe(inner)
  inner
}
```

`tap_err` は同じ形へ置換できない。Functor/Applicative は `Err` payload を公開せず、`tap_err` は `Error` non-escaping 観測を許可する compiler 契約でもあるため維持する。

Result API の完全な一覧は `lib/types/result.srt` の宣言と `@doc` を正本とし、別文書へ
手書きで複製しない。`recover`, `recover_kind`, `tap_err`, `all` などの維持 API は source 上で
同期して説明する。

## 影響範囲

製品コード側で lift/with family を参照する Rust 実装はない。既知の変更対象は次の通り。

- `lib/types/result.srt`: 12 API と各 `@doc` を削除
- `lib/tests/result.srt`: 旧 API のテストを、元の処理を再現する Applicative / Monad 合成の成功・Err 順序境界へ置換。削除名の解決失敗を確認するテストは追加しない
- `tests/fixtures/script/pass/stdmod/result_helpers.srt` と `.expected`: 対象呼び出しと出力を削除または Applicative の代表例へ置換
- `crates/xldr/tests/repl_core.rs`: authored signature の補完・署名テストを、維持する `Result::then` へ置換。製品 Rust コードは変更しない

`tests/fixtures/script/pass/stdmod/result_helpers.srt` は Rune の dump/peephole 統合テストからも入力として参照される。期待値を直接固定してはいないが、fixture 更新後に Surtr の直接実行で `.expected` と出力を照合する。対応する Rune の dump テストも確認する。

repository 内の通常 example / site docs には lift/with family の利用は見つからなかった。`MonadT::lift` の tests/docs は名前が似ていても変更しない。

## 成功・拒否境界

### 成功

- `Ok(&inc) |*| Ok(1)` が `Ok(2)` になる。
- `Ok(curry(&add)) |*| Ok(1) |*| Ok(2)` が `Ok(3)` になる。
- mapper が `Err` の場合は値側より mapper の Error を返す。
- 複数値の途中が `Err` の場合は左から最初の Error を返し、それ以降の mapper 適用を行わない。
- `MonadT::lift(base)` は従来どおり解決・実行できる。

削除名が解決不能になることは定義と参照の静的確認で確かめる。関数が存在しないことを検証するテストは追加しない。暗黙 curry や未カリー化 callable の `|*|` 適用を追加する変更も行わない。

## 受入条件

1. lift/with family 12 API の定義・`@doc`・Surtr コード内の参照が残っておらず、標準定義から生成される completion/signature surface に公開されない。
2. 旧 API を経由しない Applicative の unary / curried multi-argument 成功例が通る。
3. mapper-first、値の左から右という Err の優先順位が維持される。
4. `MonadT::lift`、`lift_compose`、Result 固有の Error API、`all`、`zip`、`zip3`、`flatten`、`then`、`tap_ok` を維持する。`then` の内部実装は依存除去のため変更してよい。
5. 旧 API の互換 alias、名前ベース fallback、compiler special-case を追加しない。

## 実装時の検証

標準定義の整理は level 1 とする。追加依頼により Rust の既存テスト期待値も修正し、対象 bucket と共有する REPL core を検証する。

```sh
cargo run -- test --quiet result.srt
cargo run --quiet -- run tests/fixtures/script/pass/stdmod/result_helpers.srt
cargo run -- test --quiet --all
rtk cargo nextest run -p xldr --test repl_core repl_core_bucket_1
rtk cargo nextest run -p xldr --test repl_core
rtk cargo nextest run --profile cold -p rune --test integration -E 'test(dump_peephole_candidates_omits_fully_lowered_result_helpers_patterns) | test(dump_opcode_histogram_includes_function_summary) | test(dump_opcode_histogram_tracks_result_branch_opcode_batch)'
```

fixture の直接実行結果は `.expected` と照合する。最後に `rg` で Surtr コード内に削除対象の定義・呼び出しが残っていないことを確認する。Rust コード内の旧 API 参照も確認する。

## 実施結果（2026-10-03）

- Result 専用の 12 API と各 `@doc` を削除し、`then` の内部依存を `=?` と `next()` へ置換した。
- `lib/tests/result.srt` に旧処理の合成による再現テストを置いた。削除名の拒否テストは追加していない。
- 削除前後の `cargo run -- test --quiet result.srt` が成功した。
- `cargo run -- test --quiet --all` が成功した。初回は File テストの `tmp/sandbox/` が未作成で 5 件失敗したが、ディレクトリが作成された後の再実行では全件成功した。
- fixture の直接実行出力は既存の `.expected` と一致した。dump の peephole 候補がなく、Result 分岐命令と関数 summary が残ることも確認した。
- サブエージェントの最終差分レビューは修正指摘なし。`git diff --check` が成功した。
- 追加依頼により、Xldr の `core_completion_and_sig_prefer_authored_signatures_for_imported_helpers` が期待する API を `Result::then` に置換した。修正前に対象 bucket の失敗を確認した。製品 Rust コードは変更していない。
- 追加修正後の対象 bucket 1 件と REPL core 全 12 件が成功した。fixture を使う Rune の dump 関連 3 件も成功し、`cargo fmt --all -- --check` と `git diff --check` が通った。サブエージェントの追加差分レビューは修正指摘なし。
