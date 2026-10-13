# OI-036: Futureなしの表の空判定を省く

作業日: 2026-10-13。入力は[runtime一覧](../README.md)、[前回の追加費用の残件](future_stack_metadata_optimization_2026_10_13.md#残件)、[位置追跡の仕様](future_stack_index_plan_2026_10_13.md)。基準はmain `40609731`。配送契約と追加状態を変えない、Eldr内のlevel2変更として進める。

## 方式と受入条件

opcode前のFuture解決では、追跡slotが0件ならFuture表を参照する前に戻る。追跡slotがある場合だけ表の空判定を行い、非空なら従来どおり追跡slotのready値を収集して置換する。emptyな表とunknown IDの組合せも、従来どおり値を置換しない。

- Future表が空・Running・Readyのいずれでも、PendingFutureなしのstackで表の空判定は0回とする。
- PendingFutureがあるときは表の空判定を行い、空ならread / write visitsは0、非空なら従来と同じslotを読む。
- readyの重複slot、Futureを返すFuture、Cancelled、cleanup完了待ち、context切替え、checkpoint / rollbackを維持する。
- 位置buffer、追加metadata、public API、Opcode、VM/schema versionを増やさない。通常値の走査や状態不明へのfallbackを追加しない。

`RuntimeTable::is_empty`はRc配下のBTreeMapを読み、変更を行わない。`OperandStack::has_pending_futures`も追跡位置を読むだけで、判定順序の変更は配送の意味を変えない。Future表やcleanupの検索方式自体は変更しない。

## 手順と検証

既存のempty / nonempty境界テストに表の空判定回数を加え、PendingFutureなしでも1回参照する現在の方式でRedを確認する。表の参照を省くようにガードの順序を変え、既存の関連16件でGreenを確認する。

内部VMの5条件を前後6 process、通常Task / List入口の対照を前後10 processで、AB/BA同数にして測る。全内容検証は別processで行う。内部VMはcfg(test)のcounter付き、通常CLIはcounterなしであるため、実時間の根拠を区別する。速度閾値をcorrectness testに加えず、前回の小さい時間差を消したとは測定なしに報告しない。

最終sourceでworkspace CIと標準Surtrの全件を一回実行する。正本EldrVM・テスト方針・一覧と残件を更新し、実行結果と未解決範囲をこの文書へ追記する。

## 実装とRed / Green

既存のempty / nonemptyテストの拡張は、通常値stackに対するFuture表の空判定が1回、期待0回という理由で2件失敗した（終了コード100）。判定順序の変更後、同じ対象を含む関連16件が成功した（363 skipped、終了コード0）。unknown IDがstackにある場合は、空表の確認を1回行って未解決のまま保つ。

製品差分はガードの順序だけであり、表の検索と`ready_future_value`によるcleanup完了判定は維持する。変更前は`40609731`へcfg(test)の表参照counterと受入テストを加えた状態で測った。[基準patch](future_stack_guard_2026_10_13_baseline.patch)を`git archive 40609731`から作った独立した一時checkoutへ適用し、基準buildの全source hashとの一致を確認した。基準側の製品挙動は変えていない。

## 内部VMの決定的な操作数

1,000 / 10,000要素×5条件×前後6 fresh process、AB/BA同数で120 sampleを測った。全List内容の検証は別processの20件で行い、その系列は時間・RSSを集計していない。10,000要素の中央値を示す。時間はms。

| 条件 | 表の空判定回数 前→後 | read / write visits 前→後 | run ms 前→後 |
|---|---:|---:|---:|
| empty | 10,002 → 0 | 0 / 0 → 0 / 0 | 1.393 → 1.388 |
| unrelated_running | 10,002 → 0 | 0 / 0 → 0 / 0 | 1.396 → 1.388 |
| unrelated_ready | 10,002 → 0 | 0 / 0 → 0 / 0 | 1.419 → 1.420 |
| pending_ready | 10,002 → 1 | 1 / 1 → 1 / 1 | 1.445 → 1.412 |
| pending_running | 10,002 → 10,002 | 10,002 / 0 → 10,002 / 0 | 1.554 → 1.548 |

不要な表の空判定を省いた。counterは論理的な空判定呼出しを数え、CPUのload命令やcache missは測っていない。未解決Futureのready確認は従来どおり毎opcodeで行う。内部VMはcfg(test)の表参照・slot訪問counterを有効にした時間であり、counterの費用も差し引いていない。そのため、表の読取りだけの時間寄与とみなさず、counterなしの通常CLIを別に確認した。

ExecutionContextは前後912 bytes、VMは前後3,120 bytesで一致した。既存の位置bufferとpointer幅metadataは保ち、新しい製品追跡状態を加えていない。

[内部VM原本](future_stack_guard_2026_10_13.jsonl)・[集計](future_stack_guard_2026_10_13.summary.json)・[別process全内容検証](future_stack_guard_2026_10_13_verify.json)・[変更前build](future_stack_guard_2026_10_13_before_builds.json)・[変更後build](future_stack_guard_2026_10_13_after_builds.json)。ビルド前後のsource hashと、保存したCargo executableのhashを照合した。前後buildは同じtargetを順にrebuildし、各状態のsnapshotと別名のbinaryを保存した。測定中にはbuild・testを実行していない。

## 通常CLIの比較

1,000 / 10,000要素のList literal、Taskの有無、前後20 fresh processをAB/BA同数にして160 sampleを測った。全sampleでartifact cache hit、件数のstdoutとTask await結果が一致した。observer / traceは無効。Rustのcfg(test) counterは含まない。CLI executeはVM構築・実行・background drain・出力を含み、内部VMのrun区間とは区別する。

| 要素数 | 条件 | execute ms 前→後 | process peak RSS MiB 前→後 |
|---|---|---:|---:|
| 1,000 | empty | 0.396 → 0.387 | 30.258 → 30.570 |
| 1,000 | unawaited_task | 0.446 → 0.451 | 30.391 → 30.727 |
| 10,000 | empty | 1.432 → 1.424 | 37.305 → 37.500 |
| 10,000 | unawaited_task | 1.684 → 1.688 | 37.445 → 37.656 |

10,000要素のTaskなしは1.432→1.424ms、Taskありは1.684→1.688msだった。min / maxが重なり、実時間はほぼ同じ範囲である。一律の高速化や、前回残した追加費用の解消は主張しない。今回の成果は不要な表参照を決定的に0回へ減らした点である。

RSSはprocess全期間の高水位で、区間内保持量ではない。requested allocation / logical live / copy byteは未測定。追加状態を増やさなかったことと、一般的なRSS改善を混同しない。total・parent wall・入力全文・warmupは[通常入口集計](future_stack_guard_batch_2026_10_13.summary.json)へ保存した。[原本](future_stack_guard_batch_2026_10_13.jsonl)も保存する。

## 最終検証

- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/)'`: 16 passed、363 skipped、終了コード0。
- 同じtargetの`rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,559 passed、69 binaries、失敗・除外0、終了コード0、nextest部分115.671秒。
- 同じtargetの`rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。failed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0、終了コード0。
- Rust fmt、製品・文書・harnessの差分空白検査、Python構文、ローカル文書リンク、測定after artifactとのsource hash一致を確認した。保存したbaseline patchのcontext行は原本として保持する。

[検証記録](future_stack_guard_2026_10_13_validation.json)に終了コード、標準summary、Cargo artifact、最終source hashを保存した。測定・CI・標準テスト中は製品ソースを変更していない。最終差分を自己点検し、ガード後のready収集・書込み経路が従来と同じであることを確認した。正本EldrVMとテスト方針も更新した。level2のため別エージェントは使っていない。

## 残件

Futureなしのガードでも既存のpointer幅metadataを読む費用は残る。位置bufferの保持・clone、追跡slotごとのFuture表・cleanup検索、通常入口の小さい時間差も残る。今回の通常入口は前回の採用版と比較したもので、追跡前のVec版との新しい速度比較は行っていない。OI-037のRSS差とruntime全般のfairnessは対象外とする。
