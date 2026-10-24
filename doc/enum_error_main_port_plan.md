# EnumError の現行 main 追従

入力仕様は [EnumError 仕様書](/Users/haruca/.codex/worktrees/c6b8/surtr/doc/enum_error_spec.md)、提供された「EnumError 仕様書 訂正文.md」、[互換性調査](enum_error_main_compatibility_2026_10_24.md)、[Error 位置の表記](enum_error_error_slot_syntax.md)と、2026-10-24 のユーザーの修正指示。main `a7d75ed064504079660cd58149a4f3dea41df14b` を土台にする level4 の実装。元の f504 と main の作業内容を保持するため、この専用 worktree で進める。

1. main の分割・stage・名義型 schema を維持し、EnumError の分類情報を field 解決前に一括登録する。宣言順の入替えを成功テストで固定する。
2. Error 位置・変換・具体化・Pattern・runtime schema を main の公開型と visitor に移植する。旧 monolith は復活させない。
3. MonadExtractor の標準境界を能力判定と呼出しに適用し、SafeBind・再帰 `?`・partial `<-` の変換と一回評価を保持する。ユーザー impl の契約を維持する。
4. ResultT 三引数化、明示 Error `_` 禁止、標準定義、診断、補完・宣言表示、文書・テストを整合する。
5. 対象テスト後、`rtk cargo nextest run --profile ci --workspace` と `rtk proxy cargo run -- test --quiet --all` を直列実行し、別エージェントの最終レビューを行う。

新規問題は Astra に相談する。旧互換経路・曖昧な fallback を追加せず、VM / schema のバージョンは上げない。コミット・main への統合は別途の指示があるまで行わない。

## 実装上の対応

- EnumError の owner・members・Other・宣言 span を同 stage の field 解決前に登録する。名義型の Error 位置は宣言 schema の有限な依存関係から求め、前方参照と再帰参照でも宣言順に依存させない。
- main の `NominalType.arguments`、分割した checker、具体化の worklist を使う。削除済みの checker ファイルや型値内の field 情報は戻さない。
- 標準実装への変換は選択した実装の由来を検査して適用する。通常の Eq 呼出し、汎用の構築子引数、MonadExtractor の能力判定と実呼出しで同じ契約を使う。利用者 impl を標準実装へ置き換えない。
- 変換済みの値は元の provenance と変換後の型を保持し、field・要素の投影に沿って変換を伝える。型が同じ部分は元の情報を保持し、Result / ResultT の E が変わる位置では変換先で能力を証明する。変換前の能力で変換先の拒否を補わない。
- main の再帰的な `?`、SafeBind、partial `<-`、Test の `expect_error` / `assert_error_matches`、非最終文の Unit 制約を維持する。ResultT の三引数、Error 位置の明示 `_` 拒否、REPL と解析機能の型表示も追従する。

## 受入条件とテスト

- `crates/scar/tests/enum_error_slots.rs`: 同 stage の宣言順入替え、別 namespace の参照、前方・再帰 schema の Error 位置、Result / ResultT の `_` 拒否、Trait 禁止と inherent 関数。
- `crates/scar/tests/enum_error_generic_extraction.rs` / `enum_error_equality.rs`: 標準実装の境界、利用者 impl の保持、複数の構築子制約の両順序、関数呼出し・集約型からの抽出、成功 payload の E 保持。
- `crates/scar/src/checker/provenance.rs` の内部テスト: 投影先の Deferred / Rejected の保持と、後続分岐が要求する能力の検査。
- `tests/fixtures/modules/pass/enum_error_monad_extractor/`: 明示抽出、SafeBind、入れ子の `?`、partial `<-`、Error の Payload / message / kind、一回評価、利用者の専用実装。
- 標準 Result / ResultT、Test の assertion、Xldr / 解析機能の既存・追加テスト: main の機能と EnumError の契約の両立。

## 検証結果

最終コードで次を確認した。コードを凍結した後の全件検証と独立レビューが完了している。

| 検証 | 結果 |
|---|---|
| Scar lib と EnumError の slots / diagnostics / equality / generic extraction | 186 件成功 |
| `rtk cargo nextest run -p rune --test integration run_srt` | 9 bucket 成功 |
| `rtk cargo nextest run -p rune --test integration module_import_fixtures` | 11 bucket 成功 |
| cold profile の `test_command_long_do_preserves_order_and_short_circuit_without_stack_overflow` | 成功 |
| `rtk cargo nextest run --profile ci --workspace --test-threads 4` | 2,831 件成功、79 binary、183.817 秒、除外なし |
| `rtk proxy cargo run -- test --quiet --all` | 終了コード 0 |

既定の並列実行では、既存の `bootstrap_source_does_not_see_standard_nominals_from_native_inventory` が 15.024 秒でタイムアウトし、他の 2,830 件は成功した。同ケースは単体で 5.890 秒で成功したため、コード・テスト対象・15 秒の制限を変えず、並列数 4 で全件を再実行した。タイムアウトの延長や ignored 化はしていない。

新たに見つかった問題は Astra と確認した。別エージェントによる最終差分のレビューでは、未対応の指摘はない。Error の情報保持と一回評価、標準実装と利用者実装の区別、投影失敗と全分岐の制約、旧経路の削除を確認した。

## 作業内容の保持

実装と検証は `/Users/haruca/.codex/worktrees/enumerror-main/surtr` で行った。起点は main の `a7d75ed064504079660cd58149a4f3dea41df14b`。main の作業ツリーの既存未追跡ファイルを保持し、元の f504 の 308 パスが移植前の snapshot tree と内容一致することも検査した。

統合時はユーザーの指示に従い、テストを再実行せず、今回の差分だけをコミットして main へ取り込む。統合内容と既存ファイルの保持を確認した後、専用ワークツリーと作業ブランチを削除する。
