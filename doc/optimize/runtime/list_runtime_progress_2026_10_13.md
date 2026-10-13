> 取り込み元: `/Users/haruca/.codex/worktrees/list-runtime-remaining/surtr`（detached、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# List / VM 残件の進捗

入力は [2026-10-11の指定レポート](baseline/list_runtime_measurement_report_2026_10_11.md)。main `72154707`と他worktreeの現在のソース・資料を確認し、重複を避けて残りを進めた。作業先は `/Users/haruca/.codex/worktrees/list-runtime-remaining/surtr`。main・他worktreeのソースは変更していない。コミット・統合は行っていない。

## 今回の成果

| 項目 | 進めたこと | 検証と残件 |
|---|---|---|
| RT-4 Regex | [入力全文の共有](regex/regex_input_sharing_2026_10_13.md)。find/find_all/capturesが所有StringをRcで保持し、一致間とValue cloneで全文を共有 | level3。対象8件、独立workspace CI2,528件、標準Surtr全件成功。UTF-8・span拒否・全文Eq・lifetimeを維持。探索/収集の同期処理は残る |
| RT-2 Packed tail | [完成Vecからのchunk移譲](list/list_chunk_handoff_comparison_2026_10_13.md)を、直接生成と分けて測定 | level2相当のmanualモデル。1,260process、720件の別検証。短いsuffix保持は減るが移譲時peak・線形費用が加わる。製品採用は未決 |
| RT-4 FileOutHandler | [既存不一致の現行main再現](file-out/file_out_handler_lifecycle_reproduction_2026_10_13.md)と[修復仕様案](file-out/file_out_handler_lifecycle_repair_spec.md) | 公開CLI4入力。init/openの遅れとpathname差替えを確認。level4の資源所有・canonical path・checkpoint/停止境界は未決、製品修復なし。OI-040へ整理 |

サブエージェントが実装・モデル・既存branch棚卸し・FileOut再現/仕様案を担当し、Astraが製品差分、新規の相談、測定原本、仕様案を独立確認した。修正必須指摘は残っていない。

## main と他worktreeで確認した既存進捗

RT-3のValue/generic-do測定と空Future表の走査削減、RT-4のHashMap構築/map_values途中outputコピー削減、RT-5の不変registry・管理表/queue共有とboot plan rollbackはmain統合済みだった。

`9ea8`の `codex/rt5-builder-checkpoint-measurement` には、長いBuilder測定、segment共有、OI-039の通常経路・短出力・VM memory調査、空Builder予約容量の再利用修正が未コミットで存在する。今回その差分を変更・取り込みしていない。`4c18`の指定レポート・原本・棚卸しもそのまま保持した。

## 検証の成果物を確実にする補正

通常batchの初回after binaryは、共有targetの成果物再利用により変更後runtimeの由来を確認できなかった。全体SHAだけの差を根拠にせず、実行コードsectionをAstraと照合して初回系列を除外した。

採用系列は前後を独立target・明示manifestからrelease buildし、Cargo compiler-artifactのSindr/Eldr/Rune manifestとsaved executableを対応させた。新binaryの実行コードも異なることを確認した。CIと標準Surtr全件も独立targetでやり直し、最終製品ソースに対する成功を確認した。mainの共有targetの該当core成果物を整理し、debug CLIをmainの明示manifestで再buildした。release CLIも独立してbuildしたmainのbinaryへ戻した。mainのGit作業treeはcleanで、製品ソースの統合は行っていない。

## 残件と次の一手

- RT-2: 完成済みVecの後処理とBuilderからの直接chunk構築を分け、chunkサイズ・Cons混在・予算/取消/rollbackを含む移行仕様を決める。
- RT-3: OI-036の非空Future表走査、OI-037のRSS原因は引き続き残る。
- RT-4: Regex/HashMapの前処理・Rust loop・外部I/Oの分割と通常入口のfairness、OI-040のFileOut資源所有を仕様化する。
- RT-5: `9ea8`の検証済み未コミット成果とOI-039の残る測定を、同worktreeの最新資料を基準に進める。今回のRegex改善と混ぜて統合しない。

今回のRegex・RT-2・FileOutの結果から、runtime全体の定数時間・一律の速度/RSS改善・OS待機の上限は主張しない。
