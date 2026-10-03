# PR: テストの失敗伝播と Unit 専用の文末 `?`

## 1. 状態と目的

- 状態: 未実装の設計提案。今回の変更は本書だけとする。
- 作成日: 2026-10-04。
- 調査基準: `c9ba1c6606220a5743a93390fdc1ae9d88b08aca`。
- level: 4。文構文、受理する型、早期リターンの契約を追加する。
- 入力: `Test` の複数アサーションについての確認と、ユーザーが指定した文末 `?` の設計。

テストは直列に実行し、各 `it` の本文が返す `Result<()>` で成否を記録する。複数アサーションは通常の Result の短絡で組み立て、最初の失敗でその `it` を終了する。アサーションごとに集計プロセスへ結果を送る仕組みは追加しない。

`do` でアサーションを並べる方法に加え、独立した文の末尾に `?` を付け、成功値を捨てて Err を伝播できるようにする。値を取り出して式の計算へ渡す能力は持たせず、対象の終端の成功型を Unit に限定する。`do` は強制しない。

本書の `?` を含む例は実装後の目標であり、現行処理系で実行確認した例ではない。

## 2. 現行挙動と問題

[標準 Test 定義](../lib/test.srt) のアサーションは `Ok(())` または `Err(TestAssertionFailed(...))` を返す。各アサーションは成否を個別に記録しない。`it` が本文の返り値を検査し、VM に成否を一件記録する。

通常の本文にアサーションを並べるだけでは、末尾以外の返り値を捨てるため、途中の失敗を見逃す。

```surtr
import Test;

test("example") {
  it("途中の失敗が返されない") {
    assert_eq(1, 2)
    assert_eq(3, 3)
  }
}
```

この `it` は末尾の `Ok(())` によって成功として記録される。[標準 IO テスト](../lib/tests/io.srt) などにも複数アサーションを単純に並べた箇所があるため、実装時に移行対象を調査する。

既存 SafeBind と `do` は最初の失敗で短絡する。この挙動をテストにも採用する。`it` はアサーション失敗を記録した後に `Ok(())` を返すため、次の `it` へ順次進められる。これは Result による失敗についての契約であり、VM の実行異常を回復する契約ではない。

集計用プロセスを設けても、IO キャプチャには IO ハンドラの変更が必要で、ランタイムを分割できないため直列実行に限られる、というユーザーの設計条件を前提とする。本案ではその改修を行わず、既存の `it` ごとの IO キャプチャを維持する。

## 3. 文末 `?` の構文

```text
StatementQuestion := Expression "?"
```

これは文に対する単項構文であり、一般の式の postfix 演算子ではない。独立した文として書かれた式全体を対象とする。改行または既存の文区切りまでを一文とし、複数行の呼び出しにも付けられる。物理的な一行だけに制限しない。

```surtr
assert_true(condition)?
assert_eq(
  expected,
  actual,
)?
```

成功時の文の型は Unit。束縛を作らず、成功値を引数、演算、戻り値などへ渡せない。`Result<Int>` などから値を取り出す場合は既存の `value =? operation()` を使う。

次の式位置は拒否する。

```surtr
value = operation()?
consume(operation()?)
operation()? + 1
```

`?` を重ねて型を段階的に取り出す構文も追加しない。型位置の optional suffix と FacetPath の optional segment にある既存の `?` は、それぞれの構文として維持する。文末 `?` の解釈に失敗しても、別の式構文へフォールバックしない。

## 4. 受理する型と評価

対象は canonical Result であり、成功型をたどった終端が Unit になる型だけを受理する。`()` と Unit は同じ型を指す。

```text
UnitSuccess := Unit | Result<UnitSuccess, E>
対象型      := Result<UnitSuccess, E>
```

`E` は既存 Result が受理する error 型。新しい error 型規則は追加しない。型 alias は既存の正規化に従い、名前文字列で Result と判定しない。

| 対象型 | 判定 |
|---|---|
| `Result<()>` | 受理 |
| `Result<Result<()>>` | 受理 |
| さらに Result がネストし、終端が Unit | 受理 |
| `Result<Int>` / `Result<String>` | 型エラー |
| `Result<Result<Int>>` | 型エラー |
| Unit / Boolean / Option / List | 型エラー |
| 終端の成功型を確定できない型 | 型エラー。Unit と仮定しない |

対象の式は一度だけ評価する。成功時は値を捨て、次の文へ進む。値の取り出し能力を持たないため、ネストを受理しても終端の Unit 以外の成功値を利用できない。

Err に対する操作は既存 SafeBind と同等とする。単層では、型制約付きの `_ =? rhs` の糖衣として扱う。

```surtr
operation()?
```

は、制御フローとして次と同等である。

```surtr
_ =? operation()
```

ネストの受理は型制約の規則であり、再帰的な Err の探索を追加する規則ではない。既存 SafeBind の canonical Result の外側一段だけの射影を維持する。例えば、静的型が `Result<Result<()>>` の `Ok(Err(error))` は外側が Ok なので成功側で捨てられる。内側の Err を観測したい場合は、Result のまま保存して検査するか、既存 SafeBind で必要な段を明示して処理する。

返却先の可否は、その位置の SafeBind の規則に従う。通常の callable 内ではその callable の Result / Result-effect の返却先を使い、`do` 内では既存の do-local failure target を使う。単なる波括弧ブロックを新しい返却先にはしない。返却先が成立しない位置はエラーにする。

伝播時は元 Error の kind、message、location、cause を保持する。新しい error への置換、暗黙の回復、テスト向けの特別な伝播経路は追加しない。

## 5. テストでの使い方

```surtr
import Test;

test("example") {
  it("複数の条件を検証する") {
    assert_true(True)?
    assert_eq(3, 1 + 2)?
    assert_false(False)
  }

  it("期待する Err を検証する") {
    result: Result<Int> = Err(NoneError)
    assert_err_contains("NoneError", result)?
    Ok(())
  }
}
```

最初の `it` では各成功後に次のアサーションへ進み、途中の Err は本文から返る。すべて成功した場合は末尾のアサーションの `Result<()>` を返す。すべてのアサーションに `?` を付ける場合は、末尾に `Ok(())` を置く。末尾の Unit から `Ok(())` への暗黙変換は追加しない。

期待する Err の検証では、通常の `=` で Result を保存するか、直接アサーションへ渡す。検証対象そのものに `?` を付けると失敗を伝播してしまうため、検証用アサーションの返り値に `?` を付ける。

現行 Test の Boolean アサーション名は `assert_true` / `assert_false`。本案は `assert(True)?` と同じ使用感を目的とするが、引数一つの `assert` API の追加・改名は含まない。

既存 `do::<Result> { ... }` による sequencing も利用できる。`it = 1 assert` という制限は設けず、複数アサーションの使用方法を文書化する。どちらの書き方でも一つの `it` で報告するアサーション失敗は最初の一件とする。

## 6. 未使用値の警告は別改修

Unit 以外の戻り値が使われていない場合にユーザーへ警告する機能は未実装として扱う。[Scar の検出基盤](../crates/scar/src/checker/mod.rs) と [検出テスト](../crates/scar/tests/warnings.rs) は存在するが、検出だけをもってユーザー報告機能が完成したとは扱わない。

警告をユーザーへ届ける仕組みは別改修とし、その経路、表示、既存の明示的な破棄の扱いを別の仕様で定める。本案の `?` は成功値を明示的に捨てる Unit 文なので、未使用の Result 値として扱わない。

## 7. 実装範囲と順序

1. 本書を実装入力として、文の構文と型制約を正本へ反映する。対象は `docs/site/error-handling.md`、`docs/site/do.md`、`docs/dev/Do_intrinsic_spec.md`、`docs/dev/テスト方針.md`、Test の `@doc`。警告のユーザー報告は別改修のままとする。
2. Spire で独立した文末だけを認識し、既存 optional 構文と式位置の拒否を固定する。tolerant parser と構文の再帰走査も整合させる。
3. Sigil / Scar で canonical Result と終端 Unit を検査し、文末 `?` の制約を検査した後に既存 SafeBind の制御フローへ接続する。構文固有の型制約を消して一般の `_ =? rhs` として無条件に受理しない。
4. Forge 以降は既存 SafeBind の経路を再利用する。新規集計プロセス、IO ハンドラ、opcode、builtin は追加しない。旧経路や曖昧なフォールバックを残さない。
5. 標準テストの複数アサーションを調査し、目的に応じて文末 `?` または `do` に移行する。末尾の Result と、期待する Err を直接検査する箇所を維持する。

## 8. 受入条件と検証

- 単層・複数段の終端 Unit の型を受理し、終端非 Unit、non-Result、未確定型を拒否する。
- 一文全体への適用、複数行 call、既存文区切りを検証し、引数・束縛 RHS・演算途中への組み込みと `?` の連続を拒否する。
- optional 型と FacetPath の既存 `?` の意味を維持する。
- 成功時は後続文へ進み、外側の Err は既存 SafeBind と同じ返却先へ届く。元 Error の情報を保持し、失敗後の副作用を実行しない。
- 対象式を一度だけ評価する。ネストの内側の Err を新たに再伝播せず、Result として明示的に検査できる。
- `it` の途中の失敗を見逃さず、その `it` の後続アサーションを実行しない。次の `it` は直列に実行され、IO キャプチャは各 `it` の既存の分離規則を守る。
- 末尾の `?` が Unit を返す場合は既存の返り値型検査を適用し、`it` の `Result<()>` に暗黙 wrap しない。
- `do` は任意とし、文末 `?` と既存 SafeBind の返却先の境界を、通常 callable、nested callable、`do` で検証する。

実装時は parser / Scar の直接の成功・拒否テスト、script fixture、標準 SRT、CLI の成否と診断の必要な境界を検証する。level4 として `rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all` と、最終差分への別エージェントレビューを実施する。

今回は設計提案書の作成のみ。製品コード、標準定義、実行可能テスト、正本文書は変更せず、コンパイラのビルドと実行テストは行わない。
