# HashMap反復構築のコピー削減結果

状態: 製品のコピー削減、局所・全体検証、再測定、交互追試、Astraレビューを完了。
対象: `547c6a68f1f1186f64cb582f1d303d6c59d12e29`上の未コミット差分。
入力は[改善前の基準値](hashmap_runtime_measurement_2026_10_11.md)と、改善実施依頼。
採用範囲・受入条件・検証手順は[実装計画](hashmap_runtime_optimization_plan.md)。

## 変更内容

以下の3経路で、完成前のmapを内部所有storageへ直接追加する方式に置き換えた。
追加ごとに公開の永続的insertを呼び、既存entry全体をcloneする旧構築経路は削除した。

- `HashMapHandle::from_entries`: 入力Vecからkey / Valueを移動する。
- VMの`map_from_entries`: 入力検査・iteratorを保ち、検査済みentryを局所mapへ追加する。
- `MapValuesContinuation::resume`: callback結果を所有する途中outputへ追加する。

公開insert / removeの永続性、duplicate keyの後勝ち、キー昇順、callbackの一回実行、
言語Err値の保存、RuntimeError伝播は維持する。途中mapは完了するまで公開しない。
既存の所有HashMapとCloneを使い、通常のcontext切替えでは移動し、checkpointの保存状態を独立させる。
継続の構造、budget境界、標準API、builtin ID、Opcode、VM/schema versionは変えていない。
2クレートにまたがる既存契約の内部改善としてlevel3で実施した。

Astraの実装前レビューで、`collect()`による入力件数を基にした事前確保は、
重複キーが多いと最終mapの容量を増やし得ると指摘された。
空mapから実際のdistinct数に応じて成長するloopを採用し、その経路は導入していない。

## 同条件の前後測定

改善前と同じ36条件（6 workload × 3件数 × 2 payload）を、count / time各5回の
独立processで測った。改善後も計360 process、1,080区間、216集計。
prepare / run / drop、payload、key長、observer条件は同じ。probeの変更はclone期待値の0への更新と
製品source / 差分hashの記録であり、workloadや計測区間は変えていない。
root lockと同版の依存を使い、after専用target-dirで`--locked` release buildした。
改善前binaryと原本は上書きしていない。

時間はcount無効系列、累積requestedはcount有効系列の各5回の中央値。
requestedはallocator layoutの要求量であり、物理コピーbytesやRSSではない。
以下はn=1,024。map_valuesのVM系列は入力mapの構築も含む全体値。

| workload | payload | run ms 前 → 後 | 累積requested MiB 前 → 後 |
|---|---|---:|---:|
| Rust distinct構築 | Int | 26.434 → 0.107 | 142.474 → 0.628 |
| Rust distinct構築 | String | 31.385 → 0.123 | 266.361 → 0.628 |
| Rust same-key構築 | Int | 0.149 → 0.043 | 0.652 → 0.001 |
| Rust same-key構築 | String | 0.149 → 0.044 | 0.894 → 0.001 |
| clone + sort | Int | 0.113 → 0.117 | 0.328 → 0.328 |
| clone + sort | String | 0.174 → 0.136 | 0.570 → 0.570 |
| VM distinct構築 | Int | 27.197 → 0.932 | 144.405 → 2.559 |
| VM distinct構築 | String | 31.298 → 0.974 | 269.502 → 3.770 |
| VM same-key構築 | Int | 0.770 → 0.733 | 1.924 → 1.273 |
| VM same-key構築 | String | 0.787 → 0.821 | 2.893 → 2.000 |
| VM distinct構築 + map_values | Int | 54.522 → 2.205 | 288.150 → 4.459 |
| VM distinct構築 + map_values | String | 62.268 → 2.205 | 537.619 → 6.154 |

StringのVM構築＋map_valuesでは累積requestedが約98.9%減った。
この条件の観測値であり、他のpayloadや実利用で同じ改善率を保証するものではない。
改善後のString distinct構築のrequested bytesは、n=256 / 512 / 1,024で、
Rustは164,284 / 329,156 / 658,892、VMは990,422 / 1,978,078 / 3,953,382。
VM構築＋map_valuesは1,616,496 / 3,228,800 / 6,453,392となり、従来の約4倍の増加を除けた。
開始時のsort、引数・iterator・callback、Value物理操作の費用は残る。

原本: [after count](hashmap_cost_2026_10_11_after_count.jsonl)・[集計](hashmap_cost_2026_10_11_after_count.summary.json)、
[after time](hashmap_cost_2026_10_11_after_time.jsonl)・[集計](hashmap_cost_2026_10_11_after_time.summary.json)。
再現は[harness README](../../../../tests/profile/hashmap_cost/README.md)。
base commitだけで改善後sourceを表さないため、metadataに変更製品sourceのhashと
tracked製品差分のhashを加え、未コミットの測定対象を記録した。

## 同一キーの保持量とRSS

Rust distinct Intでは、完成mapの破棄時freed bytesがn=256で86,552 → 92,648、
512で173,080 → 185,320、1,024で346,136 → 370,664となり、24×(n−2) bytes増えた。
`Int(index)`の元BigInt bufferを所有移動で保持する一方、旧経路のcloneはVecの余剰容量を縮める。
この依存実装の挙動と増分は整合する。コピーを省く際の所有payload容量の交換条件として記録し、
Int専用のcloneや縮小処理は加えない。Rust StringとVM系列の最終保持・破棄bytesは前後で一致した。

n=1,024のsame-key対照では、countのprepare / runのlive差分、最終dropのfreed bytesが前後で一致した。
Rustの最終map破棄はIntで692 bytes、Stringで916 bytes。
VM全体の破棄はIntで586,414 bytes、Stringで816,318 bytes。
出力が1件でもinput件数分のtable容量を残す変更になっていないことを、直接capacityテストと測定で確認した。

一方、time-only系列のprocess peak RSSの中央値は、VM distinct Stringで6.188 → 6.438 MiB、
VM構築＋map_values Stringで6.344 → 6.562 MiBとなった。
VM same-key Stringの時間は0.787 → 0.821 msで、改善後の5回の範囲は0.763〜1.628 msだった。
小さい時間差・RSS差も原本に残し、全条件の速度・RSS非増加とは結論しない。
これらはAstraに相談し、全体gate終了後に同じ旧・新binaryを交互に各10回追試した。
4条件×2binary×10回=80 process、240区間・24集計。
各pairの順序を交互に入れ替え、count無効、n=1,024、Stringを使った。

| workload | run ms 前 → 後 | process peak RSS MiB 前 → 後 |
|---|---:|---:|
| VM same-key構築 | 0.811 → 0.717 | 4.562 → 4.594 |
| VM distinct構築 | 31.246 → 0.971 | 6.180 → 6.430 |
| VM distinct構築 + map_values | 63.077 → 2.238 | 6.352 → 6.562 |
| clone + sort対照 | 0.135 → 0.137 | 3.422 → 3.438 |

same-keyの時間中央値増加は再現しなかった。元5回の値は除外しない。
distinctのRSSは0.250 MiB増え、旧範囲6.156〜6.219 MiBと新範囲6.422〜6.453 MiBは
重ならなかった。論理liveの一致やpeak低下はこのRSS差の否定にならない。
原因は未確定で、allocatorのpage保持やbinary配置に起因すると断定しない。
[OI-038](../../../open-issues.md#oi-038-hashmapコピー削減後のprocess-rss差)に再現条件と受入を残した。
既存OI-037と同じ原因だと判断する根拠はない。

追試原本は[生データ](hashmap_cost_2026_10_11_recheck.jsonl)と[集計](hashmap_cost_2026_10_11_recheck.summary.json)。
元seriesのsummary / binary hashを検査してから実行し、計測後も元原本は保持した。

## 検証

- TDD Red: 所有入力の先頭String key / Value、および最初のcallback結果のbufferを2件目追加でも保持する直接テストが、旧経路で狙ったcloneにより2 failed。3経路の置換後はGreen。
- 対象テストは17 passed、440 skipped。Sindr・Eldr全件は457 passed、失敗0。
- duplicate後勝ち・空map・小さいduplicate容量・公開更新後の旧値不変、malformed entryのindex付き拒否、非空途中outputのClone独立性を直接検証した。
- サブエージェントがVM回帰を4件追加し、小予算と予算0、キー順とcallback一回、部分output後のFuture待機・再開、取消後の未評価、checkpoint復帰時の独立性と保存以前のcallback非反復を確認した。
- 改善後の内容検証60 processと、36条件×5回×2系列の測定はすべて終了コード0。内容検証は性能原本へ混ぜていない。
- Astraコードレビューはブロッカーなし。READMEの改善前原本上書きにつながる実行例と、改善前レポートの時点表現を修正した。
- `rtk cargo nextest run --profile ci --workspace`: 2,509 passed、69 binaries、失敗0（111.728 s）。
- `rtk proxy cargo run -- test --quiet --all`: 終了コード0、標準Surtrテスト全件成功。最終製品差分で両gateを直列実行した。
- RSS / 時間差の交互追試80 processも終了コード0。Astraが改善後1,080行・216集計、追試240行・24集計、表の数値、source / diff / lock / binary / runnerのhash、追試の交互順序を独立に照合した。BigIntの容量保持と再現したRSS差を文書化し、修正必須の指摘は残っていない。レビュー側はビルド・テストを再実行していない。

## 残る範囲

RT-4のコピー削減部分だけを改善した。map_values開始時の全entry clone・sort、構築のRust loopは
未分割で、schedulerの一回の実時間上限を保証しない。
JSON object変換、regex、OS I/O、他の未分割builtin、checkpoint保存時の必要なdeep cloneは今回の対象外。
次は開始処理・構築・sortの分割単位と通常入口でのfairness / 取消を仕様化する。
