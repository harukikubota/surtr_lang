# Test 拡張の実装計画

入力は [仕様書](test_feature_extension_pr.md)。level 4。
指定ブランチに既存の文末 `?` と診断位置改善を保持し、仕様書第10節の順で実装する。
仕様書の「今回は提案書だけ」は作成時の状態を示す。今回の実装依頼に従い実装と検証を行う。

1. Rune: typed options、共通トークン化。1,024通りと拒否境界を unit test で確認。
2. Eldr/Sindr/Test: 種類付きスコープ、ケース選択と状態、IOと時間。VM と CLI で検証。
3. Test/各フェーズ: 追加アサーション、ErrorKind の共有解決。SRT と成功・拒否 fixture で確認。
4. Rune: 集計と human/JSON、usage error と走査出力。CLI 境界で確認。
5. 標準テスト移行、@doc と docs/dev・docs/site・README の同期。
6. `rtk cargo nextest run --profile ci --workspace` と `cargo run -- test --quiet --all`、最終差分の別エージェントレビュー。

各段階で契約テストの Red を確認して実装する。検証後、利用者の指示に従って既存変更を分割コミットする。main へはマージしない。

## 完了結果

2026-10-04、指定ワークツリー `codex/test-statement-question` に実装。入力文書は main の未追跡ファイルからコピーし、main 側は変更していない。

- xit / pend、共通 runner policy、名前フィルター、一覧、時間、human / JSON の集計を実装した。
- 追加アサーションと ErrorKind の canonical 解決を実装し、標準テスト124箇所の inspect 比較を目的に合う API へ移した。
- Test の @doc、CLI synopsis、docs/dev と docs/site を同期した。
- TDD の Red は未対応オプション、未定義 xit、未定義 assert_ne、未定義 assert_err_kind で確認した。作成途中のテスト構文・型注釈の誤りは Red と扱っていない。
- `rtk cargo nextest run --profile ci --workspace`: 2,218 passed、54 binaries、終了0。
- `cargo run -q -- test --quiet --all`: 終了0。
- `cargo fmt --all -- --check` と `git diff --check`: 成功。
- 別エージェントが最終差分・入力仕様・検証結果をレビューし、指摘なし。

実装の残件なし。検証後に実装・標準テスト移行・文書を分割コミットする。main へのマージは対象外。
