# Builderの所有条件・解放境界・通常CLIの追試

OI-039の約22 MiBのInt RSS増加は、保存付き処理の後に通常完成を同じprocessで続けた条件へ絞れた。通常完成だけをfresh processで実行した場合は約0.2 MiB差だった。全owner解放後の論理liveは復帰する一方、allocatorのempty領域にresidentが残る。通常CLIの追加費用とallocator / OSの細部は未解決として維持する。

## 来歴と検証

旧Vecは`72154707`へ保存済みbaseline sourceを適用し、共通2ファイルを`3d030ad54`から置いた。Rc版は`3d030ad54`のarchiveで復元。元の測定記録の199 / 200 source SHAが全件一致した。削除済みの元binaryは使っておらず、別target directoryで再buildした。

まず当時のprobeを変更せず、32 Task / 長さ16,384・Int/String・1/8反復の全内容を別processで検証した。count無効の4回AB/BAとcount有効の2回を分けた。続いて製品コードを変えない別copyのprobeにshared-only / unique-only / sequentialを追加し、6回AB/BAで所有条件を分離した。選択用branchでprobeのコード配置も変わるため、その時間値を当時のprobeと同一視しない。patchと両系列のsource / binary / runner SHAを保存した。

## 変更していない当時のprobe

8反復のprocess全期間peak RSSは旧Vec→RcでInt 256.055→279.312 MiB、String 628.633→542.180 MiB。通常完成区間の時間中央値はInt 11.069→12.978 ms、String 12.102→13.845 ms。以前の追加費用とRSS差を再現した。

count系列の最大requested-live増分はInt 359.824→180.188 MiB、String 745.450→447.618 MiB。全owner破棄後は開始値へ復帰し、`drop_unique_vm`の追加requestedは0、freedは両版Int 147,340,106 / String 293,616,714 bytesだった。count無効のRSSと同じprocessの値ではない。

## 所有条件の分離

以下は選択付きprobe、default環境・Int・8反復のprocess全期間peak RSS。時間計測とRSSでは停止barrierを使わない。

| 所有条件 | 旧Vec中央値（範囲） | Rc中央値（範囲） |
|---|---:|---:|
| 保存付きだけ | 256.641（256.609–257.719） | 202.406（202.391–202.422） |
| 通常完成だけ | 163.016（162.969–163.062） | 163.203（163.141–163.219） |
| 保存付き→全破棄→通常完成 | 256.680（253.812–256.688） | 279.305（279.266–279.328） |

Rc版の保存付き処理自体が、独立した通常完成より一律に約22 MiB余分に保持するわけではない。先行した保存付き処理・破棄と後続の通常完成を同じprocessで組み合わせる条件が増分に寄与する。

## 解放境界のOS診断

時間/RSSの系列とは別に、Int・1反復の各境界でvmmapを採取した。最初の32観測に加え、前後のps RSSを持つ追加32観測もexit 0。vmmapの前後でps RSSも記録し、全32境界で一致した。この観測系列の値を時間や全期間peakの集計には混ぜない。

| Rc版の境界 | ps RSS（MiB） |
|---|---:|
| 保存付き完成 | 199.344 |
| 保存付きVM全破棄 | 129.016 |
| 通常VM準備 | 199.422 |
| 通常完成 | 208.422 |
| 通常VM全破棄 | 274.297 |

保存付きVMの全破棄後に`MALLOC_SMALL (empty)`のvirtual 124.0 MiB / resident 116.2 MiBを観測した。通常VM全破棄後も同empty領域に加え、`MALLOC_LARGE (empty)`のvirtual 204.3 MiB / resident 144.8 MiBが残る。empty領域を持つzoneのallocated bytesは約29 KiBだった。最後の破棄でps RSSが65.875 MiB増える一方、別count系列では追加requestedが0だった。

Rc版の最後のphysical footprintは約151.8 MiB、footprint peakは約197.2 MiB。旧Vecは約159.0 / 249.7 MiBだった。RSS増加をそのままphysical footprintの増加へ読み替えない。empty領域のresident残存と解放境界の寄与は確認できたが、全期間RSS差22 MiBの各allocator内部要因への厳密な配賦はまだできていない。

`MallocSecureAllocator=0`などの指定系列でも大きな領域構成は変わらなかった。`MallocReportConfig=1`のstderrは空で、実設定を確認できていない。[Appleの公開実装](https://github.com/apple-oss-distributions/libmalloc/blob/main/src/malloc_config.c)はSecureAllocatorの環境指定を内部開発用policyが許される場合に限って読むため、環境指定だけで切替え済みとしない。[Appleの説明](https://github.com/apple-oss-distributions/libmalloc/blob/main/doc/xzone_malloc.md)にある型情報・caller PC・bucket配置は候補となる仕組みであり、今回の各差の原因を証明したものではない。

## 通常CLIと現在の統合版

旧Vec履歴版とmain `761c6771`の通常Rune release artifactを保存し、8/256/10,000要素・Int/String・map / flat_map一要素 / flat_map二要素の18条件を比較した。全内容検証36 processと長さだけを出力する216測定processを分離し、測定は全件artifact cache hit、各条件6回AB/BA。現在の統合版には他のruntime / compiler変更も含むため、Builderだけの因果比較ではない。

10,000要素のexecute中央値は以下の通り。小さい条件を含む全原本は証跡に保持した。

| 入力 | 旧Vec履歴版（ms） | 現在の統合版（ms） |
|---|---:|---:|
| Int map | 123.020 | 124.230 |
| Int flat_map一要素 | 397.045 | 399.663 |
| Int flat_map二要素 | 401.502 | 402.611 |
| String map | 123.308 | 124.747 |
| String flat_map一要素 | 668.760 | 670.732 |
| String flat_map二要素 | 671.477 | 673.981 |

一律の通常CLI速度改善や、旧Vec基準への追加費用解消は確認できない。RSSも増減が混在し、例えばInt二要素は41.758→42.273 MiB、String二要素は42.844→42.633 MiBだった。現在の有界chunk採用による保持量・実VMの改善は[採用記録](../list/packed_chunk_adoption_2026_10_15.md)で追跡し、この履歴Rc比較と区別する。

再現方法は[harness](../../../../tests/profile/runtime_rss_recheck/README.md)、原本は[rss_evidence_2026_10_15](rss_evidence_2026_10_15/validation.json)。OI-039の通常費用と残るallocator / OS内訳を台帳に維持する。
