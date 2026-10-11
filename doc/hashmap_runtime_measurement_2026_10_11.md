# RT-4: HashMap構築とmap_valuesの基準値測定

測定日: 2026-10-11。対象commit: `547c6a68f1f1186f64cb582f1d303d6c59d12e29`。
状態: 改善前の基準値測定記録。本書の現行コード・未着手の記述は上記commit時点を指す。
後続のコピー削減と検証結果は[改善レポート](hashmap_runtime_optimization_2026_10_11.md)を参照。

入力は別worktreeの[測定報告](/Users/haruca/.codex/worktrees/4c18/surtr/doc/list_runtime_measurement_report_2026_10_11.md)のRT-4と、
本worktreeの[List / VMの残課題](list_runtime_followups.md)。
メインに追加されたRT-3対応（`282f7815`、merge `547c6a68`）を含む状態で、一件進めた。
手動測定ハーネスと文書の追加に閉じるlevel2相当の作業であり、製品コード、
通常のcorrectness test、標準API、言語仕様、VM/schema versionは変更していない。

## 確認できたこと

異なるキーの構築では、入力件数を倍にすると累積requested bytesが約4倍になる。
一つのキーへの上書きでは約2倍だった。これは既存entryを全cloneする現行コードと整合する。
最終結果の保持量と累積割当量は異なるため、二次の割当量を二次の保持量とは扱わない。

Stringの1,024件を構築する公開Rust APIでは266.361 MiB、VM構築では269.502 MiBを
累積requestした。さらにidentityの`map_values`を実行するVM系列では537.619 MiBだった。
後者には入力mapの構築も含む。開始、callback、各resumeの費用を個別に測った値ではない。

今回の結果は既存の性能課題を具体化したものであり、直近変更のデグレとは判定しない。
RT-4全体を解決したものでもない。

## 測定条件と原本

- Apple arm64、macOS 26.6.2、8 logical CPU、physical memory 8 GiB。
- `rustc 1.94.1 (e408947bf 2026-03-25)`、Cargo release、System allocator。root lockと同版の依存を使うpackage lockで、最終binaryを`--locked`でbuildした。
- 6 workload × 3件数 × 2 payload = 36条件を、各5回の独立processで実行した。count有効・無効の2系列で計360 process、1,080区間サンプル、216区間集計。
- n=256 / 512 / 1,024。キーは8-byte ASCII固定長。payloadは小さいInt(index)と、indexを含む256-byte String。same-key対照の最終件数は1であり、distinct系列と出力件数は異なる。
- prepare / run / dropを分け、JSON整形・出力は全区間終了後に行った。build、process起動、`/usr/bin/time`の集計は区間外。VMのopcode集計observerは有効、opcode/call traceは無効。
- 表の時間はcounter無効系列、割当量はcounter有効系列の中央値。RSSはcounter無効系列のprocess全期間peakの中央値。各条件5回の観測であり、速度やRSSの合否基準にはしない。

再現コマンドと区間の正確な内容は[harness README](../tests/profile/hashmap_cost/README.md)。
原本は[count生データ](hashmap_cost_2026_10_11_count.jsonl)・[集計](hashmap_cost_2026_10_11_count.summary.json)、
[time生データ](hashmap_cost_2026_10_11_time.jsonl)・[集計](hashmap_cost_2026_10_11_time.summary.json)。
metadataにはsource commit、toolchain、platform、binary / Rust source / runner / lockのSHA-256を保存した。

requested bytesはallocator layoutの要求量であり、物理コピーbytesやRSSではない。
reallocは新layout全体をrequestedへ、旧layoutをfreedへ加える。
peak extraは区間開始時の論理liveからの増分で、allocator内部のrealloc時の同時保持やpage保持は測れない。
counter無効系列の割当欄は未測定を表すnull。計測overheadを時間から差し引かない。

## 公開Rust API

`HashMapHandle::from_entries`の入力Vecはprepareで作り、runで消費する。
runのlive差分には入力Vecの解放も入るため、出力mapのstorageサイズではない。
`sort`系列は公開storageへ直接collectして入力mapを作り、`sorted_entries`だけをrunで測る。
run後は入力mapとsorted Vecが両方生存する。これはbuiltin開始全体の測定ではない。

| workload | payload | n | run ms | 累積requested MiB |
|---|---|---:|---:|---:|
| distinct構築 | Int | 256 | 2.535 | 8.963 |
| distinct構築 | Int | 512 | 6.529 | 35.695 |
| distinct構築 | Int | 1,024 | 26.434 | 142.474 |
| distinct構築 | String | 256 | 2.165 | 16.684 |
| distinct構築 | String | 512 | 7.576 | 66.639 |
| distinct構築 | String | 1,024 | 31.385 | 266.361 |
| same-key構築 | Int | 1,024 | 0.149 | 0.652 |
| same-key構築 | String | 1,024 | 0.149 | 0.894 |
| clone + sort | Int | 1,024 | 0.113 | 0.328 |
| clone + sort | String | 1,024 | 0.174 | 0.570 |

String distinct 1,024件のrunは30.027〜118.319 msとばらついた。最大値を除外していない。
他条件の最小・最大、prepare / drop、allocation回数、live / peakも原本に残した。
process peak RSSは、同条件で4.047 MiB、same-keyで2.688 MiB、sortで3.422 MiBだった。
累積266.361 MiBをそのまま生存させる処理ではない。

[`HashMapHandle::insert`](../crates/sindr/src/runtime.rs)は既存map全体をcloneする。
distinct n件の既存entry cloneはコード上n(n−1)/2回、same-key n件はmax(n−1, 0)回。
1,024 distinct件では523,776回になる。これは期待値であり実測clone counterではない。
keyやpayloadの物理コピー量、hash、drop、allocatorの寄与を個別には計測していない。

## 実VM

手組みbytecodeを`VM::run`で実行する。builtin IDはcanonical metadataから取得した。
runはboot / bytecode検証、定数load、TupleとListの生成、`map_from_entries`を含む。
`vm_map_values`はその後のidentity callbackによる`map_values`も含む。
dropは出力だけでなく定数・observerを含むVM全体の破棄。出力mapの取り出しcloneは行わない。

| workload | payload | n | run ms | 累積requested MiB | process peak RSS MiB |
|---|---|---:|---:|---:|---:|
| distinct構築 | Int | 256 | 1.947 | 9.448 | 3.781 |
| distinct構築 | Int | 512 | 6.991 | 36.662 | 4.188 |
| distinct構築 | Int | 1,024 | 27.197 | 144.405 | 5.297 |
| distinct構築 | String | 256 | 2.154 | 17.472 | 3.953 |
| distinct構築 | String | 512 | 8.108 | 68.211 | 4.703 |
| distinct構築 | String | 1,024 | 31.298 | 269.502 | 6.188 |
| same-key構築 | Int | 1,024 | 0.770 | 1.924 | 4.109 |
| same-key構築 | String | 1,024 | 0.787 | 2.893 | 4.562 |
| distinct構築 + map_values | Int | 256 | 3.826 | 18.729 | 3.891 |
| distinct構築 + map_values | Int | 512 | 14.501 | 72.995 | 4.375 |
| distinct構築 + map_values | Int | 1,024 | 54.522 | 288.150 | 5.500 |
| distinct構築 + map_values | String | 256 | 4.376 | 34.597 | 4.125 |
| distinct構築 + map_values | String | 512 | 16.392 | 135.729 | 4.844 |
| distinct構築 + map_values | String | 1,024 | 62.268 | 537.619 | 6.344 |

map_valuesのcallback完了数は全条件でnだった。各callbackが一度Returnし、他のuser functionやTCOを
含まないworkloadに限ってReturn数を代理指標に使う。identityの結果とReturn数からcallback順は確認できない。
キー昇順、RuntimeError伝播、Err値の保持は既存の直接テストで確認する。

[`map_values`](../crates/eldr/src/builtin/map_values.rs)は開始時に全entryをclone・sortし、
成功再開ごとにoutput全体をcloneするinsertを呼ぶ。入力構築と出力構築の既存entry clone期待値は
合わせてn(n−1)回。引数・iterator・callback・sort等のcloneはこの式に含まない。
別workloadの時間差をmap_values単独の実測値として扱わない。

## 次に必要な仕様と受入条件

優先候補は、完成前のmapを内部だけで可変に構築し、反復insertによる全cloneを除く方式である。
今回の測定では採用せず、次の仕様入力で対象を確定する。公開insertの永続性と、
continuationが持つ途中出力の保存契約を分けて設計する。

- 構築: duplicate keyは後勝ち、公開mapはimmutable、入力エラーの位置・伝播を維持する。通常のinsert / removeの意味を変えず、完成前の部分結果を公開しない。
- map_values: 全キーを保持し、キー昇順でcallbackを一度ずつ呼ぶ。空mapは呼ばず、言語Err値は通常値として保存し、RuntimeError後は後続callbackを呼ばない。
- 可変な途中出力: 通常の実行切替えでは所有権を移し、checkpoint保存状態からlive状態を独立させる。取消・rollback・待機と再開でcallback、副作用、結果配送を重複させない。
- 分割: cloneやsort、出力構築のどこまでを共通予算へ参加させるか、一回の処理量を明示する。CPU yieldとFuture待機を区別し、batch / REPL / Task / processの通常入口で確認する。Value clone / dropやallocatorの一回の実時間を要素数だけで保証しない。
- JSON object変換やregex / OS I/Oは別対象とする。コピー削減だけでRT-4全体やVM実時間上限の解決を宣言しない。

正本は[HashMap標準定義](../lib/types/hash_map.srt)と[Eldr VM仕様](../docs/dev/EldrVM_spec.md)。
分割方式・continuation契約を変える実装はlevel4として、仕様、回帰テスト、全体gate、独立レビューを揃える。

## 検証と未検証範囲

- rootと同版の依存でcount / time release buildが`--locked`で成功。36条件×5回×2系列はすべて終了コード0。
- `verify.py`の別process60件で全出力内容、duplicate後勝ち、sort結果、空・一件の境界を確認した。性能原本には混ぜていない。
- `rtk cargo nextest run -p sindr -p eldr -E 'test(/hash_map_handle_insert_overwrite_and_remove_use_sorted_key_order/) | test(/hash_map_builtins_preserve_order_and_overwrite_semantics/) | test(/builtin::map_values::tests/) | test(/map_values_rejects_invalid_map_input/)'`: 6 passed、442 skipped、失敗0。duplicate後勝ち・キー順、callback一回、空map、RuntimeError伝播・Err値の保持、不正引数の拒否を既存テストで確認した。テスト選択はsurtr-test-strategyに従った。
- 原本と集計の件数・最小/最大/中央値、binary / source / runner / lockのhash、root lockと同版の依存を照合した。
- サブエージェントが現行コードと測定対象を独立に確認した。Astraが1,080区間・216集計、24表行、callback数・理論clone数、requested−freedとlive差分、count/timeのnull区別、binary / source / runner / lockのSHA、rootとの依存name / version / source / checksumを照合した。報告の丸め転記4箇所を原本に合わせて修正した。製品のデグレや新規の機能問題の指摘はない。
- 製品変更がないためworkspace CI gateと標準Surtrテスト全件は実行しない。compiler通常入力、REPL / Task / process、取消・rollbackの性能、JSON、RT-4の他builtinは未測定。

次の一手は、反復構築のコピー削減仕様と、map_valuesの可変途中出力・分割仕様をそれぞれ確定すること。
