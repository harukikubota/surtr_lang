# OI-036: 配送できないFuture結果の複製を省く

入力は[runtime一覧](../README.md)と[ready確認削減後の残件](future_stack_lookup_runs_2026_10_14.md#残件)。基準はmain `922c457d`。Eldr内のlevel2変更として、Future表の結果を配送前に複製する費用を減らす。

## 方式と受入条件

現在の`raw_ready_future_value`はReady / CancelledのValueを複製して返す。`ready_future_value`はその後にdetached Taskのcleanup完了を調べ、未完了なら複製した値を捨てる。Taskの終了判定もraw結果の有無だけを使うため、結果の複製を必要としない。

privateなraw結果取得は`Option<&Value>`を返し、Future表内の不変参照を借用する。配送用の`ready_future_value`はcleanup完了後にだけValueを複製し、従来どおり`Option<Value>`を返す。借用中に表を変更する経路や新しいキャッシュは設けない。

- Ready / Cancelledでもcleanup待ちなら配送しない。Future表から配送候補を作るためのValue cloneは0回とする。
- raw結果の有無だけを調べるTask終了判定では結果を複製しない。cleanupの順序と完了条件は変えない。
- 配送可能なReady / Cancelledは従来と等しい独立したValueを一回複製する。元の表の値を消費・変更せず、重複slotと他contextへの配送を維持する。
- Running / unknownはraw・配送用ともNone、cloneは0回のままとする。Futureを返すFuture、一回の収集結果、context切替え、取消、checkpoint / rollbackを維持する。
- 公開API・Opcode・VM/schema version、永続metadata、容器確保を追加しない。旧owned raw経路やfallbackを残さない。

Future表・cleanup表の検索と、配送可能な値や各slotへの複製は対象外とする。新しい言語仕様・公平性・資源寿命の変更は行わない。

## 手順と検証

実際のFuture結果clone位置にcrate-local counterを置き、raw結果検査とcleanup待ちの余分な複製でRedを確認する。借用とcleanup後の遅延cloneへ置き換え、関連Future・Task終了・cleanup・checkpointテストでGreenを確認する。

同じmanual testを含むCargoの前後executableを保存し、1 / 256 / 4,096回の確認、空 / 64要素Tuple、raw検査 / cleanup待ち / 配送可能 / Running / unknownをfresh processで比較する。clone回数と値の一致を検証し、source・binary hashと基準patchを保存する。実時間・allocator要求・RSSの改善は測定なしに主張しない。

最終sourceでworkspace CIと標準Surtr全件を一回確認し、EldrVM・テスト方針・一覧・残件を更新する。自己点検後、この改善だけをコミットする。

## 実装とRed / Green

基準のraw取得でValueをcloneする位置へcounterを加えた。cleanup待ちのstack配送では1回、raw検査とTask終了判定では合計2回cloneし、期待0回との差で2件が失敗した（終了コード100）。借用と遅延cloneへの変更後、Future stack・位置表・Task継続・process停止の関連52件が成功した（335 skipped、終了コード0）。

raw取得はReady / Cancelled値への不変参照だけを返す。配送用関数は同じcleanup検索を行い、未完了なら値を複製せずNoneを返す。完了時には遅延評価する`then`内で一回cloneする。Task終了判定のraw `is_some`も結果を複製しない。表の検索順、cleanupの完了条件、配送用のowned値は維持した。

既存process停止テスト6箇所は、private raw取得の返却型に合わせて期待値を参照にした。caller timeoutの先行配送、calleeの継続、遅延結果による上書き防止という期待値は変えていない。受入テストはReady / Cancelledのraw検査とcleanup待ち、cleanup完了後の配送を確認し、配送したTupleを変更しても表内の元のbufferが変わらないことを固定する。借用中の表の変更はRustの借用契約で制限される。

## 内部VMのclone回数

同じmanual testを含む前後のdebug Cargo test executableで、1 / 256 / 4,096回の確認、0 / 64要素Tuple、10条件、前後2 sampleをAB / BA順のfresh processで比較した。計240 processで全結果の値、元の表の値、cleanup待ちから完了後の配送への遷移も確認した。

4,096回の確認におけるFuture表の元のValueのclone回数を示す。Tupleの要素数0 / 64で同じ結果だった。

| 条件 | Ready 前→後 | Cancelled 前→後 |
|---|---:|---:|
| raw結果の有無を検査 | 4,096 → 0 | 4,096 → 0 |
| Task終了判定 | 4,096 → 0 | 4,096 → 0 |
| cleanup待ちの配送確認 | 4,096 → 0 | 4,096 → 0 |
| 配送可能な結果の取得 | 4,096 → 4,096 | 4,096 → 4,096 |

Running / unknownは前後とも0回でNoneを維持した。ExecutionContextは912 bytes、VMは3,120 bytesで前後一致した。cleanup前に結果を配送する経路は追加していない。配送に必要なcloneは残す。

counterはFuture表内の元のValueを複製する呼出しだけを数える。Tuple内の各Valueのclone、各stack slotへの配送clone、検証用の値準備は数えていない。今回の系列は実時間、allocator要求、live、RSS、通常CLIを測っておらず、cloneの削減を一般的な時間・メモリ使用量の改善率へ読み替えない。

[原本](future_ready_clone_deferral_2026_10_14.jsonl)・[集計とbuild来歴](future_ready_clone_deferral_2026_10_14.summary.json)・[変更前build](future_ready_clone_deferral_2026_10_14_before_builds.json)・[変更後build](future_ready_clone_deferral_2026_10_14_after_builds.json)。同じtargetで前後を逐次buildし、Cargoが返すtest executableを別名で保存した。保存したsourceとbinaryのhashを確認し、操作数比較中に他のbuild・testを行っていない。manual testのsourceは前後同一である。

[基準patch](future_ready_clone_deferral_2026_10_14_baseline.patch)は`922c457d`へcfg(test)のcounter・受入テスト・manual testだけを加え、基準の製品経路を変えない。独立した一時checkoutへ適用し、基準の69 source / manifest / lockのhashが記録と一致することを確認した。再現手順は[manual probe](../../../../tests/profile/future_stack_scan/README.md#cleanup前の結果clone)を参照する。

## 最終検証

- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/) | test(/process_continuation::tests/) | test(/process_stop_tests/)'`: 52 passed、335 skipped、終了コード0。
- 同じtargetの`rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,567 passed、69 binaries、失敗・除外0、終了コード0、nextest部分114.944秒。
- 同じtargetの`rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。failed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0、終了コード0。
- Rust workspace fmt、製品・文書・harnessの差分空白検査、Python構文、文書のローカルリンク、保存sourceとbinaryのhash、基準patchの再現を確認した。patchのcontext行は原本のまま保持する。

[検証記録](future_ready_clone_deferral_2026_10_14_validation.json)に終了コード、標準summary、Cargo artifact、最終source hashを保存した。操作数比較・CI・標準テストの間に製品sourceを変更していない。最終差分を自己点検し、raw参照の不変性、同じcleanup判定後の遅延clone、owned配送と元のTupleの独立性、終了判定の既存条件、Running / unknown、context・取消・checkpoint / rollbackを確認した。level2のため別エージェントは使っていない。

## 残件

Future表とcleanup表の検索、配送可能な値のclone、各slotへの配送clone、値Vecと位置表の保持費用は残る。Taskの待機Futureを配送用関数の`is_some`で調べる箇所も、配送可能な場合のcloneが残る。今回のraw借用は終了判定の検査に適用するが、一般の待機・配送APIは分割しない。実時間・allocator・RSSとOI-037は未測定・未解決のままとする。
