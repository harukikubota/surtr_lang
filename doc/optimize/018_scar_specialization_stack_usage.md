# Scar specialization のスタック使用量修正

## 目的

`cargo run -- test --all` が `lib/tests/function.srt` の型検査中に Rust のメインスレッドを stack overflow させる問題を修正する。OS のスタック上限を増やす回避策には依存しない。

## 現状と原因

- 通常の 8176 KiB stack では `target/debug/surtr test function` が exit 134 で abort する。
- `function.srt` の `.eldr` cache がない compile path で再現する。大きい stack で一度生成した cache が残っていると、次回は型検査を迂回して症状が隠れる。
- `float` 単独は成功し、`function` 単独で再現するため、`--all` の累積状態や VM 実行が原因ではない。
- LLDB では Scar の `Checker::rewrite_specializations_in_node` で落ちる。呼び出し列は `rewrite_specializations_in_node -> materialize_trait_method_instantiation -> ensure_specialized_def -> rewrite_specializations_in_node` を含む。
- 同じ入力は stack を 8704 KiB へ増やすと 10/10 成功するため、無限再帰ではなく有限の specialization 書き換えで stack を使い切っている。
- debug binary の逆アセンブルでは `rewrite_specializations_in_node` が 1 呼び出しあたり約 384672 bytes (`0x5d000 + 0xea0`) を確保する。`c28d185c` の親では約 233920 bytes (`0x39000 + 0x1c0`) であり、method instantiation / constructor carrier 統合後に約 64% 増えている。
- `function.srt` の各テスト群を分離すると通常 stack で成功する。個別の Surtr 関数の runtime 再帰ではなく、複数の trait/callable specialization を含む AST を、巨大なフレームを持つ再帰関数で書き換える組み合わせが閾値を越えている。

`c28d185c` より前は現行 `function.srt` を別の型エラーで拒否するため、同一入力による first-bad commit の比較はできない。ただし、スタックフレーム増加と当該経路の変更は同 commit に一致する。

## 変更後の契約

- specialization の意味論、生成する function index、trait implementation 選択、診断は変えない。
- `rewrite_specializations_in_node` の巨大な自動変数を減らす。match arm の小さい helper への分割、large value の boxing、または明示 worklist 化を用い、通常の debug build / OS stack で有限 AST を処理する。
- stack size を増やす launcher、再試行、fallback は追加しない。
- specialization key の衝突回避と再帰 definition の予約は維持する。

## 受入条件と検証

Level 2。Scar 内部の局所的な実装修正で、言語仕様やフェーズ間契約は変更しない。

1. 現行 `lib/tests/function.srt` と同等以上の specialization 深度を持つ Scar 回帰テストが、通常 stack の debug build で成功する。
2. `cargo run -- test --quiet function` が exit 0 となり、10 tests が成功する。
3. `cargo run -- test --quiet --all` が abort せず最後の aggregate summary まで到達する。既存の独立した型エラーは本修正の成功条件と混同せず、別件として列挙する。
4. 対象 crate は `rtk cargo nextest run -p scar <回帰テスト名>` で確認する。意味論に触れない限り workspace 全体の nextest は不要。

## 対象外

- `either.srt` の `AmbiguousReturnTypeArgument`。
- `list.srt` / `option.srt` の `Alternative::empty` return type argument arity error。
- parser の極端な括弧ネストなど、specialization 以外の stack overflow。
