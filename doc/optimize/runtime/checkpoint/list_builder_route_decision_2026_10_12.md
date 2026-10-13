> 取り込み元: `/Users/haruca/.codex/worktrees/9ea8/surtr`（codex/rt5-builder-checkpoint-measurement、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# List構築経路の選択

入力は [OI-039の調査結果](list_runtime_oi039_cost_2026_10_12.md) と「サイズで分岐するか、常に同じルートを通すか」という依頼。List.flat_mapを用いる標準APIとGeneratorのList取得処理を確認し、サイズ閾値を設けず、共通の内部Builderを使う方針を選ぶ。言語仕様と公開APIは変えない。

## 選択した方針

対象のList.flat_mapとGeneratorのList構築は要素数にかかわらず、現在のappend専用segmentを共有する `ListBuilder` と共通の完成継続を使う。保存時にValueを全cloneしない契約を維持し、小さいBuilderだけ旧Vecコピーへ戻す切替は追加しない。consやappendを含む、全てのList構築をこのBuilderへ置き換える方針ではない。

独占した単一bufferはPackedへ移し、共有・分割済みprefixは予算内でConsへ接続する。この違いは所有条件による最適化であり、サイズ閾値ではない。Generatorの初期予約容量上限1,024も、構築方式を切り替える条件ではない。

この方針は実装の単純さ、保存・rollbackの独立性、予算、保存時のValueコピー0の契約に基づく選択である。全入力で最速になることや、OI-039のInt遅化・RSS差が解決したことは意味しない。

## サイズ分岐を選ばない根拠

既存の交互原本を、各process内で保存付き9区間を加算し、8反復で割ってから6 processの中央値を取る方法で再集計した。同じ保存時Builder長でもTask数やpayloadで差の方向が変わる。

| 保存時Builder長 | 旧Vecに対する共有方式の9区間合計の差 |
|---:|---:|
| 32 | -2.5%〜+0.3% |
| 1,024 | -5.3%〜+0.5% |
| 16,384 | -32.0%〜-14.6% |

原本は [旧Vecと共有方式の交互測定](rt5_builder_2026_10_12_accepted_recheck.jsonl)・[集計](rt5_builder_2026_10_12_accepted_recheck.summary.json)。小さい保存時長さで旧Vecが一律に有利となる境界は確認できない。さらに、この長さは保存済みprefixの長さであり、完成結果は2,048件長い。長さ32の条件も完成出力は2,080件なので、真に短い完成出力の性能を測ったものではない。

要素数だけではValueのclone費用を決められない。StringやBigInt等の内容、保存回数、保存側の生存期間、Vec容量、完成後の利用も費用へ影響する。`flat_map`の入力長と出力長は異なり、GeneratorのAll/Whileも出力長を実行前に確定できない。入力長やtake上限をそのまま閾値へ使う根拠はない。

[現在版への局所改善比較](oi039_2026_10_12_capacity_compare.summary.json)は別時期の交互系列なので、そこから旧Vecとの差を直接合成しない。新規の性能測定は行わず、既存原本の再集計とソースの接続確認を追加調査として実施した。

## 標準ライブラリへの適用

作業ツリーでは次の公開APIが共通Builderへ接続済みであり、標準ソース側に別のサイズ分岐や実装を加える必要はない。

| 標準API | 実装の接続 |
|---|---|
| List.flat_map、map、filter、Applicative.ap、Monad.bind | `lib/types/list.srt` → `builtin/list_flat_map.rs` → `ListBuilder` / `finish_list` |
| Generator.take、take_while、to_list | `lib/types/generator.srt` → `builtin/generator.rs` → 同じBuilder / 完成継続 |
| InfiniteGenerator.take、take_while | `lib/types/infinite_generator.srt` → 同じmaterialize / Builder / 完成継続 |

開発正本 `docs/dev/EldrVM_spec.md` にサイズ閾値を設けない方針を明記した。本ターンではRust・標準 `.srt`・実行可能テストを変更していない。既に実装した共通方式の適用範囲を確認したものであり、mainへの統合ではない。main `72154707` は旧実装のままで、作業ツリーの変更は未コミット・未統合である。

## 検証と残件

サブエージェントが標準APIからbuiltinへの接続と原本を確認し、Astraがサイズ閾値を導入しない判断を独立レビューした。公開APIの既存テストを次のコマンドで実行した。

- `rtk proxy cargo run -- test --quiet --format=json lib/tests/monads/list.srt`: 121件成功。
- `rtk proxy cargo run -- test --quiet --format=json lib/tests/modules/generator.srt`: 38件成功。
- `rtk proxy cargo run -- test --quiet --format=json lib/tests/modules/infinite_generator.srt`: 18件成功。

計177件、全て実行、失敗・skip・pending・filterは0、終了コード0。JSONとstderrは `/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-9ea8-builder-route-90_tewdx/` に保持する。製品sourceは前回検証時から変わっていないため、workspace全件は再実行していない。

OI-039の通常経路と実VMのRSS差は引き続き調査対象である。性能上の最適方式まで決める追加比較では、真の短い完成出力、初期構築、保存頻度、実VMのentry変更・解放境界を分ける。サイズに依存しない所有末尾の直接保持等を試す場合も、保存時コピー0と完成途中のcheckpointの契約を先に照合する。

続行時の [末尾の直接所有の比較](list_builder_owned_tail_2026_10_12.md)では、真に短い完成出力0/1/8/32/256と初期構築を追加した。2候補は安定した改善を確認できず不採用とした。共通Rc方式を維持し、空のBuilderを保存しても元の予約容量を再利用できる修正を適用した。サイズ分岐を導入する根拠は追加比較でも得られていない。
