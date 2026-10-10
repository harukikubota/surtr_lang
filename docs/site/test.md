# テストを書く

`Test` は、値やエラーが期待どおりかを確かめる標準モジュールです。
テストファイルに `import Test;` を書き、`surtr test` で実行します。
このページの例はテストファイル用です。通常の REPL では `Test` を読み込めません。

## 最初のテスト

`test` でテストをまとめ、`it` に一件ずつ条件を書きます。
必要なら `describe` を使って関連するケースをまとめられます。

```surtr
import Test;

test("addition") {
  describe("positive numbers") {
    it("adds two values") {
      assert_eq(3, 1 + 2)
    }
  }
}
```

`assert_eq(expected, actual)` は、期待値と実際の値が等しいかを検査します。
各アサーションは成功なら `Ok(())`、失敗なら `Err` を返します。
`it` の本文も `Result<()>` を返すため、最後に検査するアサーションをそのまま書けます。

上の例を `tests/addition.srt` に保存し、次のコマンドで実行します。

```sh
surtr test tests/addition.srt
```

## 複数の条件を確かめる

途中のアサーションには文末の `?` を付け、最後のアサーションはそのまま返します。
`?` は `Err` ならその場で早期 return し、成功なら次の処理へ進みます。
文末の `?` は、成功値の終端が Unit の Result に使います。
アサーションの `Result<()>` はこの条件を満たします。
`?` を付けた文は Unit になるため、`Result<()>` を返す `it` の末尾に置くと型エラーになります。

```surtr
import Test;

test("bounds") {
  it("checks a value") {
    value = 3
    assert(value != 0, "value must be nonzero")?
    assert_gte(value, 0)?
    assert_lt(value, 10)
  }
}
```

最初の失敗でそのケースの後続処理を止め、次の `it` へ進みます。
値を取り出して使う場合は `value =? operation()` と書きます。
詳しくは[エラーハンドリング](./error-handling.md)を参照してください。

`do` を使う場合は、アサーションを改行で並べることもできます。

```surtr
import Test;

test("bounds with do") {
  it("checks a value") {
    do {
      value = 3
      assert_gte(value, 0)
      assert_lt(value, 10)
    }
  }
}
```

途中のアサーションの結果を捨てると、失敗を見逃します。
`assert_eq(...);` のように `;` を付ける書き方は、`do` 内でも結果を捨てます。

## 検査したい内容から選ぶ

### 値・条件・数値

| 確かめたいこと | アサーション |
|---|---|
| 期待値と等しい | `assert_eq(expected, actual)` |
| 二つの値が異なる | `assert_ne(expected, actual)` |
| True または False である | `assert_true(flag)` / `assert_false(flag)` |
| 条件を満たし、失敗時に説明を出す | `assert(flag, message)` |
| 値が指定した条件を満たす | `assert_satisfies(actual, message, predicate)` |
| 小さい・以下・大きい・以上である | `assert_lt(lhs, rhs)` / `assert_lte(lhs, rhs)` / `assert_gt(lhs, rhs)` / `assert_gte(lhs, rhs)` |
| Float の差が許容誤差以内である | `assert_approx(expected, actual, tolerance)` |
| この分岐に到達したら失敗にする | `fail(detail)` |

等価性の検査には同じ型の二値と `Eq`、大小の検査には同じ型の二値と `Compare` が必要です。
大小の検査が失敗すると、左右の値と期待する関係を表示します。

`assert` は指定した説明をそのまま失敗メッセージにします。空文字列も使えます。
`assert_satisfies` は、説明に加えて検査した値も表示するため、独自の条件に使えます。
条件の関数は一度だけ呼ばれます。

```surtr
import Test;

test("custom condition") {
  it("accepts a positive number") {
    assert_satisfies(4, "expected a positive number", {|n| n > 0})
  }
}
```

`assert_approx` の許容誤差は非負の Float で指定します。差が許容誤差ちょうどなら成功です。
Int から Float への暗黙変換はありません。負の許容誤差や、差が有限 Float に収まらない場合は失敗します。

### 文字列

| 確かめたいこと | アサーション |
|---|---|
| 文字列全体が等しい | `assert_eq(expected, actual)` |
| 指定した文字列を含む | `assert_contains(fragment, actual)` |
| 指定した文字列で始まる | `assert_starts_with(prefix, actual)` |
| 指定した文字列で終わる | `assert_ends_with(suffix, actual)` |

部分一致・前方一致・後方一致は大文字小文字を区別し、正規表現として解釈しません。
空の prefix や suffix は一致します。失敗時には期待した文字列と実際の文字列を表示します。

### Result と Option

| 確かめたいこと | アサーション |
|---|---|
| Ok である | `assert_ok(result)` |
| Ok の値が期待値と等しい | `assert_ok_eq(expected, result)` |
| Err である | `assert_err(result)` |
| Some である | `assert_some(option)` |
| Some の値が期待値と等しい | `assert_some_eq(expected, option)` |
| None である | `assert_none(option)` |

中身を比べる `assert_ok_eq` と `assert_some_eq` は `Eq` を要求します。
枝だけを確かめる `assert_ok`・`assert_err`・`assert_some`・`assert_none` は、中身に `Eq` がなくても使えます。
たとえば、関数を含む Option にも `assert_some` を使えます。

### エラーの種類・メッセージ・原因

| 確かめたいこと | アサーション |
|---|---|
| エラーの種類 | `assert_err_kind(marker, result)` |
| メッセージの完全一致 | `assert_err_message_eq(expected, result)` |
| エラー全体の表示に含まれる文字列 | `assert_err_contains(fragment, result)` |
| 原因となったエラーの種類と順序 | `assert_cause_chain(expected, result)` |

失敗すること自体を確かめるときは、検査対象の Result を `=` で保存してアサーションへ渡します。
検査対象に `?` や `=?` を付けると、期待している Err を検査する前に return してしまいます。

```surtr
import Test;

deferror InvalidCount(count: Int) { "count must be positive: #{count}" }

test("expected failure") {
  it("checks the kind and message") {
    result: Result<Int> = Err(InvalidCount(0))
    assert_err_kind(InvalidCount, result)?
    assert_err_message_eq("count must be positive: 0", result)
  }
}
```

`assert_err_kind` には `deferror` で宣言した名前を渡します。
引数を持つエラーでも `InvalidCount(0)` ではなく `InvalidCount` と書きます。

`assert_err_message_eq` はメッセージだけを比較します。種類・原因・発生位置は含めません。
エラー全体の表示を部分一致で調べたい場合は `assert_err_contains` を使います。
いずれも、検査対象が Ok なら失敗します。

原因も調べる場合は、外側から順にエラーの宣言名を並べます。

```surtr
import Test;

test("cause chain") {
  it("keeps the original cause") {
    result = Result::cause(Err(NoneError), ZeroDivisionError)
    assert_cause_chain([ZeroDivisionError, NoneError], result)
  }
}
```

`assert_cause_chain` は種類・順序・長さを検査し、同じ種類の繰り返しも区別します。
値・メッセージ・発生位置は比較しません。期待列には宣言名を直接書き、変数・文字列・
エラーを生成する式・spread は使いません。空の列は一致せず、Ok も失敗です。
`assert_err_kind(OuterError, result)` は原因の有無を問いませんが、
`assert_cause_chain([OuterError], result)` は原因がないことも確かめます。

## 出力や入力を確かめる

`assert_stdout_eq(expected)` と `assert_stderr_eq(expected)` には、期待する出力を文字列の List で渡します。
`capture_stdout()` と `capture_stderr()` で出力を取り出してから、別のアサーションで調べることもできます。
出力を読み取ると、それまでの内容は取り除かれます。

`push_stdin(text)` はテスト用の入力を追加します。
入力と出力はケースごとに分かれており、読み残しは次の `it` に持ち越されません。
ケースの入力は空の状態から始まります。入力を追加する前と、すべて読み終えた後は、`IO::get` と `IO::get_line` が `Err(InputError(...))` を返します。端末の入力は待ちません。

```surtr
import Test;
import IO;

test("IO") {
  it("reads each line in order") {
    push_stdin("alpha\nbeta\n")?
    assert_ok_eq("alpha", IO::get_line(""))?
    assert_ok_eq("beta", IO::get_line(""))
  }
}
```

StyledDoc の内容を調べるには、装飾なしの `assert_doc_plain_eq(expected, doc)` と、
ANSI エスケープを含む `assert_doc_ansi_eq(expected, doc)` を使います。

## 失敗した場所を読む

失敗すると、アサーションの式とファイル名・行・列を表示します。
`?` や `do` を使った場合も、失敗したアサーションの位置を確認できます。
`assert_eq` を直接呼んだ場合は、期待値に LHS、実際の値に RHS のラベルが付きます。

## ケースを選ぶ・一時停止する

```sh
surtr test tests/parser.srt --test Parser --describe expression --it nested
surtr test tests/parser.srt --include-xit --deny-pending
surtr test tests/parser.srt --list --timings --format json
```

`--test` と `--describe` は所属するグループ名、`--it` はケース名で絞り込みます。
大文字小文字を区別する部分一致です。種類の異なる条件はすべて満たす必要があり、
同じ種類のグループが入れ子になっている場合は、どれか一つに一致すれば選ばれます。
`--it=nested` とも書けます。フィルターを指定して一件も一致しなければ失敗です。

```surtr
import Test;

test("Parser") {
  xit("nested expression", "ネスト処理を修正中") {
    assert_eq(3, 1 + 2)
  }
  pend("error position", "診断機能の実装待ち")
}
```

`xit` は本文のある一時停止、`pend` は本文のない未実装項目です。
どちらも空白だけではない理由が必要です。`--include-xit` は選ばれた xit を実行し、
`--deny-pending` は選ばれた pend があればコマンドを失敗終了させます。一覧でも同じ条件で判定します。
`it`・`xit`・`pend` をケース本文の中に宣言することはできません。

`--list` はケース本文を実行せず、種類・所属グループ・名前・理由・実行予定を表示します。
一覧表示や絞り込みでも、トップレベルと `test`・`describe` に直接書いた処理は実行します。
停止中や選択外のケースも型検査するため、その中の型エラーは実行前に報告されます。

## 実行結果と CLI オプション

全ファイルのコンパイルを終えてから、テストと各ケースを直列に実行します。コンパイルに失敗したファイルがあっても残りのファイルを確認し、失敗した各ファイルで最初に見つかったエラー1件を報告します。読み取り・コンパイルに失敗したファイルがある場合は、テストを実行せず終了します。

成功したコンパイル結果はキャッシュに保存します。修正後の再実行では、変更されていないファイルと依存の有効なキャッシュを使い、再コンパイルを省きます。対象一覧と入力内容は変更の検出のために確認します。

結果は PASS / FAIL / SKIP / PENDING / FILTERED で表示します。
`--quiet` は成功・停止・許容された未実装項目の詳細と成功サマリーを省きます。
失敗と拒否された pend は省きません。一覧表示では quiet でも選択項目を表示します。

端末では Cargo と同じ形式で、標準環境の準備中は `Preparing`、ファイルのコンパイル中は `Compiling`、実行中は `Running` を一時表示します。ファイル番号・総数・パスから処理中の場所を確認できます。`--quiet` でも表示し、完了時に消去します。進捗は stderr に出すため、`--format json` の結果には混ざりません。stderrをファイルへ保存する場合や端末幅を取得できない場合は表示しません。

`--timings` は実行したケースとコマンド全体の所要時間を表示します。
実行しないケースに時間は付きません。一覧表示ではコマンド全体だけを計測します。

`--format json` は stdout に一つの JSON 文書を出力します。
`cases` にケース、`scripts` に各ファイルの処理結果、`errors` に異常やオプションによる拒否、
`summary` に全件の集計、`exit_code` に終了コードを持ちます。
時間は `duration_ns` の整数で、未計測は null です。
quiet でも summary は全件を集計します。cases は省略される場合があるため、件数は summary を使います。

ファイルの相対パスは起動時の作業ディレクトリを基準にします。
絶対パス・親ディレクトリを含むパス・symlink も使えます。
パスの空白や区切り文字はそのまま扱い、拡張子は補完しません。
位置引数のファイルは一件、各オプションの指定は一度だけです。
`--` 以降はファイル名として扱います。`-` で始まるフィルターは `--it=-name` の形で指定します。

API の詳細は [Test の標準定義](../../lib/test.srt)、
`do` の使い方は [Monad の逐次処理](./do.md)を参照してください。
