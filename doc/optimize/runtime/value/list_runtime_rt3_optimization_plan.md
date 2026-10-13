# RT-3 の最適化方式と実装計画

状態: 実施済み。以下の現状と手順は着手時の計画を示す。結果は [最適化レポート](list_runtime_rt3_optimization_2026_10_11.md) を参照。

入力は [List runtime の測定レポート](../baseline/list_runtime_measurement_report_2026_10_11.md) のRT-3と、
その後の「RT-3の方式選定・製品最適化まで」という依頼。
現在の基準値は [Valueと実入口の測定](list_value_cost_measurement_2026_10_11.md)。
言語仕様と外部の実行結果を維持する、Eldr内の局所最適化（level2）として進める。

## 改善対象と方式選定

Futureを使わないList workloadで、opcodeごとの不要なoperand stack走査を除く。
現在の `resolve_ready_pending_stack_values` は、解決できるFutureがない場合も
stackの読取りと書込み側の2回の全走査を行う。
手組みhuge/intではcallbackが1回のまま、n=1,000から10,000に増やすとrunが0.760から82.906 msに増えた。
n個の値を積んでListにする入力では、このstack走査の要素数がΘ(n²)になる。
観測時間の全てをこの原因へ帰属するものではない。

追加保持メモリは0を条件とする。新しい追跡表、件数、予約容量、共有表現を導入しない。

| 方式 | 効果・影響 | 判断 |
|---|---|---|
| Future表が空なら走査を省く。解決値の表が空なら書込み側を省く | 実表だけで解決結果なしを証明でき、新しい状態を持たない | 採用 |
| PendingFutureのslot / 件数を常時追跡する | 非空Future表でも不要な走査を除けるが、全stack変更・context・checkpointとの同期が必要 | 今回は採用しない |
| stackを一巡して各slotを直接置換する | 一時HashMapも除けるが、FutureありのValue clone / drop順まで変わる | Futureありの測定を先に行う |
| Tuple / HashMap共有化、Listを消費するcursor | 再帰cloneを減らせる可能性があるが、共有・suffix・checkpointの所有契約まで広がる | 今回は採用しない |
| Builderの先行reserve | reallocを減らせる可能性があるが、空結果・取消時の保持量が増える | 追加保持メモリ0を満たさない |

flat_map入口のCallable所有権移動など、今回の入力で支配的と確認できない変更を同時に加えない。
RT-2のList保持方式、RT-4の未分割builtin、RT-5のcheckpointコピーは対象外。

## 実装する条件

`VM::resolve_ready_pending_stack_values` だけに次の条件を加える。

1. `process_runtime.futures.is_empty()` ならstackを読まずに戻る。
2. それ以外は現行の `ready_future_value` で解決値を収集する。
3. 収集した解決値が空なら書込み側loopへ進まない。
4. 解決値があれば現行どおり、同じFutureを持つ全slotを置換する。

`raw_ready_future_value` はFuture表を唯一の値源とするため、表が空なら全idで値がない。
`ready_future_value` はさらにdetached taskのcleanup完了を確認する。この判定を省略しない。
解決値が空の書込み側loopは何も変更しないため、除いても結果は同じになる。
同期処理中に別threadがFuture表を書き換える経路はない。

LoadLocalの待機・local更新・内部Future解放、通常のopcode、callback、予算、取消、context切替え、
checkpoint / rollback、source traceは維持する。VM / schema versionを上げない。

Future表が非空なら最初の読取り走査は残る。無関係なFutureが残る深いstackまで
定数時間にする最適化ではない。残る条件はOI-036で追跡する。

## 受入条件と手順

1. テスト専用の訪問数計測で、空Future表の深いstackが読取り・書込みとも0になることを固定する。
   Future表が非空でも解決値がなければ、書込みの訪問数は0にする。現行コードで狙った失敗を確認する。
2. Running / Ready / Cancelled、同一Futureの複数slot、cleanup前後、context切替え、rollbackの結果を維持する。
   既存テストで守れる契約は再利用し、新しい状態境界だけcrate-localに追加する。
3. 最小の実装で対象テストを成功させ、Eldr全体を確認する。
4. 同じharness・依存版・条件・5 sampleで前後を測る。B / E / M / 手組みcallback完了数、
   requested / live / peak、RSS、全内容検証を照合する。時間やRSSの閾値はcorrectness testに入れない。
5. `rtk cargo nextest run --profile ci --workspace` と `rtk proxy cargo run -- test --quiet --all` を逐次実行する。
   ソース変更を止めた状態で全体gateを行い、Green後にAstraへ最終差分と検証結果を渡す。
6. 選定方式、前後の結果、改善しない条件をレポートへ残し、RT-3とOI-036の状態を実施範囲に合わせて更新する。

製品コードには訪問数のカウンタを持たせず、`cfg(test)` の計測だけで決定的な操作数を確認する。
実測で問題やデグレが見つかった場合はAstraへ相談し、原因を固定してから修正する。
