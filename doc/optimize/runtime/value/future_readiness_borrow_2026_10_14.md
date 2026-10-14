# OI-036: 配送可能かの判定でFuture結果を複製しない

入力は[runtime一覧](../README.md)と[配送前clone削減の残件](future_ready_clone_deferral_2026_10_14.md#残件)。基準はmain `53354bdf`。Eldr内のlevel2変更として、owned値を取得して有無だけを調べる待機判定のcloneを省く。

## 方式と受入条件

cleanup完了条件を含む結果取得を、privateな`ready_future_value_ref`へまとめる。raw結果の借用と既存のdetached Task検索を行い、配送可能な結果の不変参照を返す。owned配送の`ready_future_value`はこの共通判定を使い、Someの値だけを複製する。

待機Taskの実行可能判定・選択・再開前検査、cleanup完了通知、Task起動境界、singleton waiter解放、任意Futureの待機解除では、結果の有無だけを共通の借用取得で調べる。Task自身の終了判定はcleanup開始のためraw状態を使う既存の契約を保つ。

- readyの有無だけを確認する製品経路では、Future表の元のValueをcloneしない。
- Ready / Cancelledでもproducer cleanup待ちなら待機を解除しない。Running / unknownも配送可能とみなさない。raw検査を配送可能判定の代わりに使わない。
- owned配送と判定専用の借用は同じcleanup条件を使う。配送する値は独立して所有し、元の表を消費しない。
- 待機Taskの結果を完了Futureへ配送する場合の取得cloneは一回とする。結果・完了通知・cleanupを一度だけ実施する。
- 取消、deadline、起動境界、singleton回収、Futureを返すFuture、context切替え、checkpoint / rollbackを維持する。新しいキャッシュ・永続metadata・公開API・Opcode・VM/schema version・fallbackを追加しない。

Future表・cleanup表の検索と、配送に必要なcloneは対象外とする。cleanup判定を重複実装せず、旧owned判定経路を製品に残さない。

## 手順と検証

待機FutureだけがReady / CancelledになったTaskの実行可能判定と実配送を、既存のsource Value clone counterで固定する。現行の判定cloneでRedを確認し、共通の借用判定へ置き換えてGreenを確認する。producer cleanup待ちから実行可能への遷移、Running / unknown、元の値と配送値も検証する。

同じmanual testを持つCargoの前後executableを保存し、1 / 256 / 4,096回のTask実行可能判定、0 / 64要素Tuple、Ready / Cancelled / Running / unknownをfresh processで比較する。cleanup待ちの選択・通知境界はcrate-local受入テストで確認する。clone回数と値・Taskの状態遷移を検証し、source・binary hashと再現patchを残す。実時間・allocator要求・RSSの改善は測定なしに主張しない。

最終sourceでworkspace CIと標準Surtr全件を一回確認し、EldrVM・テスト方針・一覧・残件を更新する。自己点検後、この改善だけをコミットする。

## 実装とRed / Green

既存のFuture表のsource Value clone counterを使った受入テストは、待機Taskの実行可能判定とcleanup完了通知が各1回cloneするため、期待0回との差で2件が失敗した（終了コード100）。変更後はFuture stack・位置表・Task継続・process停止の関連55件が成功した（335 skipped、終了コード0）。

cleanup判定は借用取得に一箇所だけ置き、owned取得はそこから返る値をcloneする。判定だけの製品呼出し8箇所を借用取得へ置き換えた。Task終了判定のraw検査と`error_generation_pending`条件は維持し、producer自身のcleanupを進められるようにした。旧owned判定経路や別のcleanup条件は残していない。

Ready / Cancelledの待機Taskは、判定だけではcloneせず、driver内の実配送で一回cloneする。配送後に同じdriverを再実行しても完了を繰り返さない。producer cleanup待ちの受入テストでは、選択時に待機Taskを選ばず、producer recordを取り除いて完了通知を行った後の次のdriverで配送することを確認した。cleanup通知のために値をcloneしない。

## 内部VMのclone回数

製品の`start_await_task`で登録した待機Taskを使い、1 / 256 / 4,096回の`has_runnable_detached_tasks`、0 / 64要素Tuple、4条件、前後2 sampleをAB / BA順のfresh processで比較した。計96 processで、判定と一回のdriver配送を分け、結果・元のFuture表・Task recordの除去・完了の非反復を確認した。Running / unknownでは待機Taskと未解決の完了Futureが残る。

4,096回の確認におけるFuture表内の元のValueのclone回数を示す。Tupleの要素数0 / 64で同じ結果だった。

| 条件 | 実行可能判定のclone 前→後 | driver内のclone 前→後 |
|---|---:|---:|
| Ready | 4,096 → 0 | 2 → 1 |
| Cancelled | 4,096 → 0 | 2 → 1 |
| Running | 0 → 0 | 0 → 0 |
| unknown | 0 → 0 | 0 → 0 |

driver内の変更前2回は選択判定と実配送の取得であり、変更後は実配送の取得だけが残る。ExecutionContextは912 bytes、VMは3,120 bytesで前後一致した。rawのReadyを理由にcleanupを省略する変更はしていない。

counterはFuture表から配送候補を取得するsource Value cloneを数える。入れ子のcloneや値の準備は含めない。実時間、allocator要求、live、RSS、通常CLIは今回測っていない。owned取得へ共通借用判定の呼出しが加わる費用も未測定で、clone回数の減少を全入力の高速化へ読み替えない。

[原本](future_readiness_borrow_2026_10_14.jsonl)・[集計とbuild来歴](future_readiness_borrow_2026_10_14.summary.json)・[変更前build](future_readiness_borrow_2026_10_14_before_builds.json)・[変更後build](future_readiness_borrow_2026_10_14_after_builds.json)。同じtargetを逐次buildし、Cargoが返すtest executableを前後別名で保存した。保存したsourceとbinaryのhashを確認し、操作数比較中には他のbuild・testを行っていない。前後のtest / manual sourceは同一で、異なるsourceはVMとTask継続の製品2ファイルだけである。

[基準patch](future_readiness_borrow_2026_10_14_baseline.patch)は`53354bdf`へcrate-local受入テスト・manual testだけを加え、基準の製品経路を変えない。独立した一時checkoutへ適用し、69 source / manifest / lockのhashが記録と一致することを確認した。再現手順は[manual probe](../../../../tests/profile/future_stack_scan/README.md#待機taskの判定と配送)を参照する。

## 最終検証

- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/) | test(/process_continuation::tests/) | test(/process_stop_tests/)'`: 55 passed、335 skipped、終了コード0。
- 同じtargetの`rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,570 passed、69 binaries、失敗・除外0、終了コード0、nextest部分116.967秒。
- 同じtargetの`rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。failed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0、終了コード0。
- Rust workspace fmt、製品・文書・harnessの差分空白検査、Python構文、文書のローカルリンク、保存sourceとbinaryのhash、基準patchの再現を確認した。patchのcontext行は原本のまま保持する。

[検証記録](future_readiness_borrow_2026_10_14_validation.json)に終了コード、標準summary、Cargo artifact、最終source hashを保存した。操作数比較・CI・標準テストの間に製品sourceを変更していない。最終差分を自己点検し、共通のcleanup判定、owned配送の独立性、判定だけの旧owned経路の除去、producer cleanupの選択境界、Task終了のraw契約、取消・deadline・起動境界・singleton回収・context・checkpoint / rollbackを確認した。level2のため別エージェントは使っていない。

## 残件

Future表とcleanup表の検索、owned配送のclone、各slotへの配送clone、値Vecと位置表の保持費用は残る。共通借用判定の呼出しの実時間寄与、通常CLI・allocator・RSSとOI-037は未測定・未解決のままとする。
