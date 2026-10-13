# RT-3: Future表が非空の不要なstack走査を削減する

作業日: 2026-10-13。入力は [runtime一覧](../README.md)・[OI-036](../../../open-issues.md#oi-036-future表が非空のoperand-stack走査)。基準はmain `c577b32d`に前回の未コミットBuilder共有化を反映した作業treeである。Builder共有化と今回のFuture走査削減は、変更単位ごとにmainへコミットした。測定時の基準ソースと原本は保持している。

## 方式と受入条件

Eldr内に閉じるlevel2の改善として、内部の`OperandStack`へtop-level PendingFuture件数を持たせた。件数0、またはFuture表が空なら、opcode前の読取り・書込み走査を省く。PendingFutureが残る場合の走査は維持する。これは既存の配送条件を維持する局所的な方式選択で、公開構文・API・builtin ID・Opcode・VM/schema versionは変更しない。

従来の可変Vecを包み、push / pop / truncate / clear / ready置換をこの型の中で行う。readonly sliceの参照は許すが、任意のValueの可変参照は返さない。Callable metadataの変更はCallableへの可変参照だけを返す。nested Value内のPendingFutureは旧走査でも解決しないため数えない。複数slotの置換、ready値が別PendingFutureである場合も件数を保つ。

VM・ExecutionContext・checkpointは値と件数を一緒に移動・Clone・復元する。状態不明時に全走査へ戻すfallbackは置かない。`ready_future_value`によるdetached Taskのcleanup完了判定は維持する。旧Vecの直接可変アクセス経路は置き換えた。

[正本EldrVM](../../../../docs/dev/EldrVM_spec.md)と[テスト方針](../../../../docs/dev/テスト方針.md)を更新した。前回のBuilder処理に変更したのはstack初期化の型対応だけで、予算付き完成処理は維持する。

## 決定的な操作数と内部VM測定

Redでは、非空Future表・PendingFutureなしの既存ケースと、128回のLoadLocalからListを作る新規ケースが失敗した。後者のread visitsは8,257で、期待0に対する失敗である。

[manual probe](../../../../tests/profile/future_stack_scan/README.md)は準備を区間外に置き、VM::runだけを測る。1,000 / 10,000要素、五つのFuture条件、各前後6 fresh processをAB/BA同数で実行した。120 sample、20集計。cfg(test)のscan counterは両版とも有効で、その費用を差し引いていない。別processの20検証で全内容を確認し、検証系列に時間・RSSを含めていない。

10,000要素、時間はmsの中央値。

| 条件 | read visits 前→後 | VM run ms 前→後 |
|---|---:|---:|
| empty | 0 → 0 | 1.299 → 1.325 |
| unrelated_running | 50,005,001 → 0 | 36.616 → 1.307 |
| unrelated_ready | 50,005,001 → 0 | 36.541 → 1.380 |
| pending_ready | 50,015,003 → 1 | 36.363 → 1.330 |
| pending_running | 50,015,003 → 50,015,003 | 36.439 → 36.512 |

PendingFutureがない条件の二次的な全走査を除いた。ready PendingFutureは最初の一件のread / writeだけになる。未解決のPendingFutureが残る条件は約5,001万slotの走査がそのまま残り、全てのFutureあり入力を定数時間にしたものではない。

件数用usizeをstack ownerに一つ追加する。今回のcompilerではExecutionContextは904→912bytes、VMは3,104→3,120bytesで、VMの差は配置のpaddingも含む。この数値は安定ABIではない。件数用の追加heap容器は持たないが、context数に応じた固定幅の追加状態と、push / pop等の判定費用はある。従来のRT-3計画で条件にした追加状態0とは範囲が異なる。

[生データ](future_stack_nonempty_2026_10_13.jsonl)・[集計](future_stack_nonempty_2026_10_13.summary.json)・[別process検証](future_stack_nonempty_2026_10_13_verify.json)。前後は独立targetでbuildし、Cargo compiler-artifactのmanifest・executableと保存binary、source hashを照合した。[変更前build](future_stack_nonempty_2026_10_13_before_builds.json)・[変更後build](future_stack_nonempty_2026_10_13_after_builds.json)。測定とビルド・テストを重ねていない。

## 通常のTask・List literal入口

`Task::async({|| Ok(())})`を開始し、1,000 / 10,000要素のList literalを構築して件数を表示し、最後にTaskをawaitする。Taskなしの対照も同じ要素数で測る。compile・warmupは区間外、48 measured processはすべてartifact cache hit。observer / traceは無効。各条件前後6 processをAB/BA同数にした。

10,000要素・TaskありのCLI execute中央値は39.672→1.873ms、Taskなしは1.459→1.558msだった。Futureなしの対照で追加費用を観測しており、一律の高速化は主張しない。1,000要素・Taskありは0.627→0.442ms、Taskなしは0.409→0.414ms。executeはVM構築・通常実行・background drain・出力を含み、内部VMの区間と混同しない。totalとparent wallも原本で分けて保存する。

10,000要素・Taskありのprocess peak RSS中央値は37.883→37.828 MiB、Taskなしは37.711→37.641 MiBだった。RSSはprocess全期間の高水位で、区間保持量ではない。requested allocation / logical live / copy byteは今回測っていない。微小なRSS差を一般的なメモリ改善とは扱わない。

最初の入力はprintへIntを渡してしまい、String必須の型検査で拒否された。`print(inspect(...))`へ修正し、採用系列は全条件で件数とawait結果が一致した。失敗はwarmup前で、性能原本へ混ぜていない。

[通常入口の原本](future_stack_nonempty_batch_2026_10_13.jsonl)・[集計・入力全文・warmup](future_stack_nonempty_batch_2026_10_13.summary.json)。

## 検証と点検

- Red: 2件が想定したscan count不一致で失敗。終了コード100。
- 移植時のtest constructorの型名・管理表API・source context不足を修正した。これらは製品の回帰でもRedの証拠でもない。
- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/)'`: 12 passed、363 skipped、終了コード0。
- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,555 passed、69 binaries、失敗・除外0、終了コード0、nextest部分139.673秒。
- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功、失敗・skip・pending・filter・policy error・scope failure・script errorは0、終了コード0。Cargo artifactのSindr・Eldr・Rune manifestと実行exeが今回のmain・専用targetを指すことを確認した。
- `cargo fmt --all -- --check`、`git diff --check`、Python構文、文書のローカルリンク・code fence、測定after source hashとの一致: 成功。

[検証記録](future_stack_nonempty_2026_10_13_validation.json)に終了コード、標準summary、Cargo artifact、最終source hashを残した。[変更前source patch](future_stack_nonempty_2026_10_13_baseline.patch)は、`c577b32d`に前回Builderの製品変更と今回の計測関数を追加した状態を再現する。独立checkoutへ適用し、今回のharnessを置けば変更前のunit / CLI binaryを再buildできる。

最終差分を自己点検し、任意の可変アクセスの残存、slot件数、Futureが返すFuture、重複slot、cleanup前後、context切替え、checkpoint / rollback、Callable metadataの診断を受入条件と照合した。今回のlevel2作業では別エージェントを使っていない。

## 残件

OI-036のPendingFutureなしの不要な読取り走査には対応した。未解決PendingFutureが残るstackの全走査、ready判定によるdetached Task表の検索は残る。固定幅metadataとFutureなしの追加費用、Value clone / drop、OI-037の別workloadのRSS差は今回の効果へ読み替えない。

その後の[未解決Futureの位置追跡](future_stack_index_optimization_2026_10_13.md)と[通常経路の費用削減](future_stack_metadata_optimization_2026_10_13.md)は別の改善として記録した。上記の件数方式・残件・測定値は、このレポート作成時点の状態である。
