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
そのまま `assert_err_contains` に渡します。検証対象に `?` や `=?` を使うと、
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
| `assert_ok_eq(expected, result)` | `Ok` の成功値を `Eq` で比較 |
| `assert_err_contains(fragment, result)` | `Err` の表示に文字列が含まれること |
| `assert_doc_plain_eq` / `assert_doc_ansi_eq` | StyledDoc の出力 |
| `assert_stdout_eq` / `assert_stderr_eq` | キャプチャした出力行 |

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
