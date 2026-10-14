# OI-036: 連続する同じFutureのready確認をまとめる

入力は[runtime一覧](../README.md)と[位置表共有後の残件](future_stack_position_sharing_2026_10_14.md#残件)。基準はmain `0288964c`。Eldr内のlevel2変更として、追跡slotごとのFuture表・cleanup検索の重複を減らす。

## 方式と受入条件

ready値を収集する一回の処理内で、追跡位置の順にIDを読み、直前と同じIDの確認を省く。普通値は追跡に含まれないため、普通値を挟んだ同一IDも一つの連続区間となる。異なるIDを挟んだ同一IDは再確認する。前回のIDは処理内のローカル変数だけに置き、contextやcheckpointに追加状態を持たせない。

- 同一IDの連続区間では`ready_future_value`を一回だけ呼ぶ。Ready / Cancelled / Running / unknown ID / cleanup完了待ちに同じ条件を適用する。
- 全ての追跡slotを読み、固定した収集結果から同じFutureの全slotへ配送する。位置表と値Vecの更新契約を維持する。
- ready収集中はFuture表とdetached Task表を変更しない。配送元はcleanup完了を確認する既存の`ready_future_value`のままとする。
- Futureを返すFutureは一回の置換で一段だけ進む。別IDや次のopcode、context切替え、rollbackをまたいで確認結果を使い回さない。
- Futureなし・空Future表の早期終了を保つ。公開API・Opcode・VM/schema version、永続metadata、重複排除用の確保を増やさない。通常値の全走査やfallbackを設けない。

全IDの一意化は対象外とする。一時Setは未解決しかない入力にも確保を増やすため採用しない。値の配送clone、異なるIDを挟んだ再確認、非重複入力の比較費用は残る。

## 手順と検証

crate-localの呼出しcounterで同一IDの確認回数を固定し、現行のslotごとの確認でRedを確かめる。連続区間だけをまとめ、既存のcleanup・context・checkpoint境界とともにGreenを確認する。

同じmanual testを含む前後のCargo test executableを保存し、別processで普通値に混在する1 / 256 / 4,096位置、反復ID / 交互ID、Running / Ready / Cancelled / unknown / Futureを返すFuture / PendingFutureなしの論理操作数と全値を比較する。source・binary hashと再現patchを保存する。これは操作数の検証であり、実時間・allocator要求・RSSの改善は主張しない。

最終sourceでworkspace CIと標準Surtr全件を一回確認し、EldrVM・テスト方針・一覧・残件を整合させる。自己点検後、この改善だけをコミットする。

## 実装とRed / Green

呼出しcounterをready収集の`ready_future_value`直前へ追加した。基準実装では同じIDの256 slotに256回、ID列`A A B A A`に5回の確認を行い、期待する1回・3回との差で3件が失敗した（終了コード100）。変更後は関連22件が成功した（363 skipped、終了コード0）。

製品変更はready収集のiteratorへ直前IDとの比較を加える部分だけである。cleanupを確認する関数、値の収集先HashMap、収集後の置換、位置表は変更していない。Runningやunknownの結果も同じ連続区間内だけまとめる。ID列が変われば再確認し、次の収集時には直前IDを初期化する。表は収集中に不変なので、同じIDの再確認を省いても配送結果は変わらない。

テストは普通値を挟む同一ID、Ready / Cancelled / Running / unknown、cleanup待ちの256 slotとcleanup完了後の全配送を固定した。Futureを返すFutureでは、返されたIDが既にreadyでも同じpass内で連鎖解決せず、次のpassで確認する。既存のcontext切替えとcheckpoint / rollbackも成功した。

## 内部VMの操作数

普通値と交互に置いた1 / 256 / 4,096追跡位置、6条件、反復ID / 交互ID、前後2 sampleをfresh processで検証した。計144 processで、全slotの値・件数と論理操作数を確認した。前後の順序はAB / BAとした。

4,096位置の結果を示す。普通値は別に4,096個あり、追跡位置の間に入る。

| 条件 | ready確認 前→後 | read visits 前→後 | write visits 前→後 |
|---|---:|---:|---:|
| 同一ID・Running / unknown | 4,096 → 1 | 4,096 → 4,096 | 0 → 0 |
| 同一ID・Ready / Cancelled / Future結果 | 4,096 → 1 | 4,096 → 4,096 | 4,096 → 4,096 |
| 交互ID・Running / unknown | 4,096 → 4,096 | 4,096 → 4,096 | 0 → 0 |
| 交互ID・Ready / Cancelled / Future結果 | 4,096 → 4,096 | 4,096 → 4,096 | 4,096 → 4,096 |
| PendingFutureなし | 0 → 0 | 0 → 0 | 0 → 0 |

PendingFutureなしは表の空判定も0回、ありは前後とも1回だった。ExecutionContextは912 bytes、VMは3,120 bytesで前後一致した。反復IDは各条件でready確認だけを一回にし、同じFutureの全slotを配送する。交互IDは確認回数を変えない。Future結果は最初のpass後の値を確認してから、次のpassで全解決することも検証した。

これは実際のVMを使うdebug test executableの論理操作数である。counterはready判定の関数呼出しを数え、Future表の内部比較回数、cleanup表の内部訪問数、CPU load / cache missは数えていない。実時間、確保要求、live、RSS、通常CLIは今回測っていない。非重複入力にもID比較が加わるため、全入力の高速化は主張しない。

[原本](future_stack_lookup_runs_2026_10_14.jsonl)・[集計とbuild来歴](future_stack_lookup_runs_2026_10_14.summary.json)・[変更前build](future_stack_lookup_runs_2026_10_14_before_builds.json)・[変更後build](future_stack_lookup_runs_2026_10_14_after_builds.json)。Cargoが返したeldr test executableを前後別名で保存し、source・binary hashを照合した。同じtargetを順にrebuildしており、保存したbinaryで検証中にはbuildや他のtestを行っていない。

[基準patch](future_stack_lookup_runs_2026_10_14_baseline.patch)は`0288964c`へcfg(test)のcounter・受入テスト・manual testだけを加える。独立した一時checkoutへ適用し、記録した基準sourceの全hashと一致することを確認した。再現手順は[manual probe](../../../../tests/profile/future_stack_scan/README.md#連続する同一idのready確認)を参照する。

## 最終検証

- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/)'`: 22 passed、363 skipped、終了コード0。
- 同じtargetの`rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,565 passed、69 binaries、失敗・除外0、終了コード0、nextest部分113.664秒。
- 同じtargetの`rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。failed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0、終了コード0。
- Rust workspace fmt、製品・文書・harnessの差分空白検査、Python構文、文書のローカルリンク、保存sourceとbinaryのhash、基準patchの再現を確認した。patchのcontext行は原本のまま保持する。

[検証記録](future_stack_lookup_runs_2026_10_14_validation.json)に終了コード、標準summary、Cargo artifact、最終source hashを保存した。操作数比較・CI・標準テストの間に製品sourceを変更していない。最終差分を自己点検し、表の不変性、処理内だけの直前ID、全slotへの配送、Future結果の一段置換、早期終了と既存位置表の維持を確認した。level2のため別エージェントは使っていない。

## 残件

異なるIDを挟んだ同一IDの確認、各opcodeでの未解決Futureの再確認、各連続区間のFuture表・cleanup検索、値の配送clone、位置表と値Vecの保持費用は残る。非重複入力へ加わる比較の実時間寄与も未測定である。通常CLI・RSSとOI-037は今回の対象外とする。

その後、[配送前の結果cloneを省く改善](future_ready_clone_deferral_2026_10_14.md)を行った。上記の操作数は連続IDの確認削減時点の原本として保持する。
