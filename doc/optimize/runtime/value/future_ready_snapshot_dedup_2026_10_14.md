# OI-036: 収集済みのready Futureを再確認しない

入力は[runtime一覧](../README.md)と[待機判定の借用後の残件](future_readiness_borrow_2026_10_14.md#残件)。基準はmain `cabbcf9e`。Eldr内のlevel2変更として、別IDを挟んで再出現する配送可能Futureの検索と収集用cloneを減らす。

## 方式と受入条件

一回のready収集で、既存のready mapにあるIDは再確認しない。直前と同じIDを省く処理は維持し、Running / unknown / cleanup待ちの連続区間にも適用する。Noneを新しい容器へ保存せず、別IDを挟む未解決IDは再確認する。

- Ready / Cancelled / Futureを返すFutureは、同じIDが一回の収集中に何度出現してもready判定と収集用のsource Value cloneを一回にする。
- 配送元は既存のcleanup条件を含む`ready_future_value`とする。Future表・cleanup表は収集中に変更せず、収集結果を固定して全slotへ配送する。
- Running / unknown / cleanup待ちをready mapへ入れない。連続する同一IDだけを省く既存の条件は維持する。
- Futureを返すFutureは一段だけ置換し、次のpassでは新しく確認する。元の表・保存stack・各配送slotの独立性、context切替え、取消、checkpoint / rollbackを維持する。
- 既存のready map以外にSetやmapを追加しない。キャッシュは一回の収集内だけに置き、公開API・Opcode・VM/schema version・永続metadata・fallbackを増やさない。Futureなし・空Future表の早期終了を保つ。

新しいIDや未解決IDにもready mapの有無の検査が加わる。各slotへのcloneと未解決Futureの再確認は対象外とし、clone回数だけから全入力の高速化を主張しない。

## 手順と検証

既存のFuture結果の一段置換テストを拡張し、別IDを挟むready IDの再確認とsource cloneでRedを確認する。未解決・unknown・cleanup待ちを収集済みと扱わない境界と、複製slot・保存枝の独立性を直接検証する。既存の追跡位置・context・rollbackも確認する。

同じmanual testを持つ前後のCargo executableで、1 / 256 / 4,096追跡位置、反復 / 2 ID交互 / 全ID別の配置、Ready / Cancelled / Running / unknown / Future結果 / PendingFutureなしをfresh processで比較する。ready判定とsource clone回数、全slotの値を検証し、source・binary hashと基準patchを残す。実時間・allocator要求・RSSは測定なしに改善を主張しない。

最終sourceでworkspace CIと標準Surtr全件を一回確認し、EldrVM・テスト方針・一覧・残件を更新する。自己点検後、この改善だけをコミットする。

## 実装とRed / Green

基準のFuture結果テストでは、ID列`A A B A A`のready確認とsource cloneが各3回だった。期待する各2回との差で失敗した。readyと未解決のIDを交互に置くテストも、確認5回・source clone 3回に対し期待3回・1回で失敗した（2件、終了コード100）。変更後はFuture stack・位置表・Task継続・process停止の関連56件が成功した（335 skipped、終了コード0）。

ready収集を、直前IDと既存ready mapを確認するloopへ置き換えた。直前と同じIDなら判定を省き、ready mapに収集済みなら検索と収集用cloneも省く。未収集IDは既存のcleanup条件を含む配送用関数で調べ、Someだけをmapへ入れる。置換は全IDを収集した後に行い、値・位置の更新経路は変えていない。

新しいテストはRunning / unknown / cleanup待ちをmapへ入れず、別IDを挟んで再確認することを固定した。Tuple結果を全ready slotへ配送して保存枝を保ち、一つの配送Tupleを変更しても別slotとFuture表の元の値が変わらないことを確認した。既存のFuture結果テストは、両方のFutureがreadyでも一段だけ置換し、次のpassで返されたIDを再確認する。

## 内部VMの操作数

既存のmanual testに全ID別の配置とsource clone回数を加えた。同じ前後のtest sourceで、1 / 256 / 4,096追跡位置、6条件、3配置、前後2 sampleをAB / BA順のfresh processで検証した。計216 processで全slotの値・件数、一段ずつのFuture結果、read / write visitsも確認した。普通値は追跡slot間に一つずつ置く。

4,096追跡位置の結果を示す。「配送可能」はReady / Cancelled / Future結果の三条件を指す。

| 配置・条件 | ready確認 前→後 | 収集用source clone 前→後 | read / write visits 前→後 |
|---|---:|---:|---:|
| 同じID反復・配送可能 | 1 → 1 | 1 → 1 | 4,096 / 4,096 → 同じ |
| 2 ID交互・配送可能 | 4,096 → 2 | 4,096 → 2 | 4,096 / 4,096 → 同じ |
| 全ID別・配送可能 | 4,096 → 4,096 | 4,096 → 4,096 | 4,096 / 4,096 → 同じ |
| 2 ID交互・Running / unknown | 4,096 → 4,096 | 0 → 0 | 4,096 / 0 → 同じ |
| 全ID別・Running / unknown | 4,096 → 4,096 | 0 → 0 | 4,096 / 0 → 同じ |
| PendingFutureなし | 0 → 0 | 0 → 0 | 0 / 0 → 同じ |

同じIDのRunning / unknown反復では前後とも確認1回・source clone 0回だった。PendingFutureなしは表の空判定も0回、ありは前後とも1回である。ExecutionContextは912 bytes、VMは3,120 bytesで前後一致した。

counterはready判定の関数呼出しと、Future表から収集候補を作る元のValueのcloneを数える。表の内部比較、cleanup表の訪問数、入れ子のclone、各slotへの配送cloneは数えていない。実時間・allocator要求・live・RSS・通常CLIは未測定である。新しいIDと別IDを挟む未解決IDにはready mapの検査が加わり、一般の高速化・メモリ使用量の改善率は主張しない。

[原本](future_ready_snapshot_dedup_2026_10_14.jsonl)・[集計とbuild来歴](future_ready_snapshot_dedup_2026_10_14.summary.json)・[変更前build](future_ready_snapshot_dedup_2026_10_14_before_builds.json)・[変更後build](future_ready_snapshot_dedup_2026_10_14_after_builds.json)。同じtargetを逐次buildし、Cargoが返すtest executableを前後別名で保存した。保存したsourceとbinaryのhashを確認し、操作数比較中に他のbuild・testを行っていない。前後で異なるsourceはVMの収集処理だけで、test / manual sourceは同一である。

[基準patch](future_ready_snapshot_dedup_2026_10_14_baseline.patch)は`cabbcf9e`へ受入テストの更新・追加とmanual testの拡張だけを加え、基準の製品経路を変えない。独立した一時checkoutへ適用し、69 source / manifest / lockのhashが記録と一致することを確認した。再現手順は[manual probe](../../../../tests/profile/future_stack_scan/README.md#収集済みready-idの再利用)を参照する。

## 最終検証

- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/) | test(/process_continuation::tests/) | test(/process_stop_tests/)'`: 56 passed、335 skipped、終了コード0。
- 同じtargetの`rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,571 passed、69 binaries、失敗・除外0、終了コード0、nextest部分126.822秒。
- 同じtargetの`rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。failed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0、終了コード0。
- Rust workspace fmt、製品・文書・harnessの差分空白検査、Python構文、文書のローカルリンク、保存sourceとbinaryのhash、基準patchの再現を確認した。patchのcontext行は原本のまま保持する。

[検証記録](future_ready_snapshot_dedup_2026_10_14_validation.json)に終了コード、標準summary、Cargo artifact、最終source hashを保存した。操作数比較・CI・標準テストの間に製品sourceを変更していない。最終差分を自己点検し、既存mapだけの再利用、全slotでの直前ID更新、未解決結果の非保存、同じcleanup条件、固定収集結果と全slotへの配送、一段置換、各Tupleと保存枝の独立性、context・取消・checkpoint / rollbackを確認した。level2のため別エージェントは使っていない。

## 残件

未解決IDの各連続区間のFuture表・cleanup検索、各opcodeでの再確認、ready mapの検査費用、distinct ready IDの収集用cloneと各slotへのclone、値Vecと位置表の保持費用は残る。実時間・allocator・RSSとOI-037は未測定・未解決のままとする。
