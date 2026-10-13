# Runtime 改善資料

更新日: 2026-10-13。main `72154707` の資料と、mainへ未統合の3 worktreeの資料・測定原本をここへ集約した。本文の実装済み・未着手・検証値はそれぞれ作成時点の記録であり、現在の統合状態は以下の一覧を基準にする。集約後のRegex入力共有・Builder共有化・Future stack走査削減の作業状況も一覧へ追記した。

## 項目と統合状態

| 項目 | 入口 | mainの状態 / 残件 |
|---|---|---|
| List / VM全体 | [残課題](list_runtime_followups.md)、[10月13日の進捗](list_runtime_progress_2026_10_13.md)、[元の基準測定](baseline/list_runtime_measurement_report_2026_10_11.md) | RT-2〜5の作業を横断して辿る。元の測定・棚卸しは4c18から集約した資料 |
| RT-2 Packed tail | [保持方式比較](list/list_tail_retention_comparison_2026_10_11.md)、[完成Vecのchunk移譲比較](list/list_chunk_handoff_comparison_2026_10_13.md) | 測定・モデル比較のみ。製品への保持方式採用は未決 |
| RT-3 Value / Future | [基準値](value/list_value_cost_measurement_2026_10_11.md)、[改善計画](value/list_runtime_rt3_optimization_plan.md)、[空Future表の改善](value/list_runtime_rt3_optimization_2026_10_11.md)、[非空Future表](value/future_stack_nonempty_optimization_2026_10_13.md)、[未解決slotの位置追跡](value/future_stack_index_optimization_2026_10_13.md)、[通常経路の費用削減](value/future_stack_metadata_optimization_2026_10_13.md)、[表の空判定削減](value/future_stack_guard_optimization_2026_10_13.md) | 位置追跡・metadata削減・PendingFutureなしの表参照削減をmainへコミット済み。CI 2,559件・標準1,124件成功。通常経路の小さい差・Future/cleanup検索・OI-037は残る |
| RT-4 HashMap | [基準値](hashmap/hashmap_runtime_measurement_2026_10_11.md)、[計画](hashmap/hashmap_runtime_optimization_plan.md)、[結果](hashmap/hashmap_runtime_optimization_2026_10_11.md) | 内部構築・途中outputの反復clone削減はmain統合済み。前処理の分割とOI-038のRSS差は残る |
| RT-4 Regex | [入力共有の実装・計測](regex/regex_input_sharing_2026_10_13.md)、[mainへの反映と再測定](regex/regex_input_sharing_adoption_2026_10_13.md) | 入力共有はmain `c577b32d`へコミット済み。再測定・workspace CI・標準Surtr全件成功。同期探索/収集は残る |
| RT-4 FileOutHandler | [現行mainでの再現](file-out/file_out_handler_lifecycle_reproduction_2026_10_13.md)、[修復仕様案](file-out/file_out_handler_lifecycle_repair_spec.md) | 資源寿命の既存不一致を確認。OI-040。仕様案までで修復は未実施 |
| RT-5 metadata / 管理表 | [metadata改善](checkpoint/list_runtime_rt5_metadata_optimization_2026_10_12.md)、[管理表・queue改善](checkpoint/list_runtime_rt5_remaining_optimization_2026_10_12.md)、[初期測定計画](checkpoint/list_runtime_rt5_measurement_plan.md) | registry・容器共有・boot plan rollbackはmain統合済み |
| RT-5 Builder | [基準測定](checkpoint/list_runtime_rt5_builder_measurement_2026_10_12.md)、[共有化](checkpoint/list_runtime_rt5_builder_optimization_2026_10_12.md)、[経路選択](checkpoint/list_builder_route_decision_2026_10_12.md)、[mainへの反映・再測定](checkpoint/list_runtime_rt5_builder_adoption_2026_10_13.md) | Rc segment共有化と予算付き完成処理をmainへコミット済み。再測定・workspace CI 2,548件・標準Surtr 1,124件成功。OI-039は継続 |
| OI-039 Builder追加費用 | [寄与分離](checkpoint/list_runtime_oi039_cost_2026_10_12.md)、[末尾所有比較](checkpoint/list_builder_owned_tail_2026_10_12.md)、[実VMメモリ](checkpoint/list_runtime_oi039_vm_memory_2026_10_12.md) | 通常経路の遅化/RSS差は未解決。直接所有/Box案は不採用、空Builder予約再利用修正はmain作業treeにも反映 |
| 末尾呼出し | [既存TCO記録](../001_tail_call_optimization.md) | 従来の記録は既存のoptimize配置を維持 |

未解決事項は [open-issues](../../open-issues.md) で追跡する。現在の実装契約は [EldrVM](../../../docs/dev/EldrVM_spec.md)・[Process Runtime](../../../docs/dev/ProcessRuntime_spec.md) が正本であり、計測レポートの過去の状態を現行仕様へ読み替えない。

## ディレクトリ

- `baseline/`: RT-2〜5の元の測定原本、RT-4棚卸し、Astraレビュー。
- `list/`: tail保持方式と、完成Vecのchunk移譲モデル。
- `value/`: Value clone/drop、generic do、Future stack走査の計測・改善。非空Future表の内部VM・通常Task/List入口の原本も含む。
- `hashmap/`: HashMap構築とmap_valuesの計測・改善。
- `regex/`: 入力共有、手組みVM/通常batchの原本。最初のbatch無効系列は除外表示のまま保存。
- `checkpoint/`: RT-5のmetadata・容器・Builder、OI-039。証跡ディレクトリも同じ項目に置く。
- `file-out/`: FileOutHandlerの契約不一致の再現と仕様案。

レポートと対応するJSON/JSONL原本を同じ項目に置いた。raw・summary・log・patch・証跡の内容はbyte一致で保存する。JSON内の古いworktree/binary/source pathは測定時の来歴なので書き換えない。Markdownのリンクと再現先の文書パスを移動先へ更新した。

## 取り込み元と範囲

| 取り込み元 | 採取した内容 | 扱い |
|---|---|---|
| main `72154707` | RT-2比較、RT-3、HashMap、RT-5 metadata/管理表 | 元の62ファイルを移動し、旧doc直下の配置を削除 |
| `4c18` / detached `ee13e448` | 初期計測、レビュー、棚卸し、RT-5計測計画 | 元資料をcopy。元worktreeは保持 |
| `9ea8` / `codex/rt5-builder-checkpoint-measurement`、HEAD `72154707` | 未コミットのBuilder/OI-039レポート、原本、証跡 | 資料だけcopy。その後Builderの採用方式・harnessをmain作業treeへ反映。元worktreeは保持 |
| `list-runtime-remaining` / detached `72154707` | Regex/chunk/FileOut/進捗、原本 | 資料だけcopy。その後Regex製品変更とharnessをmainへ反映・コミット。元worktreeは保持 |

集約時の未統合レポートのRustソース・新規harnessへのリンクは、実際に検証した元worktreeを参照する。Regex・Builderのmain反映後のソース・再測定は追加レポートを参照する。過去レポートの実行コマンドは取り込み元のソースを基準とし、計測原本の移動先はこのmainの資料集となる。

ローカル36ブランチをmainとの分岐点から棚卸しし、上記のdirty worktree以外に新しい未統合ランタイム最適化文書がないことを確認した。分岐前の古い資料やmainで削除済みの案は復活させていない。コンパイラ/テスト最適化は親ディレクトリの既存資料、言語機能・診断・一般監査は元の置き場を維持する。

[集約manifest](collection_manifest_2026_10_13.json)には、採取元HEAD/branch/dirty状態、各ファイルの移動元・移動先・SHA-256、参照更新、branch棚卸しを記録した。集約時は文書・原本の整理のみで、製品コード・テストは変更していない。測定harnessはHashMap再測定スクリプトの参照パス2箇所だけ新配置へ更新し、Python構文と参照先を確認した。集約時には性能測定やcompiler全体テストを再実行していない。その後のRegex・Builder再測定・全体検証は追加レポートに記録する。
