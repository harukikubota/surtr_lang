# PR: Pattern failure の診断本文・Help・処理先キャプションの改善

2026-10-07。ユーザ合意済みの実装用仕様書（level2）。未実装。

## 目的と範囲

失敗し得る Pattern を使ったとき、返り型がどのように失敗を処理できるかを説明する。
「Alternative の実装がない」という Trait dispatch の結果だけでは、MonadFail も選べることや、実装による挙動の違いが分からない。
診断本文は必要なTraitの未実装を簡潔に示し、実装による挙動の違いはHelpで説明する。
失敗箇所に加えて、通常関数の戻り値型またはdoキーワードに、処理先の型をキャプションとして表示する。
言語の型規則・評価規則は変更しない。do内のpartial `<-` とSafeBindは同じ本文・Helpを使う。
total `<-`、guard、SafeBind の RHS 射影規則、通常の Trait 呼出し診断は対象外。

## 現行規則と実測

正本は `docs/dev/Do_intrinsic_spec.md`、`docs/dev/diagnostics.md`、標準定義の `lib/traits/operator/{monad_fail,alternative}.srt`。
実装は `crates/scar/src/checker/carriers.rs`、診断本文は `crates/diagnostics/src/typecheck.rs`。

do 内は MonadFail を先に選び、なければ Alternative を選ぶ。両方なければ拒否する。
do 外の通常関数・Closure は、自身の返り型の MonadFail だけを使う。
Extractor / ExtractorClosure の専用 MatchResult target は今回の通常関数の案内対象に含めない。

`cargo run --quiet -- check tmp/box.srt` で現行ソースをビルドし、`target/debug/surtr run` で確認した。
一時ディレクトリの probe は削除済み。製品コード・テスト・`tmp/box.srt` は変更していない。

| 入力 | 現行結果 |
|---|---|
| `tmp/box.srt` の `2 <- Identity(3)` | `No implementation satisfies Alternative for Identity<Int>` |
| Identity の do 内で `2 =? Ok(3)` | 同上 |
| Identity を返す通常関数で `2 =? Ok(3)` | `` `=?` requires an enclosing MonadFail return type, got Identity<Int> `` |
| Option を返す通常関数で `2 =? Ok(3)` | `` `=?` requires an enclosing MonadFail return type, got Option<Int> `` |
| Option の do 内で `2 =? Ok(3)` | compile 成功。実行結果 `Option::None` |
| Result の do 内／通常関数で `2 =? Ok(3)` | compile 成功。実行結果 `Err(PatternMismatch("Pattern did not match."))` |

## 変更後の診断

以下は合意した日本語の表示サンプル。Trait名・型名・コード表記を維持する。
本文、キャプション、Helpは別々の診断要素として生成し、Helpの説明を本文へ混ぜない。

### 通常関数内のSafeBind

```text
Error: MonadFail が実装されていません。

  1 │ def sample() -> Identity<Int> {
    │                 ──────┬──────
    │                       ╰─ 戻り値: Identity<Int>
  2 │   2 =? Ok(3)
    │     ─┬
    │      ╰─ 失敗を処理するために MonadFail が必要です。

Help:
  MonadFail を実装すると、失敗の Error を保持して返せます。
  Alternative による empty() への変換は do 内でのみ使えます。
```

追加キャプションのスパンは、関数宣言の戻り値型 `Identity<Int>`。
表示する型は最も近いcallableの解決済みの失敗処理先とし、SafeBindのRHSや外側の関数から取得しない。
Alternativeのみを実装する返り型も、do外では同じ理由で拒否する。

### do構文内のSafeBind

```text
Error: MonadFail または Alternative が実装されていません。

  1 │ ret: Identity<Int> = do {
    │                      ─┬
    │                       ╰─ do_carrier: Identity<Int>
  2 │   2 =? Ok(3)
    │     ─┬
    │      ╰─ 失敗を処理するために MonadFail または Alternative が必要です。

Help:
  MonadFail を実装すると、失敗の Error を保持して返せます。
  Alternative を実装すると、Error を破棄して empty() を返せます。
  Option では None、List では空Listになります。
  両方を実装している場合は MonadFail を使います。
```

追加キャプションのスパンは `do` キーワードの2文字。`::<Identity>` やブロック全体を含めない。
このスパンと、そのdo自身の解決済み `do_carrier` をセットで保持する。
入れ子のdoでは最も近いdo自身のスパンとcarrierを使う。

### do構文内のpartial `<-`

本文・Help・do側の追加キャプションはSafeBindと同じ。失敗箇所のキャプションはPatternを指す。

```text
  2 │   2 <- Identity(3)
    │   ┬
    │   ╰─ 失敗を処理するために MonadFail または Alternative が必要です。
```

## 実装状況ごとの振る舞い

| 失敗処理先の実装 | do内のpartial `<-`／合法なSafeBind | do外の通常関数のSafeBind |
|---|---|---|
| MonadFailもAlternativeもなし | 上記のdo用診断で拒否 | 上記の通常関数用診断で拒否 |
| Alternativeのみ | Errorを破棄してempty()を返す | 上記の通常関数用診断で拒否 |
| MonadFailあり | Errorを保持してfail(error)を返す | Errorを保持してfail(error)を返す |
| 両方あり | MonadFailを使う | MonadFailを使う |

能力が足りる場合は、新たなエラーや注意を出さない。

## 診断データとスパンの契約

- primaryはpartial `<-` ではPattern、SafeBindでは `=?`。追加キャプションでprimaryを置き換えない。
- 通常関数の追加キャプションは、戻り値型のsource spanと解決済みの失敗処理先の型をセットで保持する。
- doの追加キャプションは、doキーワードのsource spanと解決済みのdo_carrierをセットで保持する。
- source IDを含む元のsource originを保持し、生成されたbind closureやsynthetic nodeのスパンで代用しない。
- producerが通常関数／doの文脈と処理先の型・スパンを構造化データとして渡す。表示文字列やソース文字列の探索から復元しない。
- 通常の `Alternative::empty` 等の呼出しにdo専用の案内を混ぜない。
- 未確定返り型、generic bound不足、metadata不正、曖昧なdispatchを「未実装」にまとめない。必要なスパンの欠落も無関係な位置へのフォールバックで隠さない。

今回合意した追加キャプションは、明示された通常関数の戻り値型とdoキーワードを対象とする。
戻り値型を明示しない関数・Closureで追加する位置は未確定であり、実装時に無断で別の位置へ広げない。
既存の最も近いcallableを使う失敗処理規則は維持する。

## 実装手順と検証

1. Scarの失敗処理先の解決と診断producer、関数戻り値型・doキーワードのsource spanの伝搬を確認する。
2. 既存のScar surface testsとdiagnostics testsを拡張し、本文・Help・primary・追加キャプションの型とスパンを先に固定する。
3. 必要な文脈をproducerから渡し、専用の本文・Help・キャプションを生成する。置き換えた旧診断経路は削除する。
4. `docs/dev/diagnostics.md` の診断データ・位置の契約を追従する。doの意味論は変更しない。

受入条件は以下とする。

- 通常関数ではMonadFail未実装の本文とHelpを表示し、戻り値型に実際の処理先の型を表示する。
- do内ではpartial `<-` とSafeBindで本文・Helpが一致し、doキーワードに実際のdo_carrierを表示する。
- 非実装／Alternativeのみ／MonadFailあり／両方ありの境界を維持する。
- 入れ子のdoや通常関数内のdoで、外側の型・位置を誤って追加表示しない。
- JSONでも追加キャプションに対応するsource spanと処理先の型を保持する。
- 通常のTrait呼出し、SafeBindのRHS射影、成功時の処理には変更を加えない。

検証は診断テストと関連するScarテストから始め、source spanの伝搬に変更が及ぶcrateの関連テストを追加する。
複数フェーズに及ぶ変更の全体検証には `rtk cargo nextest run --profile ci --workspace` を使う。
検証範囲は最終的な変更箇所に合わせ、実行コマンド・結果・未検証範囲を報告する。

このPR仕様書の作成では製品コード・テストを変更していない。上記の実測は先行調査の結果であり、変更後の検証結果ではない。
