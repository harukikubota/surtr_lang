# List / VM の残課題

更新日: 2026-10-03。状態: 未解決事項の調査・設計用。

以下には追加の性能改善を残す。
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

### RT-5 REPL checkpoint の残るコピー量

- 現状: process・future・detached taskの3表とentryをcheckpointで共有し、変更時に表のindexと対象entryを複製する方式へ移行した。可変Builderはそのentryの初回変更時に独立させる。通常の実行切替えでは引き続き複製しない。
- 残る範囲: metadata、singleton/worker表、waiting/reply表、queueの複製と、変更時の表indexの複製が残る。これらの共有を広げるかは未決定で、runtime全体の定数時間保存を保証しない。
- 次の作業: metadataやqueueの件数、future数、Builderの長さ、chunk数を分け、実chunk全体の時間とピークメモリを測る。停止済みPIDの回収契約は別に確定する。
- 受け入れ条件: 失敗したchunkの進捗を破棄して保存位置から再開し、その位置より前のcallbackを呼び直さず、復帰先へ一度だけ結果を渡す。実行中に変更する可変Builderは保存状態から独立させる。失敗したchunkの外部I/Oは巻き戻さない。

## 今後の検証と文書更新

着手する項目の契約を直接検証するテストから始め、影響範囲に応じて
[テスト方針](../docs/dev/テスト方針.md) の全体 gate と独立レビューを実施する。
解決した項目は本書から削除し、確定した契約を対応する正本へ移す。
