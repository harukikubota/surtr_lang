# 文末 `?` の実装計画

入力仕様: [test_assertion_statement_question_pr.md](test_assertion_statement_question_pr.md)。ユーザーの実装指示により、設計作成時の「本書だけ変更」「ビルドしない」という作業範囲を実装・検証へ更新する。level 4。

1. Spire / Sigil: 文位置だけで `StatementQuestion` を認識し、型制約を Scar へ保持する。構文の成功・拒否、optional 型 / FacetPath、tolerant parser と再帰走査を直接テストする。
2. Scar: canonical Result の成功型の終端が Unit であることを確定してから、既存 SafeBind の制御フローへ接続する。通常 callable / nested callable / do-local の返却先と未確定型の拒否を固定する。Forge / VM の追加経路は作らない。
3. script / CLI: 一度だけの評価、失敗後の副作用停止、外側一段の射影、元 Error の保持、次の it の実行と IO 分離を検証する。
4. 正本: error-handling、do、Do_intrinsic_spec、テスト方針、Test の @doc を更新し、docs/site/test.md を追加する。
5. 標準テスト: 同一 it の複数アサーションを `do::<Result>` の短絡へ移行する。単一アサーションと既存の適切な伝播は維持し、期待する Err は Result のまま検査する。
6. 機能・文書と標準テスト移行を別コミットにする。全件検証と独立レビューを完了してからコミットし、依頼範囲のワークツリーに保存する。

局所検証は `rtk cargo nextest run -p spire`、`rtk cargo nextest run -p scar`、`rtk cargo nextest run -p rune --test integration run_srt`、`rtk cargo nextest run --profile cold -p rune --test integration test_command`。全体検証は `rtk cargo nextest run --profile ci --workspace`、`cargo run -- test --quiet --all`、`cargo fmt --all -- --check`、`git diff --check`。

新規問題・デグレは再現条件とコード上の原因を揃え、会話履歴を引き継がない Astra に相談する。最終レビューにも仕様、最終差分と検証結果だけを渡す。

## 調査で確認した実装上の境界

- 標準の長いアサーション列を一つの do にすると、既存の closure 合成の再帰で CLI のスタック上限に達した。Monad transformer の検証群ごとに内側 do を分け、外側 do で順に短絡させる。it の構成・アサーション・期待値を維持し、コンパイラのスタック設定は変更しない。
- 未注釈 closure が外側 callable の return context を継承する既存不具合を発見した。Astra の独立調査で共有 callable 境界を原因と確認。closure 自身の期待返り型、または自身の未確定返り型へ必ず切り替え、文末 `?` と旧 SafeBind の両方で外側の target を借用させない。返却先が未確定の場合は明示的な callable signature が必要になる。
- 通常式位置の裸の `{ ... }` は無引数 closure であり、本文は値を作るだけでは実行されない。通常ブロックの失敗伝播は match arm の block で検証する。

## 最終検証

- `rtk cargo nextest run --profile ci --workspace`: 2,202 件成功、54 binaries、失敗・除外なし。
- `cargo run -- test --quiet --all`: 最終製品コードで終了コード 0。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- 文末 `?` と closure の返却先の専用テスト: 16 件成功。Scar 全体: 356 件成功。
- 新規 Test ガイドの5例と Test の @doc 例: test サブコマンドで実行成功。
- Astra の独立レビュー: 意味論・返却先・旧経路・標準テスト移行に未解決指摘なし。

標準31ファイルの217ブロックを移行した。独立したトークン比較でも、do の囲みと IO capture の明示破棄以外に、アサーション・setup・期待値・順序の変更がないことを確認した。
