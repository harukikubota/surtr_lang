# OI-036: 未解決Futureのslotだけを読む

作業日: 2026-10-13。入力は[runtime一覧](../README.md)と[前回の残件](future_stack_nonempty_optimization_2026_10_13.md#残件)。基準はmain `187fc6b4`。[今回の方式・受入条件](future_stack_index_plan_2026_10_13.md)に沿うEldr内のlevel2変更である。

## 変更と契約

`OperandStack`の件数を、top-level PendingFutureのslot位置を昇順に持つprivate Vecへ置き換えた。push / pop / truncate / clear / ready置換が値と位置を一緒に更新する。truncateは追跡位置の境界を検索して除き、通常値のsuffixを読み直さない。位置不一致を全走査で隠すfallbackは設けない。

ready収集と書込みは追跡slotだけを訪問する。収集結果を固定してから置換するため、Futureを返すFutureの一段ずつの配送、同一IDの重複slot、Cancelled、missing ID、cleanup完了待ちを維持する。context・checkpointは値と位置を一緒に移動・clone・復元する。nested Value内のFutureは従来どおり対象に含めない。公開API、Opcode、VM/schema versionは変更していない。

## 検証

先に追加した2件は、128要素のList構築でread visits 8,387に対する期待130、通常値4,096件とFuture 3 slotの混在でread / write 4,100に対する期待3を理由に失敗した（終了コード100）。実装後は、従来の配送・context・rollback・cleanupと位置更新、空位置bufferの非確保を含む関連15件が成功した。通常値のpopが下層Futureの位置を維持する境界もList構築で通る。

## 位置追跡だけの比較

まず、位置Vecをstack内に持つ版を前回の件数版と比較した。小さなstack操作への新しいinline属性は、この系列には加えていない。内部VMは1,000 / 10,000要素×5条件×前後6 processで120 sample、全内容の別process検証20件。前後6 processはAB/BA同数で、cfg(test)の訪問counterは両版とも有効である。

10,000要素、内部VM runの中央値は次のとおり。単位はms。

| 条件 | read visits 前→後 | run ms 前→後 |
|---|---:|---:|
| empty | 0 → 0 | 1.307 → 1.421 |
| unrelated_running | 0 → 0 | 1.377 → 1.449 |
| unrelated_ready | 0 → 0 | 1.305 → 1.412 |
| pending_ready | 1 → 1 | 1.311 → 1.400 |
| pending_running | 50,015,003 → 10,002 | 36.479 → 1.506 |

readyのwrite visitsは1、未解決時は0である。未解決Futureが一つ残る条件の二次的なstack走査を除いた一方、他の条件には追加費用がある。この段階のExecutionContextは912→928 bytes、VMは3,120→3,136 bytesだった。metadataの増加と通常経路の追加費用は、次の改善対象とする。

通常のcached Task / List literal入口は、1,000 / 10,000要素×Task有無×前後10 processで80 sample、AB/BA同数、全artifact cache hitだった。10,000要素のexecute中央値はTaskなし1.396→1.498ms、Taskあり1.663→1.770ms。内部VMのrun区間には混ぜない。stdoutの件数とawait結果を全processで確認した。

[内部VM原本](future_stack_index_2026_10_13.jsonl)・[集計](future_stack_index_2026_10_13.summary.json)・[別process検証](future_stack_index_2026_10_13_verify.json)・[通常入口原本](future_stack_index_batch_2026_10_13.jsonl)・[集計](future_stack_index_batch_2026_10_13.summary.json)。[基準build](future_stack_index_2026_10_13_before_builds.json)の全source hashは`187fc6b4`と一致する。[位置追跡build](future_stack_index_2026_10_13_after_builds.json)は独立targetで保存した。source snapshotはbuild前後のhashを照合し、各測定系列でも保存binaryのhashを照合した。

局所検証は`CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/)'`で15 passed、363 skipped、終了コード0。位置Vecを持つ中間版への全件CIは重ねず、metadata改善後の最終版で実行する。

## 費用と残件

read / writeの訪問数はstack総長でなく追跡slot数に比例する。Future表とdetached Taskのcleanup検索は引き続き行う。毎opcodeのready判定自体をなくしたものではない。

位置Vecは初期状態でheap確保しない。実Futureのslot数に応じてbufferを確保し、解決・pop・clear後の容量は再利用する。contextの固定幅metadataと、Futureを含むcontext cloneにおける位置コピーが追加の費用である。requested allocation / live memoryを測らずにメモリ改善とは報告しない。
