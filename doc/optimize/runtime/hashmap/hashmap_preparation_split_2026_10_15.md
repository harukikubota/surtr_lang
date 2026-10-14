# RT-4: HashMapの前処理と構築を継続へ分ける

入力は[残り6分野の実施計画](../remaining_implementation_2026_10_14.md)。基準はmain `b9e7a376`。公開API・キー昇順・duplicate後勝ちと元mapの永続性を維持するlevel3変更。

`map_values`は所有する引数mapのentryをVecへ移し、全entryをcloneする`sorted_entries`を呼ばない。in-place heap sortを既存のResumeへ接続し、一遷移で一heap境界、または最大2回のキー比較と一回のswapを進める。純粋な遷移は残予算内でまとめて進め、全遷移を課金する。callback・完了・失敗の配送は次の共通stepに残す。sort完了後にだけcallbackをキー昇順で一度ずつ開始する。空mapではcallbackを呼ばない。checkpointの継続Cloneは独立した途中状態を持ち、取消・RuntimeErrorで部分結果を公開しない。

`map_keys` / `map_values_list`も同じsortを使い、一遷移で一要素だけ共通Builderへ追加する。`map_from_entries`は一遷移で一entryの検査と所有するoutputへの追加を進める。malformed entryのindex付き診断、duplicate後勝ち、追加済みentryの反復cloneなしを維持する。

所有HashMapのiteratorからVecへの移動と、Valueのclone / drop・String比較・allocatorの物理費用は同期のまま。sortの全比較と構築・収集の全loopを一回で実行する旧経路を削除するが、VM全体の実時間上限は保証しない。HashMap storageの表現、builtin ID、Opcode、VM/schema versionを変更しない。

所有入力のString buffer維持とsort前の中断をRedから固定する。順序、途中sortの保存・独立再開、取消、言語Err値、malformed entryは直接テストへ置く。通常Task入口の他runnable taskへの切替えをVMと必要なsource fixtureで検証し、最終workspace CIと標準全件を一回確認する。既存HashMap manual probeで全結果を確認し、時間と確保を分けて測る。OI-038のallocator原因はこの改善とは別に調査する。

## 測定結果

[原本](hashmap_preparation_split_2026_10_15.jsonl)と[summary](hashmap_preparation_split_2026_10_15.summary.json)に、before `b9e7a376` と最終sourceのbuild・Cargo artifact・binary・source hashを保存した。6 case × 3件数 × 2 payloadの36条件で全内容確認72 process、確保と時間を分けた前後6 sampleで864 processを実行した。順序を交互にし、build・テストと重ねていない。

1,024件の条件の中央値は以下。VM区間はboot・入力構築・callback・observerを含み、sortやmap_values単独の測定ではない。要求量はallocator layoutの累計、RSSはprepare・run・dropを含むprocess全期間peakである。

| case / payload | 要求量 before → after (bytes) | 時間 before → after (ms) | peak RSS before → after (MiB) |
|---|---:|---:|---:|
| vm_distinct / Int | 2,823,062 → 2,806,686 | 0.868 → 0.845 | 5.430 → 5.484 |
| vm_distinct / String | 4,092,862 → 3,822,526 | 0.932 → 0.908 | 6.352 → 6.367 |
| vm_same / Int | 1,473,934 → 1,457,558 | 0.686 → 0.661 | 4.203 → 4.219 |
| vm_same / String | 2,236,310 → 1,965,974 | 0.715 → 0.683 | 4.625 → 4.672 |
| vm_map_values / Int | 4,815,152 → 4,618,547 | 2.060 → 2.032 | 5.563 → 5.359 |
| vm_map_values / String | 6,592,872 → 5,888,347 | 2.162 → 2.094 | 6.477 → 6.273 |

String vm_map_valuesの時間範囲はbefore 2.129–2.201、after 2.078–2.116 ms。全入力や通常CLIでの速度改善へ一般化しない。clone期待欄はinsert関連だけの導出値であり、source/sortの観測clone数ではない。run/dropの論理live収支は全条件で一致した。

旧OI-038系列と今回の基準は異なる。構築のRSS小幅増とmap_valuesの減少を観測したが、allocator / OSの原因を特定した値ではない。OI-038は継続する。予備のunbatched継続のone-shot値、import修正前のbuildはこの採用系列から除外した。

## 検証

所有String bufferを移し、sort前にResumeする受入テストは旧経路で意図したRedを確認した。順序・途中保存の独立再開・予算0/1/256・取消・言語Err値・malformed entryの直接テストを確認した。Task fixtureは `started`、`mapped`、`Ok(1000)` の順を固定し、通常入口でも前処理中に切り替わることを確認した。

最終Eldr 398件、workspace CI 2,581件成功。標準全件もexit 0。quiet出力に件数はない。[検証記録](hashmap_preparation_split_2026_10_15.validation.json)を参照。途中のmissing importとfixtureのrange終端誤りは修正済みで、意図したRedには含めない。独立レビューを必要とするlevel4変更は含まない。

