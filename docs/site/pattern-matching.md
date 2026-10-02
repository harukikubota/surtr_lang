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

## Record の分解

```surtr
defrecord User(name: String, age: Int)
user = User("Ada", 20)
User(name, age) = user
User(age: selected_age, name: selected_name) = user
```

Record は宣言された全 field を位置順、または field 名で分解できます。名前指定の順序は自由ですが、子の照合順は宣言順です。重複・未知・不足 field、位置指定との混在を拒否します。`User(name, age)` は位置指定の変数束縛であり、field 名 shorthand ではありません。Record 自体は必ず分解できますが、literal や Extractor など失敗し得る子を置いた Pattern は通常の `=` に使えません。

Record head の `name: child` は名前付き field として解釈します。位置指定の子に型注釈を付けるときは `User((name: String), age)` のように括ります。

## guard と exhaustiveness

OR Pattern `p1 | p2` は `match` arm、`if_let`、`if_let_then`、変数を束縛しない `is_match` で使えます。子 Pattern に入れ子にすることもできます。`match` / `if_let` / `if_let_then` では、同じ OR の全候補が同じ順序で同じ名前・型の変数を束縛する必要があります。`is_match` はすべての候補で変数束縛を禁止します。`=` / `=?`、do binding、`apply_pattern` の Pattern では、束縛数が 0 でも入れ子の OR を含めて構文エラーになります。これらの input / RHS にある通常の `match` では OR を使えます。

```surtr
pair = (2, 42)
print(to_string(if_let(pair, (1, x) | (2, x), x, 0)))
```

`if_let` / `if_let_then` は左から最初に成功した候補の束縛を成功側で使い、全候補が失敗したときだけ else 側 / `Unit` に進みます。`match` の網羅性は要求しません。`match` では OR 全体が一つの arm なので、候補が成功した後の guard は一度だけ評価され、guard が `False` なら同じ OR の残り候補ではなく次の arm へ進みます。

- `match` は網羅性が必要
- guard があっても、全体として取りこぼしがあると compile error
- `Boolean`, `Result`, enum では特に exhaustiveness が重要

具体例は `../../tests/fixtures/script/pass/control/` と `../../tests/fixtures/script/fail/exhaustiveness/` が参考になります。

## consumer の呼び出し

`is_match`、`apply_pattern`、`if_let`、`if_let_then` は、第2引数をPatternとして読みます。通常の前置Call、`Kernel::` 修飾、backtick前置Callで同じ文法を使います。`Regex::is_match` は通常のExpr引数Callです。

```surtr
Ok(1) `is_match` Ok(_) | Err(_)
[10, 20] `apply_pattern` [_, _1]
```

中置Callは左辺を第1引数、右辺をPatternとして扱います。後続のExpr演算子はPatternの外側へ戻ります。`if_let` / `if_let_then` は2引数では不足するため中置Callにできません。ORと中置Callの改行規則は通常演算子と同じです。

`p1 | p2 @ whole` はOR全体にaliasを付けます。最外aliasは照合対象全体、子Patternのaliasはその子位置の値を束縛します。同じ階層に `@` を連続させることはできません。

## projection を Result へ返す

`apply_pattern(value, pattern)` は値全体を一度だけ照合し、成功した projection を `Ok`、失敗を `Err` にします。`_1`〜`_16` は1から連続・重複なしで指定し、番号順に返します。0個なら Unit、1個ならその値、複数なら tuple になります。

```surtr
apply_pattern(("label", 42), (_2: String, _1: Int))
# Ok((42, "label"))
apply_pattern(Ok(3), Ok(_1))
# Ok(3)
```

list tail や `pattern @ _1: Type` の alias にも projection を書けます。型注釈は静的検査の対象です。Pattern 内の通常 binding は外側 scope に公開されず、事前引数・pin は Pattern 開始前の外側 scope を参照します。

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
