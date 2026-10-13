# OI-036: Futureなしの追加費用を減らす

作業日: 2026-10-13。[位置追跡](future_stack_index_optimization_2026_10_13.md)後の通常経路を対象にしたEldr内のlevel2変更である。[方式と受入条件](future_stack_index_plan_2026_10_13.md)に沿って比較する。

## 方式

位置Vecのmetadataを必要になったときだけBoxへ確保する。stack内にはoptional pointerだけを持ち、追加metadataを1 pointer幅以内に抑える。最初のFuture slotはBoxと位置bufferを確保し、使用済みのbufferは再利用する。Futureを一度も持たないstackはどちらも確保しない。contextの固定幅費用と、Futureありのheap費用を交換する方式である。

pushはFutureの位置登録を小さなhelperへ分け、通常値は値bufferへ追加する。popは追跡した最後のslotが現在の末尾かどうかを先に調べ、値を返した後のvariant検査を除く。値と位置を同じ型だけが変更するため、この判定でFutureの位置を正確に除ける。readonly参照とCallable参照だけを公開する境界を維持する。小さなpush / popにinline指定、Future専用の位置登録helperにcold指定を加える。実時間の効果はbinary配置と切り離して断定しない。

## 比較と不採用案

- stack内の位置Vecは、件数版に対してcontextを16 bytes増やし、通常入口でも追加費用を観測した。
- 同じ位置Vecのまま5操作に`#[inline]`、さらに`#[inline(always)]`を指定した版も測った。件数版との差を解消できず、これらの製品経路は残していない。
- optional Box版でcontextの固定幅を戻し、最後に通常のpush / popからFuture専用処理を分けた版を比較した。

前後のsource snapshot、Cargo artifact、保存binaryのhashを残す。比較した版のrawとsummaryは上書きしない。最初の件数版はmain `187fc6b4`と全source hashが一致し、それ以前のVec版は前回レポートのbaseline artifactを使う。インライン指定・Box・helperの各版は変更時のsnapshotと保存binaryがある。途中のcodegen比較では同じtargetを順にrebuildしたが、Eldr/Runeの非fresh artifactと保存したexeを記録し、各系列の前後でもbinary hashを照合した。

## 最終版の測定

内部VMは前回の件数版と最終版を比較した。1,000 / 10,000要素×5条件×前後6 process、AB/BA同数で120 sample、全内容検証は別processの20件。cfg(test)のscan counterの費用は差し引いていない。10,000要素、run時間はmsの中央値。

| 条件 | read visits 前→後 | run ms 前→後 |
|---|---:|---:|
| empty | 0 → 0 | 1.350 → 1.352 |
| unrelated_running | 0 → 0 | 1.406 → 1.433 |
| unrelated_ready | 0 → 0 | 1.325 → 1.326 |
| pending_ready | 1 → 1 | 1.311 → 1.320 |
| pending_running | 50,015,003 → 10,002 | 37.046 → 1.436 |

readyのwrite visitsは1、未解決時は0。最終版のExecutionContextは912 bytes、VMは3,120 bytesで、件数版と同じになった。位置Vecをstack内に持つ中間版からはどちらも16 bytes減る。元のVecだけの版に対する追加状態は消えていない。安定ABIやheap保持量の数値ではない。

[内部VM原本](future_stack_fast_2026_10_13.jsonl)・[集計](future_stack_fast_2026_10_13.summary.json)・[別process検証](future_stack_fast_2026_10_13_verify.json)・[最終build](future_stack_fast_2026_10_13_after_builds.json)。

通常入口の各比較は1,000 / 10,000要素×Task有無×前後10 processで80 sample、AB/BA同数、全artifact cache hit。stdoutの件数とawait結果を確認した。10,000要素のCLI execute中央値は次のとおり。単位はms。

| 比較 | Taskなし 前→後 | Taskあり 前→後 |
|---|---:|---:|
| 位置Vec → 最終版 | 1.546 → 1.444 | 1.766 → 1.685 |
| Box分離のみ → 最終版 | 1.480 → 1.417 | 1.730 → 1.667 |
| 前回の件数版 → 最終版 | 1.409 → 1.419 | 1.648 → 1.667 |
| 追跡前のVec版 → 最終版 | 1.399 → 1.440 | 37.098 → 1.651 |

位置追跡で増えた通常経路の費用を減らした。前回の件数版や追跡前のVec版との小さい差は残り、追加費用0や一律の高速化は主張しない。1,000要素の対照、min / max、total、parent wallは集計に残す。比較系列ごとの変動を混ぜて改善率を算出しない。

[位置Vecとの原本](future_stack_index_fast_batch_2026_10_13.jsonl)・[集計](future_stack_index_fast_batch_2026_10_13.summary.json)、[Boxのみとの原本](future_stack_boxed_fast_batch_2026_10_13.jsonl)・[集計](future_stack_boxed_fast_batch_2026_10_13.summary.json)、[件数版との原本](future_stack_count_fast_batch_2026_10_13.jsonl)・[集計](future_stack_count_fast_batch_2026_10_13.summary.json)、[Vec版との原本](future_stack_vec_fast_batch_2026_10_13.jsonl)・[集計](future_stack_vec_fast_batch_2026_10_13.summary.json)。

RSSはprocess全期間の高水位で、位置Vecとの10,000要素・Taskなし対照は37.680→37.617 MiB、Boxのみとの対照は37.531→37.641 MiBだった。時間の改善とRSSの減少を同じ主張にしない。requested allocation / logical live / copy byteは未測定。初回Future slotのBox確保と位置buffer確保、Futureありのcontext cloneと、使用済みのbuffer保持は残る。

## 検証

metadata幅のテストは、stack内に位置Vecを持つ48 bytesに対する、Value Vec + 1 pointer以内（32 bytes）の条件でRedになった（終了コード100）。Box分離後はGreen。最後のpush / pop整理も配送・位置更新を含む同じ16件でGreenを維持した。人工的な時間閾値のRedは設けていない。

- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/)'`: 16 passed、363 skipped、終了コード0。
- 同じtargetの`rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,559 passed、69 binaries、失敗・除外0、終了コード0、nextest部分113.612秒。
- 同じtargetの`rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。failed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0、終了コード0。
- `cargo fmt --all -- --check`、差分の空白検査、Python構文、文書のローカルリンク、最終release artifactとのsource hash一致を確認した。

[検証記録](future_stack_followup_validation_2026_10_13.json)に終了コード、標準summary、Cargo artifact、最終source hashを残した。CI・標準テストの実行期間に製品ソースは変更していない。最終差分を自己点検し、位置不一致のfallback、任意の可変Value参照、旧全走査経路の残存がないことと、重複slot・Futureが返すFuture・cleanup・context切替え・checkpoint / rollbackの維持を確認した。level2のため別エージェントは使っていない。

## 残件

未解決Futureを含む通常値stackの全走査は除いた。追跡slotごとのFuture表・detached Task cleanup検索は残る。追加のpointer幅metadata、Futureありのheap費用、Futureなしの比較系列での小さい時間差は残る。OI-037の別workloadのRSS差、Value clone / drop、runtime全般のfairnessは対象外である。

## 比較資料

通常のinline指定版は`future_stack_inline_*`、強いinline指定だけの版は`future_stack_force_*`、Box分離だけの版は`future_stack_metadata_*`と対応するbatch原本に残した。これらは不採用または中間状態の比較資料で、現行ソースは`future_stack_fast_*`のbuild recordを基準にする。製品コードの旧経路は残していない。
