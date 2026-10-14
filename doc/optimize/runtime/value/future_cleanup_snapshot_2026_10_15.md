# Future収集のcleanup検索と未解決ID

入力は[残り6分野の実施計画](../remaining_implementation_2026_10_14.md)。基準は `8a5f7505`。Eldr内で配送契約を変えないlevel2改善。

普通値だけのguardと、Running / unknownだけのheap確保0を維持する。raw Ready / Cancelledを既存の参照mapへ収集してから、cleanup表を一回走査し配送不可IDを除く。個々のready IDごとの全task走査を除去する。raw-readyだがcleanup待ちのIDも収集mapで重複確認を省く。ただし全てcleanup待ちでも一時mapを確保する点を交換条件として計測する。

Running / unknownの直近2 IDは収集内のinline cacheへ保存する。一律の小さいFIFO cacheで、heap確保・永続metadata・COW費用は増やさない。3件以上の再訪では再確認が残り、全未解決IDを保存する方式より削減が小さい。全IDをheap mapへ保存する案は、all-Runningの確保0を失うため採用しない。cacheは一収集中だけ有効で、次回のready化・cleanup終了・context切替え・rollbackを隠さない。

一段置換、Cancelled、unknown、producer cleanup、待機通知、保存枝と各slotの独立cloneを既存直接テストと再訪・追い出しの受入テストで固定する。実収集一回を同一manual fixtureで測り、準備・全内容検証を確保/時間から分ける。Task/CLIとOI-037のallocator原因は別の系列として扱う。

## 実装と検証

TDDは未解決IDの確認3→2回と、ready ID混在時のcleanup表訪問8,448→33回の2件でRedを確認した。実装後の関連43件が成功。cache追い出し・次回のready化、保存枝とTuple配送の独立性、Cancelled・unknown・一段置換・producer cleanup後の待機通知を維持した。個別Taskのready判定は共通cleanup判定のままであり、この変更は一括stack収集に閉じる。

最終workspace CI 2,586件・標準1,124件すべて成功、失敗・除外・未実行0。[検証記録](future_cleanup_snapshot_2026_10_15.validation.json)。新しいmetadata field、公開API、Opcode、builtin ID、VM/schema versionは加えていない。参照map内のcleanup待ちの値は配送せず、cleanup表の全条件を除外した後にだけ各slotへ一回cloneする。

## 同じfixtureの費用

[原本](future_cleanup_snapshot_2026_10_15.jsonl)・[集計](future_cleanup_snapshot_2026_10_15.summary.json)へ、全内容確認152 process、確保/時間各912 process、計1,976 processを保存した。76条件、前後6 sampleずつAB / BA同数。Ready / Running / blocked / unknown / 普通値、反復 / 2 ID交互 / 全ID別、256 / 4,096位置、0 / 64要素Tuple、無関係cleanup 0 / 32 taskを区別した。cleanup条件はcaseごとにsummaryを参照する。

基準8a5f7505へ[観測patch](future_cleanup_snapshot_2026_10_15_baseline.patch)だけを加え、同じfixture・cfg(test)訪問counter・System wrapperを用いた。受入期待は基準ではRedなので、旧binary全件をGreenとは扱わずmanual testだけを実行した。前後の独立release test artifact・全source・binary・runner hashを照合した。準備・検証をInstantとallocator scopeに含めず、buildや他testを測定へ重ねていない。

4,096位置、0要素Tupleでの時間中央値とscope内要求量は以下。

| 条件 | 時間 before → after (ms) | 要求量 before → after (bytes) |
|---|---:|---:|
| Ready全ID別、無関係cleanup 32 task | 0.842 → 0.483 | 278,556 → 278,556 |
| blocked全ID別、producer 4,096 task | 25.962 → 0.595 | 0 → 278,556 |
| blocked 2 ID交互、producer 2 task | 0.039 → 0.046 | 0 → 76 |
| Running 2 ID交互、cleanupなし | 0.016 → 0.007 | 0 → 0 |
| Running同じID反復、cleanupなし | 0.005 → 0.006 | 0 → 0 |
| Running全ID別、cleanupなし | 0.120 → 0.132 | 0 → 0 |

全ID別blockedのcleanup訪問は8,390,656→4,096回、Ready全ID別・無関係32 taskは131,072→32回となった。小さいcleanup表や同じIDではmap filteringの費用が上回る場合があり、全条件の高速化は主張しない。Running全ID別の範囲は0.119–0.123 / 0.130–0.134 ms。普通値・Running・unknownは全条件でheap要求0、raw-ready全blockedは一時mapを作り、その純増は0だった。Readyのscope内要求−解放はslotへ配送したTuple bufferのlayoutに一致する。物理copy・allocator実使用量・RSSはこの内部系列では測っていない。

## 通常CLI

[原本](future_cleanup_batch_2026_10_15.jsonl)・[集計](future_cleanup_batch_2026_10_15.summary.json)は、先のRegex最終Rune binaryが8a5f7505 archiveの全compiler/runtime/lib source hashと一致することを確認して基準に再利用した。新Rune artifactは独立targetへ保存し、48 cached fresh processの出力を確認した。warmupは測定外、全測定がartifact cache hit。

10,000要素のFutureなしexecuteは1.581→1.580 ms、範囲1.571–1.590 / 1.557–1.609 ms。未await Taskは1.846→1.822 ms、範囲1.832–1.868 / 1.803–1.869 ms。両者とも範囲が重なり、通常入口の速度改善は確認していない。RSS中央値も前後の範囲が重なる。compile・cache読込みを含むtotal、parent process wall、executeは別に残した。OI-037の旧HashMap入力とallocator原因はこの系列とは別に継続する。
