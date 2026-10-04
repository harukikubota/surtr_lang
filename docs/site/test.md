# テストを書く

`Test` は `surtr test` で実行するテストの標準モジュールです。
`test` と任意の `describe` でまとめ、`it` ごとに一件の成否を記録します。
各 `it` の本文は `Result<()>` を返します。
`Test` は test サブコマンドで読み込みます。このページの例はテストファイル用で、
通常の REPL から `import Test` することはできません。

```surtr
import Test;

test("addition") {
  it("adds two values") {
    assert_eq(3, 1 + 2)
  }
}
```

テストは `lib/tests/<name>.srt` に置き、`surtr test <name>` で実行します。
`surtr test --all` はテストを辞書順に実行します。`--quiet` を付けると、成功時の出力を省きます。
テストと各 `it` は直列に実行されます。

## 名前で選ぶ・一時停止する

```sh
surtr test --all --test Parser --describe expression --it nested
surtr test parser --include-xit --deny-pending
surtr test --all --list --timings --format json
```

`--test` と `--describe` は、それぞれ同じ種類の祖先スコープの名前に照合します。
`--it` は it / xit / pend 自身の名前に照合します。大文字小文字を区別する部分一致で、
指定した種類の条件をすべて満たすケースを選びます。同じ種類の祖先は、どれか一つに一致すれば選べます。
`--it=nested` の等号形式も使えます。フィルターを明示して全ファイルを通じて一件も一致しなければ失敗です。

```surtr
import Test;

test("Parser") {
  xit("nested expression", "ネスト処理を修正中") {
    assert_eq(3, 1 + 2)
  }
  pend("error position", "診断機能の実装待ち")
}
```

`xit` は本文を持つ一時停止、`pend` は本文のない未実装項目です。どちらも空白だけではない理由が必要です。
`--include-xit` は選択された xit を通常のケースとして一度実行します。
`--deny-pending` は選択された pend があれば失敗にします。一覧でも同じ規則です。
ケース本文の中に it / xit / pend を宣言することはできません。

`--list` は本文を実行せず、宣言種別・スコープ・名前・理由・実行予定を表示します。
**一覧とフィルターでも、トップレベルと test / describe 内に直接書いた処理は実行します。**
停止中や選択外の本文もコンパイルするため、型エラーを隠しません。

## 出力と実行時間

通常の実行結果は PASS / FAIL / SKIP / PENDING / FILTERED で表示します。
`--quiet` は成功・停止・許容された未実装項目などの詳細と成功サマリーを省きます。
一覧では quiet でも選択項目を表示します。失敗と拒否された pend は省略しません。

`--timings` は実行したケース本文とコマンド全体を計測します。
実行しないケースには時間を付けず、一覧ではコマンド全体だけを計測します。

`--format json` は stdout に一つの JSON 文書を出力します。
`cases` にケース、`scripts` に走査出力と完了状態、`errors` に異常とポリシー違反、
`summary` に全件の集計、`exit_code` に終了コードを保持します。
時間は `duration_ns` の整数で、未計測は null です。
quiet では正常なケース詳細を省きますが、summary は全件を集計します。
件数を cases 配列の長さから求めないでください。

すべてのオプションは一度だけ指定できます。位置引数はファイル一件とし、`--all` と併用できません。
`--` 以降はファイル名として解釈します。`-` で始まるフィルターには `--it=-name` の形式を使います。

## 複数のアサーション

複数の条件を一つの `it` で確認するときは、`do::<Result>` でアサーションを順に実行できます。
最初の `Err` でその `it` の後続処理を止め、失敗を一件記録します。
`it` 自体は記録後に `Ok(())` を返すため、次の `it` に進みます。
この契約は `Result` による失敗に適用され、VM の実行異常からの回復を行うものではありません。

```surtr
import Test;

test("conditions") {
  it("checks several conditions") {
    do::<Result> {
      assert_true(True)
      assert_eq(3, 1 + 2)
      assert_false(False)
    }
  }
}
```

通常の本文にアサーションを並べるだけでは、途中の返り値が捨てられ、失敗を見逃します。
アサーションは結果を個別に集計せず、`it` が本文の最終的な `Result<()>` を検査します。
一つの `it` を一つのアサーションに制限する必要はありません。

`do` を使わず、[文末の `?`](./error-handling.md) で失敗を伝播することもできます。

```surtr
import Test;

test("statement question") {
  it("checks several conditions") {
    assert_true(True)?
    assert_eq(
      3,
      1 + 2,
    )?
    assert_false(False)
  }

  it("returns Result after the final question") {
    assert_eq(3, 1 + 2)?
    Ok(())
  }
}
```

`?` を付けた文は成功時に Unit を返します。末尾にも `?` を付ける場合は、
その後に `Ok(())` を置きます。Unit を `Result<()>` に変換する暗黙の規則はありません。
値を取り出す処理には、既存の `value =? operation()` を使います。

## 期待する Err を検証する

失敗すること自体を確認する場合は、Result を通常の `=` で保存するか、
そのまま `assert_err`・`assert_err_kind`・`assert_err_contains` に渡します。検証対象に `?` や `=?` を使うと、
期待している Err をアサーションに渡す前に伝播してしまいます。

```surtr
import Test;

test("expected failure") {
  it("checks the error and continues") {
    result: Result<Int> = Err(NoneError)
    do::<Result> {
      assert_err_contains("NoneError", result)
      assert_eq(3, 1 + 2)
    }
  }
}
```

## 主なアサーションと入出力

| 関数 | 検証内容 |
|---|---|
| `assert_true(flag)` / `assert_false(flag)` | Boolean の値 |
| `assert_eq(expected, actual)` | 同じ型の二つの値を `Eq` で比較 |
| `assert_ne(expected, actual)` | 同じ型の二つの値が `Eq` で異なること |
| `assert_ok(result)` / `assert_err(result)` | 中身を比較せず Result の枝を検査 |
| `assert_err_kind(marker, result)` | 具体的な deferror 宣言と Err の種別が一致すること |
| `assert_some_eq(expected, option)` / `assert_none(option)` | Some の値の等価性、または None の枝 |
| `assert_contains(fragment, actual)` | 大文字小文字を区別する部分一致 |
| `assert_approx(expected, actual, tolerance)` | Float の絶対誤差が非負の許容誤差以下であること |
| `fail(detail)` | 指定した detail を保持して必ず失敗 |
| `assert_ok_eq(expected, result)` | `Ok` の成功値を `Eq` で比較 |
| `assert_err_contains(fragment, result)` | `Err` の表示に文字列が含まれること |
| `assert_doc_plain_eq` / `assert_doc_ansi_eq` | StyledDoc の出力 |
| `assert_stdout_eq` / `assert_stderr_eq` | キャプチャした出力行 |

`assert_ne` と `assert_some_eq` は Eq を要求します。inspect による代替比較は行いません。
`assert_ok`・`assert_err`・`assert_none` は値の Eq を要求しません。
`assert_err_kind(NoneError, result)` の marker は具体的な deferror 宣言名に限ります。
payload 付きの宣言も名前だけを渡せます。同名の別 namespace の宣言は別種として扱い、message や cause は比較しません。
表示文字列を検査するときは `assert_err_contains` を使います。

`assert_approx` は絶対誤差だけを扱い、Int を Float に暗黙変換しません。
許容誤差ちょうどは成功です。負の許容誤差や、差が有限 Float に収まらない場合はアサーション失敗になります。

失敗時のキャプションは、実際に失敗したアサーションの呼出式を示します。
たとえば `assert_true(False)` が失敗した場合は、その式とファイル名・行・列を表示します。
`do` や文末の `?` で伝播した場合も同じ位置を示します。
`assert_eq` を直接呼び出した場合は、期待値と実際の値に LHS/RHS のラベルを付けます。

`capture_stdout()` と `capture_stderr()` は、前回読み取った後の出力を返し、
バッファを空にします。`push_stdin(text)` はテスト用の入力バッファに文字列を追加します。
stdout・stderr・stdin は `it` ごとに分離され、未読の内容は次の `it` に持ち越されません。

```surtr
import Test;
import IO;

test("IO") {
  it("reads each line in order") {
    do::<Result> {
      push_stdin("alpha\nbeta\n")
      assert_ok_eq("alpha", IO::get_line(""))
      assert_ok_eq("beta", IO::get_line(""))
    }
  }
}
```

各 API の一次情報は [Test の標準定義](../../lib/test.srt)、
`do` の使い方は [Monad の逐次処理](./do.md) を参照してください。
