# OI-036: 未解決Futureの位置を追跡する

入力は[runtime一覧](../README.md)と[前回の残件](future_stack_nonempty_optimization_2026_10_13.md#残件)。基準はmain `187fc6b4`。未解決PendingFutureが一つ残るだけで通常値を毎opcode全走査する費用と、Futureなしのstack操作の追加費用を対象にする。既存の配送契約を維持するEldr内のlevel2変更である。

## 方式と受入条件

件数を、top-level PendingFutureのslot位置を昇順に持つprivate Vecへ置き換える。push / pop / truncate / clear / ready置換が値と位置を一緒に更新する。空Vecはheap確保しない。任意の可変Valueアクセスは引き続き公開しない。位置の不一致を全走査で隠すfallbackは設けない。

ready収集は追跡したslotだけを読み、従来と同じ`ready_future_value`で判定する。一回の収集結果を固定してから該当slotを置換し、同一IDの重複slot、Cancelled、cleanup未完了、missing ID、Futureを返すFutureを維持する。入れ子Value内のFutureは対象に含めない。context移動・clone・checkpoint / rollbackは値と位置を一緒に扱う。

- 深い通常値stackと未解決Future一つを持つList構築で、read visitsをstack総長でなく追跡slot数とopcode数の積に抑える。
- ready slotと未解決slotが混在する深いstackでも、write visitsを追跡slot数に抑える。
- push / pop / truncate / clear、重複slot、配送による位置削除とFuture値への置換後の位置保持を固定する。
- 頻繁に使う小さなstack操作のインライン化を比較し、通常入口の追加費用を測る。時間閾値はcorrectness testへ入れない。対象系列で改善しなければ解決済みと報告しない。

位置Vecの固定幅metadataと実Future slot分のheapは新しい費用である。Future表自体の検索、detached Taskのcleanup検索、入れ子Valueの探索、public API、fairness全般は変更しない。

## 手順と検証

1. 決定的なscan countの回帰テストを先に追加し、全走査を理由とするRedを確認する。
2. 位置追跡へ置換し、既存stack・context・rollback・cleanupテストをGreenにする。正本[EldrVM](../../../../docs/dev/EldrVM_spec.md)と[テスト方針](../../../../docs/dev/テスト方針.md)を整合させる。
3. 前後のrelease artifact・source hashを記録し、内部VMの5条件と通常Task / Listの対照を独立process・AB/BA同数で測る。全内容検証は別processにする。時間・RSS・固定幅metadataを分けて報告する。
4. 最終sourceでworkspace CIを全件一回、標準Surtrを全件一回実行し、差分を自己点検する。実測・残件を結果文書へ記録する。
