# List / VM の残課題

更新日: 2026-10-03。状態: 未解決事項の調査・設計用。

共通の予算付き driver、builtin continuation、Packed List、`List::flat_map` の builtin 化は実装済み。
以下には追加の性能改善と、実装中に確認した既存のコンパイル問題を残す。
最適化の採用方式や実時間の目標値は、各項目の調査後に決める。

## 現行の契約と参照先

- List の表現・仮想計算量・継続の所有権: [Eldr VM 仕様](../docs/dev/EldrVM_spec.md)。
- process / Task の待機・取消・結果配送: [Process Runtime 仕様](../docs/dev/ProcessRuntime_spec.md)。
- generic do の型検査・lowering: [do intrinsic 仕様](../docs/dev/Do_intrinsic_spec.md)。
- 公開 API と `Monad::bind`: [List の標準定義](../lib/types/list.srt)。
- ListHandle: [runtime.rs](../crates/sindr/src/runtime.rs)。Builder: [list_builder.rs](../crates/eldr/src/builtin/list_builder.rs)。
- List 操作の回帰基準: [標準テスト](../lib/tests/monads/list.srt)。
- 予算・操作数・失敗・rollback の回帰基準: [VM テスト](../crates/eldr/src/vm/list_flat_map_tests.rs)、[REPL テスト](../crates/xldr/tests/repl_core.rs)。

改善時も、論理順、永続性、mapper の呼出し回数、失敗後の未評価、source trace、
待機・取消・rollback を維持する。通常の実行切替えでは Builder を移動し、checkpoint は独立した保存状態を持つ。

## Runtime の追加調査・最適化

### RT-1 長い Cons の解放

- 現状: `from_items` は Packed を作るが、`cons` や source-level の List 操作は長い Cons の鎖を作り得る。Cons の反復的な解放は未実装。
- 記録: [release audit](v0.1_release_codebase_audit.md) には変更前の同形の鎖を10万要素解放した Rust probe で、stack overflow / exit 134 を確認した記録がある。Packed 導入後の再現条件と Surtr 実行時の閾値は未測定。
- 次の作業: Cons の反復構築、共有 tail、Cons-over-Packed を分けて再現し、最後の所有者が解放する範囲を確認する。共有を維持した反復解放の方法を設計する。
- 受け入れ条件: 大きい正常な List の生成・解放で abort せず、別の handle が参照する tail の値を保つ。入れ子の Value の解放も検証範囲に明記する。
- 検証: Sindr の所有権・共有テストと、abort を隔離して確認できる subprocess の回帰テスト。

### RT-2 Packed の使用済み先頭部分の保持

- 現状: 小さい suffix だけを保持しても、元の `Rc<Vec<Value>>` 全体が生存する。最後の参照の破棄では buffer 全体の解放が発生し得る。
- 未確定点: chunk 化、明示的な compaction、共有を続ける方式のどれを採用するか。保持量とコピー量の交換条件を先に測る。
- 次の作業: 長い Packed から短い tail を取り出して保持する workload で、生存要素数、割当量、RSS、最後の解放時間を測る。
- 受け入れ条件: `cons` / `head` / `tail` / `uncons` / `len` の仮想的な O(1)、永続性、論理順を保つ。tail のたびに全要素をコピーする経路を導入しない。改善対象と、保持量が改善しないケースを記録する。

### RT-3 Value の clone / drop、allocator、実時間の測定

- 現状: 2E 個の Cons から E 回の Builder 追加への削減は仮想操作数の結果。Value の物理コピー、入れ子の解放、Vec の再確保、実時間・RSS は別に評価する必要がある。
- 未確定点: 何が支配的なコストか、どの workload を改善対象にするか、許容するメモリ増加量。
- 次の作業: singleton、空結果、巨大な mapper 結果、三段直積、多段 singleton について、小さい値と大きい payload を分けて測る。
- 受け入れ条件: 同じ入力・ビルド条件で比較し、入力訪問数、出力数、callback 回数と物理的なコストを分けて報告する。実時間や RSS の閾値を既存 correctness test の合否条件へ混ぜない。

### RT-4 未分割 builtin と外部呼出し

- 現状: callback と flat_map の入力・出力処理は共通予算に従う。既存の純粋な Rust loop、regex / JSON、OS I/O の一回の処理には長い停止が残り得る。
- 未確定点: 各処理を既存 continuation へ分割するか、ライブラリの分割 API、非同期 I/O、worker への移管を使うか。
- 次の作業: 入力サイズに比例する builtin と外部呼出しを棚卸しし、一回の処理量、取消、資源の後処理を個別に定義する。
- 受け入れ条件: 移行した処理が完了前に他の runnable task へ切り替わり、副作用・結果配送を重複させない。CPU yield と Future 待機を区別し、batch / REPL / Task / process の通常入口から確認する。
- 制限: 処理を分割した範囲を明示する。未移行の処理、Value の物理操作、allocator を含む VM 全体の実時間上限は、別途根拠が必要。

### RT-5 REPL checkpoint のコピー量

- 現状: 通常の実行切替えで Builder は複製されない。checkpoint では復元用に Builder と runtime の状態を独立して保存する。
- 未確定点: process / future / continuation の保存を差分化するか、immutable 部分の共有を広げるか。
- 次の作業: process 数、future 数、Builder の長さ、chunk 数を別々に増やして、入力ごとのコピー量とピークメモリを測る。[release audit](v0.1_release_codebase_audit.md) の checkpoint 項目と同じ作業として扱う。
- 受け入れ条件: 失敗した chunk の進捗を破棄して保存位置から再開し、その位置より前の callback を呼び直さず、復帰先へ一度だけ結果を渡す。実行中の mutable Builder と保存状態を共有しない。失敗した chunk の外部 I/O は巻き戻さない。
- 検証: 途中の出力追加・mapper 待機を含む VM / REPL の rollback テストと、成功 chunk の継続実行。

### RT-6 追加の List 最適化

- 現状: `map` / `filter` は標準ソースで `flat_map` を組み合わせ、Builder を使う共有 builtin 経路へ移行済み。専用 builtin は追加していない。`reverse` / `append` / `concat` の追加最適化は未着手。各 bind は完成した List を返すため、多段の kM が残る。
- 未確定点: 個別操作の Builder 化、pipeline の融合、中間 List の省略のうち、どれに効果があるか。一般の nested do は B / E で評価し、常に E = kM と仮定しない。
- 次の作業: RT-3 の測定を基に対象を選び、generic do と通常の Monad dispatch を基準に評価順・失敗・待機を比較する仕様を作る。
- 受け入れ条件: 副作用を持つ mapper、空結果、部分 pattern、SafeBind、nested do で値・順序・呼出し回数・失敗後の未評価が一致する。未完成 Builder を公開型、process payload、完成結果へ出さない。

## 今後の検証と文書更新

着手する項目の契約を直接検証するテストから始め、影響範囲に応じて
[テスト方針](../docs/dev/テスト方針.md) の全体 gate と独立レビューを実施する。
解決した項目は本書から削除し、確定した契約を対応する正本へ移す。
