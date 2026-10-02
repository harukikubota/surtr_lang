# Pattern Matching

Pattern は、値の照合・分解・束縛を指定する構文です。`match` の arm、`if_let` などの Pattern 引数、束縛の左辺に直接書きます。Pattern 自体を変数へ保存したり、通常の関数引数として渡したりはできません。

## 使う位置を選ぶ

| 書き方 | 成功時 | 不一致・Extractor の失敗時 |
|---|---|---|
| `match value { ... }` | 選択した arm の値を返す | 次の候補・arm へ進む |
| `is_match(value, pattern)` | `True`。変数束縛は禁止 | `False` |
| `if_let(value, pattern, success, failure)` | 成功側で束縛を使い、値を返す | 失敗側を評価する |
| `if_let_then(value, pattern, action)` | 成功側だけ実行する。戻り値は `Unit` | 成功側を実行せず `Unit` を返す |
| `apply_pattern(value, pattern)` | 選んだ値を `Ok` で返す | Error を保持した `Err` を返す |
| `pattern = value` | 分解して束縛する | 失敗し得る Pattern はコンパイルエラー |
| `pattern =? value` | 分解して後続処理へ進む | 現在の失敗返却先へ渡す |
| do 内の `pattern <- value` | carrier の payload を分解して後続処理へ進む | do の carrier に従う |

`match`、`if_let`、`if_let_then`、`is_match` は、Extractor の Error を不一致として扱い、破棄します。`apply_pattern` は元の Error を保持します。型・引数数・束縛などの静的な誤りは、どの位置でもコンパイルエラーです。

表の分岐側の説明は通常の遅延式についてです。`if_let` 系の引数に固定の括弧付き eager 式を書くと、照合前に評価されます。成功側の scope と括弧の違いは後述します。

```surtr
is_match(Ok(1), Ok(_))                         # True
if_let(Ok(41), Ok(n), n + 1, 0)                # 42
if_let_then(Ok("alice"), Ok(name), print(name)) # 成功時だけ表示
```

`if_let` の両側は同じ型を返します。`is_match` では通常の変数束縛と alias を作れないため、値を使いたいときは `if_let` や `match` を選びます。

## 基本の Pattern

| Pattern | 意味 |
|---|---|
| `name`、`name: Type` | 新しい変数へ束縛する |
| `_`、`_discard`、`_: Type` | 値を捨てる。変数を作らない |
| `True`、`False`、`1`、`-1`、`"text"`、`20ms` | Boolean・Int・String・Duration の literal と照合する |
| `^name` | 外側にある値と `Eq` で比較する |
| `pattern @ whole` | 照合した値全体を別名でも束縛する |
| `(left, right)` | Tuple の各要素を子 Pattern で照合する |
| `[]`、`[a, b]`、`[head, ..tail]` | List または String を分解・照合する |
| `Ok(child)`、`Type::Variant(child)` | variant を照合して payload を分解する |
| `User(name, age)` | Record の分解、または Struct の attached Extractor を使う |
| `head(...)` | named Extractor または束縛済みの ExtractorClosure を使う |
| `p1 \| p2` | 左から候補を試す。利用位置に制限がある |
| `_1`〜`_16` | `apply_pattern` 専用の projection |

子 Pattern は入れ子にできます。型注釈は静的な型の一致を検査するもので、runtime の型判定や型変換ではありません。Float literal の直接照合と Unit の `()` Pattern は使えません。Unit 値を受けるときは変数束縛や wildcard を使います。

### 束縛・pin・alias

束縛名は新しい変数を作ります。外側に同名変数があっても、その値との比較にはなりません。`^name` は Pattern を開始する前の scope にある値を参照します。大文字始まりの裸の名前は constructor として読むため、定数との比較には scope に導入済みの名前を `^LIMIT` のように書きます。

```surtr
amount = 10
match (2, 10) {
  (amount, ^amount) => amount, # 外側の 10 と比較し、内側の 2 を返す
  _ => 0,
}
```

`(x, x)` は重複束縛エラーです。alias と子の束縛にも同じ名前を二度使えません。

```surtr
match [1, 2] {
  [head, ..tail] @ whole => (head, tail, whole),
  _ => (0, [], []),
}
```

alias には `pattern @ whole: Type` と型注釈も付けられます。`@` は OR より後に適用されるので、`p1 | p2 @ whole` は OR 全体への alias です。

`_` で始まる名前は wildcard です。`_discard` や `_foo1` も束縛を作りません。数字だけを続けた `_1` などは projection の構文として扱うため、wildcard の代わりには使えません。

## 分解する値の形

### Tuple・List・String

Tuple は要素数と各要素の型を一致させます。`(pattern)` は grouping、`(pattern,)` は拒否されます。

```surtr
(number, text) = (1, "two")
```

List と String では、`[]` が空、`[a, b]` が固定長、`[head, ..tail]` が先頭と残りを表します。List の head は要素型、tail は List です。String の head と tail はともに String で、先頭の Unicode スカラー値を一つ取り出します。String に対する `[]` は空文字列に対応します。

```surtr
match [1, 2, 3] {
  [] => "empty",
  [head, ..tail] => to_string(head),
}
match "日本" {
  [] => "empty",
  [head, ..tail] => head, # "日"
}
```

式位置の `[head, ..tail]` は List 構築です。Pattern 位置の分解とは区別します。`Kernel::uncons(head, tail)` も先頭と残りを分解しますが、保持する失敗 Error まで同一ではありません。たとえば空 List に対する head-tail Pattern は `EmptyList`、直接の `uncons` は `PatternMismatch` を返します。

### Enum・Result・Error

一般の Enum は `Type::Variant(child)`、payload のない variant は `Type::Variant` で照合します。Result の `Ok(...)` / `Err(...)` も、子 Pattern を使って照合できます。

```surtr
match Ok(42) {
  Ok(value) => to_string(value),
  Err(err) => Error::format(err),
}
```

Error の子 Pattern では、具象 error 型名で kind を照合します。Error の payload を constructor の子として分解することはできません。Error 全体を観測するときは alias を使います。

```surtr
match Err(ZeroDivisionError) {
  Ok(value) => value,
  Err(NoneError | ZeroDivisionError @ err: Error) => Error::kind(err),
  Err(err) => Error::format(err),
}
```

`Err(Err(...))` は外側の Error を Result として再分解する形なので拒否されます。内側の Result の失敗を照合する形は `Ok(Err(...))` です。`Result::recover_kind` の `ErrorKind` 引数は専用の型名マーカーであり、Pattern を値として渡す機能ではありません。Error の観測・保持の制約は [Error Handling](./error-handling.md) を参照してください。

### Record・Struct

```surtr
defrecord User(name: String, age: Int)
user = User("Ada", 20)
User(name, age) = user
User(age: selected_age, name: selected_name) = user
```

Record は全 field を位置順、または field 名で分解します。名前指定の順序は自由ですが、照合・束縛は宣言順です。重複・未知・不足 field、位置指定との混在、field 名 shorthand は拒否します。`User(name, age)` は位置指定の束縛です。

Record の外枠は必ず分解できます。子も変数や wildcard など必ず成功する Pattern なら、通常の `=` を使えます。literal や Extractor など失敗し得る子がある場合は、`match` などで結果を扱います。位置指定の子に型注釈を付けるときは `User((name: String), age)` のように括ります。

Struct の `Type(...)` は attached Extractor の `Type::deconstruct(...)` を必要とします。Record の構造的分解とは異なり、一般の Extractor と同じ失敗規則を使います。宣言方法は [Record](./record.md) と [Structs](./structs.md#new-と-deconstruct-の関係) を参照してください。

## `match`・guard・OR・網羅性

`match` は対象値を一度評価し、arm を上から順に試します。Pattern と guard が成功した最初の arm の本文だけを評価します。各 arm の結果型は一致させます。本文に `{ ... }` を書けば、束縛などの複数文を実行できます。

```surtr
match 8 {
  number when number > 0 => {
    next = number + 1
    next
  },
  _ => 0,
}
```

guard は `when` で書き、Boolean を返します。Pattern の束縛は guard と本文だけで使え、別の arm や `match` の外へは公開されません。guard が `False` なら次の arm へ進みます。

### OR Pattern

`p1 | p2` は `match` arm、`if_let`、`if_let_then`、束縛を作らない `is_match` で使えます。子 Pattern 内でも使えます。同じ OR の全候補は、同じ順序で同じ名前・型の変数を束縛する必要があります。`is_match` では、alias を含め、全候補の変数束縛を禁止します。

```surtr
pair = (2, 42)
if_let(pair, (1, x) | (2, x), x, 0) # 42
```

候補は左から試し、最初の成功で停止します。`if_let` 系は全候補が失敗したときだけ失敗側／`Unit` へ進み、網羅性を要求しません。`match` の OR は一つの arm なので、候補成功後の guard は一度だけ評価します。guard が `False` でも、同じ OR の残り候補には戻りません。

`=` / `=?`、do の `<-` / `=?`、`apply_pattern` の Pattern では、束縛数が0でも入れ子の OR を含めて構文エラーです。これらの入力・右辺にある通常の `match` の arm では OR を使えます。

### 網羅性検査の範囲

`match` は網羅性検査を通る必要があります。guard のない arm だけを被覆に数え、OR の各候補も集計します。

| 対象 | 現行の検査 |
|---|---|
| Boolean | `True` / `False` |
| Result・一般 Enum | 外側の全 variant |
| List・String | 空と非空の形 |
| Tuple・Record | 単一 arm のすべての子が catch-all かを再帰判定 |
| その他 | `_` や変数束縛などの catch-all が必要 |

Result／Enum の payload や List／String の子まで、複数 arm を合成して完全に被覆する解析は行いません。子に値の制限がある Pattern や一般の Extractor を使う場合は、残りの入力を受ける arm を明示してください。標準 `Duration` の分解には、子がすべて catch-all なら単一 arm を受理する規則があります。

基本例は [`lib/tests/spec.srt`](../../lib/tests/spec.srt)、OR の例は [`tests/fixtures/script/pass/patterns/`](../../tests/fixtures/script/pass/patterns/)、拒否例は [`tests/fixtures/script/fail/exhaustiveness/`](../../tests/fixtures/script/fail/exhaustiveness/) にあります。

## Extractor を Pattern に使う

named Extractor と ExtractorClosure は、最後の入力へ照合対象を受け取ります。Pattern 側には `head(事前引数..., 成功payloadの子Pattern...)` と書きます。

```surtr
base = 10
shift = *{|amount: Int, value: Int| MatchResult::OK(value + amount + base)}
apply_pattern(3, shift(2, _1: Int)) # Ok(15)
```

成功 payload が単値なら子 Pattern は1個、tuple なら要素数、Unit なら0個または1個です。`check()` のような子の省略は Unit payload の場合だけ許可します。Unit を明示する子には変数、wildcard、projection を使えますが、`check(())` は拒否されます。

named Extractor は通常の call・capture・値化ができません。ExtractorClosure は第一級の値ですが、本体の実行は Pattern 位置に限ります。ExtractorClosure の Pattern head には束縛済みの名前を使い、literal や生成式を直接埋め込みません。MatchResult 自体は一般の変数や引数へ保持できない専用型です。

Result を返す通常関数は、明示的に変換して利用できます。

```surtr
decimal = Extractor::from_result(&Int::parse)
apply_pattern("42", decimal(_1: Int)) # Ok(42)
```

定義・型注釈・推論・変換 API の詳細は [Extractors](./extractors.md) と `:doc Extractor` を参照してください。

### scope と評価順

事前引数、pin、ExtractorClosure head は、Pattern 開始前の外側 scope を参照します。同じ Pattern の先行する子が新しく束縛した名前は参照しません。local が同名 Extractor を shadow している場合は、その local を検査し、別の Extractor を探し直しません。

入力は一回だけ評価します。各 Extractor occurrence に到達すると、事前引数を左から各一回評価し、本体を一回実行します。成功 payload を子の照合・束縛・projection に再利用し、本体を実行し直しません。先行する子が失敗したら、後続の子や未到達の事前引数は評価しません。次の arm にある occurrence は独立した照合です。

## 通常束縛・SafeBind・do

通常の `=` は、変数・wildcard・その組み合わせの Tuple／Record など、必ず成功する Pattern だけに使えます。alias の成功条件は内側の Pattern に従います。literal、pin、List、Enum variant、一般 Extractor など、不一致があり得る Pattern は `=` では拒否されます。

SafeBind `=?` は canonical Result の外側一段を自動分解し、その `Ok` payload を左辺で照合します。Result 以外の右辺では、partial Pattern で値全体を検査する場合だけ受理します。

```surtr
def first(values: List<Int>) -> Result<Int> {
  [head, ..tail] =? values
  Ok(head)
}
```

不一致は現在の失敗返却先へ渡します。通常 callable では Result／Result-effect、Extractor／ExtractorClosure 本文では自身の MatchResult、do 内ではその carrier に従います。内側の callable や do は外側の失敗返却先を借りません。

do の `<-` は carrier の payload を左辺へ渡します。partial Pattern の失敗は Result effect があれば Error を保持し、なければ Alternative の `empty` へ進みます。必要な能力がなければコンパイルエラーです。詳細は [Error Handling](./error-handling.md) と [do](./do.md) を参照してください。

## consumer の呼び出し

`is_match`、`apply_pattern`、`if_let`、`if_let_then` は、第2引数をPatternとして読みます。通常の前置Call、`Kernel::` 修飾、backtick前置Callで同じ文法を使います。`Regex::is_match` は通常のExpr引数Callです。

```surtr
Ok(1) `is_match` Ok(_) | Err(_)
[10, 20] `apply_pattern` [_, _1]
```

中置Callは左辺を第1引数、右辺をPatternとして扱います。後続のExpr演算子はPatternの外側へ戻ります。`if_let` / `if_let_then` は2引数では不足するため中置Callにできません。ORと中置Callの改行規則は通常演算子と同じです。

`p1 | p2 @ whole` はOR全体にaliasを付けます。最外aliasは照合対象全体、子Patternのaliasはその子位置の値を束縛します。同じ階層に `@` を連続させることはできません。

## projection を Result へ返す

`apply_pattern(value, pattern)` は値全体を一度だけ照合します。全照合成功時だけ、projection `_1`〜`_16` を番号順に `Ok` へ返します。番号は1から連続・重複なしにします。0個なら Unit、1個ならその値、複数なら tuple です。`_01` は `_1` と同じ番号です。

```surtr
apply_pattern(3, 3)                                  # Ok(())
apply_pattern(("label", 42), (_2: String, _1: Int))   # Ok((42, "label"))
apply_pattern([1, 2, 3], [_1, .._2])                  # Ok((1, [2, 3]))
apply_pattern([1, 2], [_, _] @ _1: List<Int>)          # Ok([1, 2])
apply_pattern(Ok(3), _1)                             # Ok(Ok(3))
apply_pattern(Ok(3), Ok(_1))                          # Ok(3)
```

Result 入力も自動 unwrap しません。通常の不一致も `Err` になり、Extractor が失敗した場合は元 Error の kind・message・location・cause を保持します。外側の関数から早期 return はしません。

projection はこの Pattern 内専用です。他の照合位置や事前引数の式には使えません。通常の束縛は内部だけで、呼び出し後の scope には公開しません。型注釈は通常位置・tail・alias で静的に検査されます。

## capture・成功側の scope・pipe

4つの Kernel consumer は、Pattern を直接書いた完全 call を capture できます。生成した関数値は通常の変数・引数・戻り値として渡せます。

```surtr
check: (Result<Int> -> Boolean) = &is_match(&1, Ok(_))
pick: (List<Int> -> Result<Int>) = &apply_pattern(&1, [_1, .._])
add_on_ok = &if_let(&1, Ok(x), x + &2, 0)
add_on_ok(Ok(1), 2) # 3
```

変数へ束縛する関数値は、入力・戻り値を含む型が確定している必要があります。上の `check` と `pick` は型注釈で Result の payload 型と List の要素型を指定しています。

Pattern 自体を `&N` で置換すること、Pattern 未指定の bare capture、consumer 自体の一般値参照は禁止です。たとえば `&is_match(&1, &2)` と `&is_match` は拒否されます。Pattern 内の事前式にある capture placeholder は、Pattern 全体の置換とは区別します。

`if_let`／`if_let_then` の成功側は、Pattern が束縛を作る場合、その成功 scope に直接書く式です。束縛を実際に使うかどうかでは規則を切り替えません。上の `add_on_ok` の `&2` は通常の Int を受け取ります。外部の0引数関数で成功側を置き換えて、新しい束縛を注入することはできません。

束縛を作らない Pattern では通常の Lazy 規則を使います。

```surtr
on_ok = &if_let(Ok(1), Ok(_), &1, 0)
on_ok({|| 42}) # 42。仮引数は (-> Int)
```

成功側へ固定の括弧付き eager 式を書くと、照合前の scope で名前解決・評価します。外側に `x` がなければ `if_let(Ok(1), Ok(x), (x + 1), 0)` は拒否されます。`match` の arm 内の括弧は grouping です。capture 仮引数の `(&2)` も通常の仮引数参照として扱います。

成功側で作った closure は通常の lexical capture に従って束縛値を保持できます。Pattern が束縛した関数値も、その scope の通常データとして扱います。

```surtr
[10, 20] |> apply_pattern([_, _1]) # Ok(20)
```

pipe は最外 call の直接の式引数へ入力を渡します。Pattern 位置へ注入したり、Extractor 事前引数内を探索したり、欠けた Pattern を補完したりはしません。pipe の `_1`、Pattern projection の `_1`〜`_16`、capture の `&1`〜`&16` は別の役割です。

`if_let`・`if_let_then`・`is_match`・`apply_pattern` は予約 consumer 名で、同名の変数・関数・user member は宣言できません。`Kernel::is_match` などの修飾形も同じ consumer です。`Regex::is_match` は通常関数であり、第2引数は式です。

詳しい共通規則は [Capture Operator](./capture-operator.md#lazy・pattern・errorkind引数)、[Lazy Evaluation](./lazy-evaluation.md#pattern-bindingと成功branch)、[パイプ演算子](./pipe-operators.md) を参照してください。

## 関連ページと標準 API

- [Extractors](./extractors.md): 定義・ExtractorClosure・`Extractor::from_result`
- [Record](./record.md) / [Structs](./structs.md): 型ごとの構築と分解
- [Error Handling](./error-handling.md) / [do](./do.md): 失敗の保持・伝播
- [型注釈](./type-annotations.md) / [言語リファレンス](./language-reference.md): 型記法と構文の制約
- `:doc Kernel::if_let`、`:doc Kernel::is_match`、`:doc Kernel::apply_pattern`、`:doc Extractor`: REPL で読める API 説明

標準 API の正本は [`lib/kernel.srt`](../../lib/kernel.srt) と [`lib/extractor.srt`](../../lib/extractor.srt) の `@doc`、開発者向けの契約は [Pattern / Extractor 実装契約](../dev/Pattern_spec.md) にあります。
