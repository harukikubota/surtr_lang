# Pattern Matching

Surtr では `match` が中心的な分岐手段です。

## 基本

```text
xldr(1)> print(match False { True => "T", _ => "F", })
F
xldr(2)>
```

値ごとの分岐も同じです。

```surtr
match n {
  1 => "one",
  10 => "ten",
  _ => "other",
}
```

## `Result`

成功と失敗を分けるときは `Ok(...)` / `Err(...)` を match します。

```text
xldr(1)> def parse_bool(text: String) -> Result<Boolean> { match text { "true" => Ok(True), "false" => Ok(False), _ => Err(NoneError), } }
xldr(2)> print(match parse_bool("true") { Ok(flag) => if(flag, "yes", "no"), Err(err) => inspect(err), })
yes
xldr(3)>
```

## list / string の分解

pattern position の `[head, ..tail]` は sequence decomposition として読まれます。  
これは expression position の list construction とは別物です。

## guard と exhaustiveness

OR Pattern `p1 | p2` は `match` arm と、変数を束縛しない `is_match` で使えます。子 Pattern に入れ子にすることもできます。`is_match` はすべての alternative で変数束縛を禁止します。`=` / `=?`、do binding、`if_let` / `if_let_then` の Pattern では、入れ子の OR も構文エラーになります。`if_let` は alternative 間で束縛変数が一致していても OR を許可しません。これらの input / RHS にある通常の `match` では OR を使えます。

- `match` は網羅性が必要
- guard があっても、全体として取りこぼしがあると compile error
- `Boolean`, `Result`, enum では特に exhaustiveness が重要

具体例は `../../tests/fixtures/script/pass/control/` と `../../tests/fixtures/script/fail/exhaustiveness/` が参考になります。

## 関連ページ

- struct pattern と `deconstruct` は `./structs.md`
- extractor の形は `./extractors.md`
- `Kernel::uncons` は `./kernel.md`
- 制約一覧は `./language-reference.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- guard は便利ですが、guard 式自体は `Boolean` でなければなりません。
- `Result` や enum の `match` は「だいたい合っていそう」では通らず、missing case があると exhaustiveness error になります。
