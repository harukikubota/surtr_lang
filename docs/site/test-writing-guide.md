# テストの書き方ガイド

Surtr のテストでは、値をそのままアサーションに渡す書き方と、必要な値を取り出してから検証する書き方を使い分けます。このページでは、通常束縛 `=`、SafeBind `=?`、Pattern、Extractor、Facet の選び方と、実行できても検証の意図を損なうアンチパターンを紹介します。

宣言方法、アサーション一覧、実行オプションは[テストを書く](./test.md)を参照してください。

## 検証したいものから書き方を選ぶ

表の Result の例では、成功値の型を Int としています。

| 検証したいもの | 基本の書き方 | 選ぶ理由 |
|---|---|---|
| 値全体、Result / Option の枝と中身 | `actual: Result<Int> = operation()` → 専用アサーション | 元の値を保持したまま検証できる |
| 成功値についての追加条件 | `value: Int =? operation()` → アサーション | 成功値を使え、不一致や失敗はケースへ伝わる |
| 期待している失敗 | `actual: Result<Int> = operation()` → エラー用アサーション | 期待する Err を検証前に伝播しない |
| Error の保存 Payload | `err: Error =? expect_error(actual)` → `assert_error_matches` | 失敗側を検証用の値として取り出せる |
| struct / record / tuple の一部 | field / tuple path | 分解のための分岐を書かず、対象を直接読める |
| List の先頭と残り、HashMap の必要なキー | Pattern と `=?` | 必要な構造を要求し、取り出した値を検証できる |
| 深い field、Enum の枝、index / slice / key | Facet と必要に応じて `=?` | 対象への経路を一つの式で表せる |
| 繰り返し使う独自の分解 | Extractor と `=?` | ケース本文に検証したい条件を残せる |
| 分岐ごとの振る舞い、Extractor の不一致 | `match` | 成功と不一致をそれぞれ検証できる |

SafeBind をすべてに使う必要はありません。`Ok(3)` を期待するだけなら `assert_ok_eq(3, actual)` で十分です。

## サンプルの実行

ファイル名を指定した例は、その名前と配置で保存してください。それ以外の `surtr` コードブロックは独立したテストファイルです。`tests/example.srt` に保存して実行します。`it` の本文は `Result<()>` を返し、アサーションの失敗がその戻り値に反映されます。アンチパターンの例には、意図どおり検証できない理由と実際の結果を併記しています。

```sh
surtr test tests/example.srt
```

## 1. 元の値を保持して検証する

Result や Option の枝と値は、通常束縛 `=` と専用アサーションで検証します。

```surtr
import Test;

it("checks the result and option as returned") {
  actual: Result<Int> = Int::parse("42")
  optional: Option<Int> = Option::Some(7)
  absent: Option<Int> = Option::None
  assert_ok_eq(42, actual)?
  assert_some_eq(7, optional)?
  assert_none(absent)
}
```

`=` は Result や Option を分解しません。`assert_ok` / `assert_err` / `assert_some` / `assert_none` は枝を、`assert_ok_eq` / `assert_some_eq` は枝と中身を検証します。

Either や独自 Enum でも、枝に意味があれば値全体を保持します。枝ごとに検証が異なるなら `match` を使います。

```surtr
import Test;

defenum Reply { Accepted(Int), Rejected(String) }

it("checks the accepted reply") {
  actual: Reply = Reply::Accepted(42)
  match actual {
    Reply::Accepted(value: Int) => assert_eq(42, value),
    Reply::Rejected(reason: String) => fail_test("unexpected rejection: #{reason}"),
  }
}
```

### アンチパターン: 専用アサーションで済む照合を手書きする

次の例は正しく検証できますが、枝の照合と失敗メッセージをケースごとに書いています。

```surtr
import Test;

it("checks a parsed value with a manual branch") {
  actual: Result<Int> = Int::parse("42")
  match actual {
    Ok(value: Int) => assert_eq(42, value),
    Err(_) => fail_test("expected Ok"),
  }
}
```

`assert_ok_eq(42, actual)` で同じ検証になります。枝ごとに検証内容が異なる `match` まで置き換える必要はありません。

## 2. 成功値を取り出して追加の条件を検証する

成功値に複数の条件を課すなら SafeBind で取り出します。`actual: Result<Int> = ...` は結果全体を保持し、`value: Int =? ...` は成功値を束縛します。SafeBind の左辺の型注釈は、取り出した後の値の型です。

```surtr
import Test;

it("checks the extracted values") {
  parsed: Int =? Int::parse("42")
  optional: Int =? Option::Some(7)
  either: Either<Error, Int> = Either::Right(8)
  right: Int =? either
  assert_gt(parsed, 0)?
  assert_lt(parsed, 100)?
  assert_eq(7, optional)?
  assert_eq(8, right)
}
```

`=?` は、右辺が MonadExtractor を実装する型なら成功値を一段取り出し、左辺の Pattern に照合します。標準では Result、Option、`Either<Error, T>` が対応し、独自型も MonadExtractor を実装すれば使えます。

Result の Err、Option の None、`Either<Error, T>` の Left はケースの失敗になります。None は `NoneError`、Err と Left は保持している Error を伝えます。

### 一段取り出すことと、値全体を照合することを区別する

`=?` は入れ子の値を再帰的に取り出しません。

```surtr
import Test;

it("keeps the inner result for inspection") {
  actual: Option<Result<Int>> = Option::Some(Err(NoneError()))
  inner: Result<Int> =? actual
  assert_err_kind(NoneError, inner)
}
```

Some を外し、中の Err を検証対象として保持します。

元のバリアントを Pattern で照合したい場合は、`Ok` で包むと、SafeBind が取り出した後も元の値が残ります。

```surtr
import Test;

it("matches the original variant") {
  actual: Either<Error, Int> = Either::Left(NoneError())
  Either::Left(err: Error) =? Ok(actual)
  assert_error_matches(err, NoneError, Ok(()))
}
```

これは元の型全体に Pattern を適用する場合の書き方です。Error を取り出すだけなら、次節の `expect_error` が意図を直接表します。

## 3. 期待する失敗を検証する

失敗を期待するテストでは、対象の Result を `=` で保持します。

```surtr
import Test;

it("checks a missing list element") {
  actual: Result<Int> = List::at([10], 3)
  assert_err_kind(ListIndexOutOfBounds, actual)
}
```

種類には `assert_err_kind`、メッセージには `assert_err_message_eq`、原因の種類と順序には `assert_cause_chain` を使います。ErrorKind には `ListIndexOutOfBounds` のように宣言名を渡します。

### アンチパターン: 期待する失敗を先に伝播する

次のケースは、List が期待どおり範囲外エラーを返しても失敗します。`assert_eq` には到達しません。

```surtr
import Test;

it("incorrectly propagates the expected error") {
  value: Int =? List::at([10], 3)
  assert_eq(10, value)
}
```

`Result<()>` などに文末の `?` を付けて期待する Err を伝播する場合も同様です。検証対象の失敗と、アサーションの失敗を区別してください。

### Error 自体と保存 Payload を検証する

`expect_error` は、入力の失敗側にある Error を `Ok(error)` として返します。期待に反して成功していた場合は `TestExpectedError` を返すため、`=?` でケースを失敗にできます。

```surtr
import Test;

it("checks the error payload") {
  actual: Result<Int> = List::at([10], 3)
  err: Error =? expect_error(actual)
  assert_error_matches(err, ListIndexOutOfBounds((index: Int), (length: Int)) @ e: ListIndexOutOfBounds) {
    assert_eq(3, index)?
    assert_eq(1, length)?
    assert_eq(index, e.index)
  }
}
```

`assert_error_matches` は Error Pattern に一致したときだけ本文を実行します。`index` と `length` は保存 Payload、`e` は種類が確定した Error の別名です。共通情報は `Error::kind(err)` / `Error::message(err)` などでも読めます。

`expect_error` は Option、`Either<Error, T>`、独自の MonadExtractor 実装型にも使えます。一般用途の `MonadExtractor::extract_error` と異なり、成功入力だった場合の診断に元の入力全体を表示します。

### 入れ子のどの失敗を調べるかを明示する

`expect_error` も抽出するのは一段です。`Ok(Err(error))` をそのまま渡すと、外側が成功なので `TestExpectedError` になります。内側の失敗を検証するなら、明示的に畳み込みます。

```surtr
import Test;

it("checks an inner failure") {
  nested: Result<Result<Int>> = Ok(Err(NoneError()))
  actual: Result<Int> = Result::flatten(nested)
  err: Error =? expect_error(actual)
  assert_error_matches(err, NoneError, Ok(()))
}
```

外側が Ok であること自体も検証するなら、`inner: Result<Int> =? nested` で外側の成功を要求し、`assert_err_kind(NoneError, inner)` で内側を検証します。`flatten` は外側と内側のどちらの Err も渡すため、両者の区別が必要な検証には使いません。

## 4. 必要な構造へ直接進む

### struct / record / tuple と Facet

field は `value.field`、tuple は `value._0` で読めます。途中の Result や Enum の枝をたどるなら Facet で経路を表せます。

```surtr
import Test;

defrecord Profile(name: String, score: Int)
defstruct Account { profile: Result<Profile> }

impl Account {
  def new(profile: Result<Profile>) -> Self {
    Account { profile: profile }
  }
}

it("checks fields through a result") {
  actual: Account = Account::new(Ok(Profile("alice", 42)))
  score: Int =? actual.profile.score
  pair: (String, Int) = ("ready", 7)
  assert_eq(42, score)?
  assert_eq("ready", pair._0)?
  assert_eq(7, pair._1)
}
```

Result を含む経路の読み取り結果は Result になります。`=?` で成功値を使うか、`assert_ok_eq` へそのまま渡せます。Enum の特定の枝への経路も同様です。

```surtr
import Test;

defenum Reply { Accepted(Int), Rejected(String) }

it("reads the accepted payload") {
  actual: Reply = Reply::Accepted(42)
  assert_ok_eq(42, Facet::view(Reply.Accepted, actual))
}
```

アンチパターンは、深い field に到達するためだけに Result や Enum の `match` を入れ子にし、各段に同じ失敗用の枝を書くことです。経路が前提なら Facet で表し、途中の枝やエラー自体を検証したいなら、その段の値を保持します。

### Enum: 片方の枝なら SafeBind、両方の枝なら match

Enum の特定の枝だけを期待するなら、SafeBind の左辺にその Pattern を書きます。複数の枝を受け入れて枝ごとに検証するなら `match` を使います。ここでは `Either<String, Int>` を例に、二つの書き方を比べます。

Right だけを期待するケースは次のように書けます。

```surtr
import Test;

it("requires the Right branch") {
  actual: Either<String, Int> = Either::Right(42)
  Either::Right(value: Int) =? actual
  assert_eq(42, value)
}
```

この型には標準の MonadExtractor 実装がないため、`actual` 全体が Pattern に照合されます。Right なら `value: Int` を束縛し、Left なら Pattern の不一致でケースが失敗します。Left だけを期待する場合も、`Either::Left(value: String) =? actual` と書けます。`Either<Error, Int>` の成功値を自動で取り出す書き方とは区別してください。

両方の枝を受け入れ、枝ごとに異なる内容を検証するなら `match` を使います。次の例では、Right の数値と Left の文字列をそれぞれ検証します。

```surtr
import Test;

def check_reply(actual: Either<String, Int>) -> Result<()> {
  match actual {
    Either::Right(value: Int) => assert_eq(42, value),
    Either::Left(message: String) => assert_eq("cached", message),
  }
}

it("checks the Right value") {
  actual: Either<String, Int> = Either::Right(42)
  check_reply(actual)
}

it("checks the Left value") {
  actual: Either<String, Int> = Either::Left("cached")
  check_reply(actual)
}
```

この `check_reply` は、どちらの枝でもその検証に成功すれば成功します。特定の枝であることを要求するテストには、先ほどの SafeBind を使います。

### List: 構造か位置かで選ぶ

先頭と残りなら head/tail Pattern、特定の位置や区間なら index / slice Facet を使います。

```surtr
import Test;

it("checks list structure and a slice") {
  actual: List<Int> = [10, 20, 30]
  [head: Int, ..tail: List<Int>] =? actual
  assert_eq(10, head)?
  assert_eq([20, 30], tail)?
  assert_ok_eq(30, actual.[-1])?
  assert_ok_eq([20, 30], actual.[1..2])
}
```

`[head: Int, ..tail: List<Int>]` は空ならケースを失敗にします。index の負数は末尾基準、slice は終点を含みます。

空や範囲外自体が検証対象なら、取り出しを強制せず、`List::at` や Facet の戻り値を保持してエラーを検証します。範囲外は `List::at` なら `ListIndexOutOfBounds`、Facet なら `FacetListIndexOutOfBounds` です。

### HashMap: 必要なキーをまとめて取り出す

```surtr
import Test;

it("checks required map entries") {
  actual: HashMap<Int> = hash!["read" => 80, "write" => 70, "extra" => 1]
  hash!["read" => read: Int, "write" => write: Int] =? actual
  assert_eq(80, read)?
  assert_eq(70, write)?
  assert_ok_eq(80, actual.["read"])
}
```

`hash!` Pattern は指定キーの存在と値を照合し、余分なキーがあっても一致します。Map 全体の等価性の代わりにはならないため、全体の一致は `assert_eq(expected, actual)` で検証します。

`hash![]` はすべての Map に一致するので、空の検証には使えません。空なら `assert_eq(0, HashMap::map_len(actual))` などを使います。子の値も自動では分解されません。`hash!["a" => value: Result<Int>]` の `value` は Result のままで、Ok の中身が必要なら `hash!["a" => Ok(value: Int)]` と明示します。

キーごとの取得結果を `match` して同じ失敗用の枝を繰り返す代わりに、Pattern か key Facet でまとめられます。欠落の検証では、呼び出している API のエラーを区別します。

```surtr
import Test;

it("distinguishes map lookup errors") {
  actual: HashMap<Int> = hash!["read" => 80]
  assert_err_kind(HashMapKeyMissing, HashMap::map_get(actual, "missing"))?
  assert_err_kind(FacetKeyNotFound, actual.["missing"])
}
```

## 5. 独自の分解を Extractor にまとめる

複数のケースで使う分解は、共通モジュールに `defextractor` で定義します。ここでは List の先頭二要素を取り出す `first_pair` を用意します。

まず、`tests/support/checks.srt` にモジュールを保存します。

```surtr
defmod Checks {
  defextractor first_pair(values: List<Int>) -> MatchResult<(Int, Int)> {
    [first: Int, ..[second: Int, .._]] =? values
    MatchResult::Ok((first, second))
  }
}
```

次に、`tests/example.srt` でファイルを `include` し、`import` で Extractor を取り込みます。`include` の相対パスは、このテストファイルからの位置です。

```surtr
include "./support/checks.srt"
import Test;
import Checks::first_pair;

it("checks values obtained by a helper") {
  actual: List<Int> = [10, 20, 30]
  first_pair(first: Int, second: Int) =? actual
  assert_eq(10, first)?
  assert_eq(20, second)
}
```

`include` はモジュールファイルを読み込み、`import Checks::first_pair` はそのメンバーを短名で使えるようにします。`surtr test tests/example.srt` で実行します。

ケース内で一度だけ使う分解なら、[ExtractorClosure](./extractors.md#extractorclosure) で足ります。

`MatchResult::Ok(payload)` は取り出した値を子 Pattern へ渡し、`MatchResult::Err(error)` は抽出の失敗です。SafeBind でヘルパーを使うと不一致がケースの失敗になり、本文には値の検証だけを書けます。

Result などを右辺にすると、MonadExtractor が取り出した成功値にこの Extractor が適用されます。元の型全体を受け取る独自の Extractor なら、`helper(value: Int) =? Ok(actual)` のように適用します。

### アンチパターン: ヘルパーの不一致を成功へ変える

次の例は、必要な先頭要素がないのに成功します。

```surtr
import Test;

it("incorrectly accepts a missing head") {
  actual: List<Int> = []
  match actual {
    [head: Int, .._] => assert_eq(10, head),
    _ => Ok(()),
  }
}
```

先頭が必要なら `[head: Int, .._] =? actual` と書き、不一致をケースの失敗にします。検証したい Error を Extractor の成功値として取り出すなら `MatchResult::Ok(error)` を返します。`MatchResult::Err(error)` にすると、検証の前にケースが失敗します。

### Extractor 自体のテストでは match を使う

Extractor の成功と不一致が検証対象なら、`match` で適用します。次の `tests/checks_test.srt` は、`tests/support/checks.srt` を読み込み、`first_pair` の成功と不一致を検証します。

```surtr
include "./support/checks.srt"
import Test;
import Checks::first_pair;

it("extracts the first two elements") {
  actual: List<Int> = [10, 20, 30]
  match actual {
    first_pair(first: Int, second: Int) => {
      assert_eq(10, first)?
      assert_eq(20, second)
    },
    _ => fail_test("expected two elements"),
  }
}

it("rejects a list with only one element") {
  actual: List<Int> = [10]
  match actual {
    first_pair(_, _) => fail_test("expected no match"),
    _ => Ok(()),
  }
}
```

`surtr test tests/checks_test.srt` で実行します。ここでの最後の `Ok(())` は、期待した不一致を確認した結果であり、取り出しの失敗を成功へ変える例とは目的が異なります。

`match` は Extractor の不一致の Error を捨てます。その種類や Payload も検証したいなら、[apply_pattern](./extractors.md#値として結果を受け取る) で照合結果を Result として受け取り、エラー用アサーションへ渡します。

## 6. 複数の検証をケースの結果へつなぐ

通常の block では、途中のアサーションに文末の `?` を付け、最後のアサーションを返します。

```surtr
import Test;

it("checks two fields") {
  actual: (Int, Int) = (10, 20)
  assert_eq(10, actual._0)?
  assert_eq(20, actual._1)
}
```

複数の検証を一つにまとめるなら `do` も使えます。アサーションを式として並べると、途中の失敗がケースの失敗になります。

```surtr
import Test;

it("groups checks with do") {
  do {
    actual: (Int, Int) = (10, 20)
    assert_eq(10, actual._0)?
    assert_eq(20, actual._1)
  }
}
```

### アンチパターン: アサーションの Result を明示的に捨てる

次のケースは、最初の二つのアサーションが失敗しても成功します。

```surtr
import Test;

it("incorrectly discards assertion failures") {
  do {
    assert_eq(10, 99);
    _ = assert_eq(20, 99)
    assert_eq(30, 30)
  }
}
```

`;` と `_ =` は値の明示破棄で、`do` の中でも失敗を伝播しません。Result を返す準備処理も、成功が後続の検証の前提なら失敗を伝播させます。

## 7. 表示と入出力を検証する

StyledDoc の文字だけなら `assert_doc_plain_eq`、ANSI の装飾を含むなら `assert_doc_ansi_eq` を使います。

```surtr
import Test;

it("checks plain and ANSI output") {
  doc: StyledDocDoc = StyledDoc::bold(StyledDoc::text("hello"))
  assert_doc_plain_eq("hello", doc)?
  assert_doc_ansi_eq("\u{1b}[1mhello\u{1b}[0m", doc)
}
```

標準入出力には Test のケース単位の入出力を使います。

```surtr
import Test;

it("reads input and checks output") {
  push_stdin("alice\n")?
  name: String =? IO::get_line("")
  print("hello #{name}")
  assert_stdout_eq(["hello alice"])?
  assert_stderr_eq([])
}
```

出力全体の比較には `assert_stdout_eq` / `assert_stderr_eq`、部分的な条件には `capture_stdout` / `capture_stderr` を使います。

アンチパターンは、`capture_stdout()` で取り出した後に、同じ出力を `assert_stdout_eq` でも検証しようとすることです。どちらも読み取った出力を取り除くため、複数の条件を付けるなら、取り出した値を保存して検証します。入力と出力はケースごとに分かれ、読み残しは次のケースへ持ち越されません。

## 関連ページ

- [テストを書く](./test.md): Test API、実行、ケースの選択、診断
- [パターンマッチ](./pattern-matching.md): 構造の照合と束縛
- [Extractor](./extractors.md): named Extractor、ExtractorClosure、apply_pattern
- [Facet](./facet.md): field・枝・index・key をたどる経路
- [Monad](./monad.md#safebind): SafeBind と MonadExtractor の契約
- [エラーハンドリング](./error-handling.md): Error、Payload、原因の扱い
