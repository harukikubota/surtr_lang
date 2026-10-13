# List / VM の残課題

更新日: 2026-10-13。状態: 未解決事項の調査・設計用。

以下には追加の性能改善を残す。
最適化の採用方式や実時間の目標値は、各項目の調査後に決める。

RT-3のValue測定・方式選定・Future表が空の場合の製品最適化は
[最適化レポート](value/list_runtime_rt3_optimization_2026_10_11.md) に記録した。
Future表が非空でも、[位置追跡](value/future_stack_index_optimization_2026_10_13.md)により未解決Futureのslotだけを読み、[通常経路の費用](value/future_stack_metadata_optimization_2026_10_13.md)も減らした。通常値の全走査は除いたが、追跡slotのready / cleanup検索と通常経路の小さい差は[OI-036](../../open-issues.md#oi-036-future表が非空のoperand-stack走査)に残る。

## 集約資料とその後の作業

以下は当初資料だけをmainへ集約した。製品コードの統合状況は [runtime一覧](README.md) を参照する。

- RT-2: [完成Vecのchunk移譲比較](list/list_chunk_handoff_comparison_2026_10_13.md)。保持量だけでなく移譲の追加allocation・peakを測定した。製品方式は未決。
- RT-4: [Regex入力共有](regex/regex_input_sharing_2026_10_13.md)をmain `c577b32d`へコミットした。[mainでの再測定・検証](regex/regex_input_sharing_adoption_2026_10_13.md)を追加した。同期探索・収集の分割は残る。
- RT-4: [FileOutHandler再現](file-out/file_out_handler_lifecycle_reproduction_2026_10_13.md)と[修復仕様案](file-out/file_out_handler_lifecycle_repair_spec.md)。既存の資源寿命不一致はOI-040で追跡する。製品修復は未実施。
- RT-5: [Builder共有化](checkpoint/list_runtime_rt5_builder_optimization_2026_10_12.md)、[通常経路の追加調査](checkpoint/list_runtime_oi039_cost_2026_10_12.md)、[実VMメモリ境界](checkpoint/list_runtime_oi039_vm_memory_2026_10_12.md)、[末尾所有の比較](checkpoint/list_builder_owned_tail_2026_10_12.md)。`9ea8`の採用済み共有方式をmainへコミットした。[mainでの再測定・検証](checkpoint/list_runtime_rt5_builder_adoption_2026_10_13.md)を追加した。OI-039の遅化/RSS原因は残る。

## 現行の契約と参照先

- List の表現・仮想計算量・継続の所有権: [Eldr VM 仕様](../../../docs/dev/EldrVM_spec.md)。
- process / Task の待機・取消・結果配送: [Process Runtime 仕様](../../../docs/dev/ProcessRuntime_spec.md)。
- generic do の型検査・lowering: [do intrinsic 仕様](../../../docs/dev/Do_intrinsic_spec.md)。
- 公開 API と `Monad::bind`: [List の標準定義](../../../lib/types/list.srt)。
- ListHandle: [runtime.rs](../../../crates/sindr/src/runtime.rs)。Builder: [list_builder.rs](../../../crates/eldr/src/builtin/list_builder.rs)。
- List 操作の回帰基準: [標準テスト](../../../lib/tests/monads/list.srt)。
- 予算・操作数・失敗・rollback の回帰基準: [VM テスト](../../../crates/eldr/src/vm/list_flat_map_tests.rs)、[REPL テスト](../../../crates/xldr/tests/repl_core.rs)。

改善時も、論理順、永続性、mapper の呼出し回数、失敗後の未評価、source trace、
待機・取消・rollback を維持する。通常の実行切替えでは Builder を移動し、checkpoint は独立した保存状態を持つ。

## Runtime の追加調査・最適化

### RT-2 Packed の使用済み先頭部分の保持

- 現状: 小さい suffix だけを保持しても、元の `Rc<Vec<Value>>` 全体が生存する。最後の参照の破棄では buffer 全体の解放が発生し得る。
- 未確定点: chunk 化、明示的な compaction、共有を続ける方式のどれを採用するか。保持量とコピー量の交換条件を先に測る。
- 測定済み: [保持方式比較](list/list_tail_retention_comparison_2026_10_11.md)で、共有継続・chunk・明示的 compaction を同じ入力で比較した。短い suffix の保持量は減るが、元 List を checkpoint が保持している間は全体を解放できない。製品の最適化は未着手。
- 次の作業: 比較で確認した所有条件を基に採用方式と製品への移行仕様を決める。chunk は Builder / `from_items` からの所有権移動、compaction は入口と対象 suffix、予算・取消・rollback を具体化する。
- 受け入れ条件: `cons` / `head` / `tail` / `uncons` / `len` の仮想的な O(1)、永続性、論理順を保つ。tail のたびに全要素をコピーする経路を導入しない。改善対象と、保持量が改善しないケースを記録する。

### RT-4 未分割 builtin と外部呼出し

- 現状: callback と flat_map の入力・出力処理は共通予算に従う。既存の純粋な Rust loop、regex / JSON、OS I/O の一回の処理には長い停止が残り得る。
- 未確定点: 各処理を既存 continuation へ分割するか、ライブラリの分割 API、非同期 I/O、worker への移管を使うか。
- 次の作業: 入力サイズに比例する builtin と外部呼出しを棚卸しし、一回の処理量、取消、資源の後処理を個別に定義する。
- 改善済み: [HashMapのコピー削減](hashmap/hashmap_runtime_optimization_2026_10_11.md)で、所有入力の構築、VM map_from_entries、map_valuesの途中出力から反復追加時の既存entry全cloneを除いた。[基準値](hashmap/hashmap_runtime_measurement_2026_10_11.md)と同じ条件で再測定し、途中outputの独立性、待機・取消・checkpoint復帰を検証した。開始時のclone / sortとRust loopは未分割で、次はその分割単位と通常入口のfairnessを仕様化する。
- 受け入れ条件: 移行した処理が完了前に他の runnable task へ切り替わり、副作用・結果配送を重複させない。CPU yield と Future 待機を区別し、batch / REPL / Task / process の通常入口から確認する。
- 制限: 処理を分割した範囲を明示する。未移行の処理、Value の物理操作、allocator を含む VM 全体の実時間上限は、別途根拠が必要。

### RT-5 REPL checkpoint の残るコピー量

- 現状: process・future・detached taskの3表とentryをcheckpointで共有し、変更時に表のindexと対象entryを複製する方式へ移行した。Builderはappend専用segmentを共有し、保存済みValueの全cloneを除いた。通常の実行切替えでは引き続き複製しない。
- 改善済み: process 定義の3表を不変 registry にまとめ、checkpoint / VM clone で共有する。[metadata コピー削減](checkpoint/list_runtime_rt5_metadata_optimization_2026_10_12.md)に実装・測定・検証を記録した。
- 改善済み: [残る管理表・queueの共有化](checkpoint/list_runtime_rt5_remaining_optimization_2026_10_12.md)で、handler/supervisor metadata、singleton/flight/refilling/reply/waiting表、deadline/run/output queueを共有し、worker setは表とentryの共有方式へ移行した。新定義なしchunkのregistry再構築を除き、boot plan追加分のrollback漏れも修復した。
- 残る範囲: 保存中に変更する容器・表index・entryの複製、共有Builder完成時のValue clone、新定義登録時のregistry構築と、共有化後の反復更新の追加費用が残る。runtime全体の定数時間保存・変更を保証しない。
- 改善済み: [Builder共有化のmain反映](checkpoint/list_runtime_rt5_builder_adoption_2026_10_13.md)で、保存後の初回変更時の全Valueコピーを除き、共通の予算付き完成処理を取り込んだ。通常経路とInt RSS差は解決していない。
- 次の作業: 未変更容器の共有化は実装・測定を完了した。長いBuilderの測定と費用の一部分離は[9ea8の未統合資料](checkpoint/list_runtime_rt5_builder_measurement_2026_10_12.md)を参照する。通常経路の追加費用とallocator/OSの内訳はOI-039に残る。稼働process・future・override・chunk数は別途分け、必要な初回変更のコピー量と実利用の時間/メモリを測る。
- 受け入れ条件: 失敗したchunkの進捗を破棄して保存位置から再開し、その位置より前のcallbackを呼び直さず、復帰先へ一度だけ結果を渡す。実行中に変更する可変Builderは保存状態から独立させる。失敗したchunkの外部I/Oは巻き戻さない。

## 今後の検証と文書更新

着手する項目の契約を直接検証するテストから始め、影響範囲に応じて
[テスト方針](../../../docs/dev/テスト方針.md) の全体 gate と独立レビューを実施する。
解決した項目は本書から削除し、確定した契約を対応する正本へ移す。
