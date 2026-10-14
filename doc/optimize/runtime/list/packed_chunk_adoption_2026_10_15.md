# RT-2: 256要素chunkの採用とBuilder完成

入力は[残り6分野の計画](../remaining_implementation_2026_10_14.md)とユーザーの256要素chunk方式の承認。main `eec8c9b6`を基準に、SindrのPackedとEldrの共通Builderを変更した。内部表現・完成予算にまたがるlevel4。構築費用を許容して、短いtailが元の全bufferを保持する問題を解消する。

## 実装と契約

Packedは最大256要素のVecと次のListを持つ不変chunkを共有する。chunk内はoffsetを進め、終端では次のListを共有する。Consとの混在、論理順・永続性とhead / tail / lenの仮想O(1)を保つ。使用中chunk内の先頭部分は保持するが、それより前のchunkを保持しない。元Listやcheckpointが生きていれば、その所有分は残る。

`from_items`は完成済みVecのValueをcloneせずchunkへ移す。元Vecそのものを全長で一括共有する旧経路は削除した。移し替えの線形費用と一時二重bufferは残る。小さいVecも過剰な予約容量をchunkへ持ち越さない。内部の`prepend_items`は空・257要素以上・長さoverflowを拒否し、tailを補正するfallbackを置かない。Cons / Packedの混在鎖は反復解放する。

Builderの予約とsegmentは256要素までとし、満杯または共有末尾への追加では新しいsegmentを作る。保存したValueを全cloneしない。独占segmentは一回の完成遷移で最大256要素のbufferをそのままPacked chunkへ移す。共有segmentは一要素ずつcloneしてConsへ接続する。境界と終端配送も共通予算に従う。完成途中の保存、取消・復帰、Generatorのrestとcallback回数を維持する。入力長で旧方式へ切り替える経路を設けない。

## 製品Listの保持量と構築費用

同じ既存probeの公開`ListHandle::from_items` / `tail_handle` / Dropだけで前後を比較した。過去の局所chunkモデルは使用していない。10,000 / 100,000要素、位置付きInt・256-byte String・3段のsingleton Listに入れたString、末尾だけ・中点suffix併存・root checkpoint併存の18条件。count / timeを独立したrelease binary・targetで構築し、各版6 sampleをAB / BA同数、432 fresh processで測った。全内容は別の36 processで確認した。

10万要素から末尾1要素だけを残してrootを破棄した場合の要求layout残存量は次のとおり。物理メモリやRSSではない。

| payload | 変更前 bytes | 変更後 bytes |
|---|---:|---:|
| Int | 16,800,008 | 26,952 |
| String | 39,200,040 | 62,792 |
| nested String | 92,000,040 | 162,632 |

末尾chunkは160要素。中点suffixを残したStringは39,200,040→19,645,472 bytesとなり、root checkpoint併存では39,200,040→39,228,152 bytesで全体を保持した。全条件でtail走査のalloc / reallocは0、全所有者破棄後の論理残存量は0だった。

構築費用は増えた。同じ10万要素・末尾だけの条件で、Intの構築中央値2.629→4.687 ms、Stringは18.425→20.469 ms、nestedは29.651→33.957 ms。構築要求はInt 16,800,008→30,428,120 bytes、String 66,100,040→79,728,152 bytes、nested 118,900,040→142,128,152 bytesだった。native `from_items`の後処理では元bufferと新chunk bufferが一時的に重なる。

time専用processの全期間peak RSS中央値はInt 18,563,072→37,945,344 bytes、String 41,140,224→60,383,232 bytes、nested 104,071,168→132,956,160 bytesだった。準備・全区間・JSONを含む高水位であり、suffix保持量ではない。nestedのroot全体の論理保持量も92,000,040→101,628,152 bytesへ増えた。各singleton chunkがtail fieldを持つ追加費用を含む。短いtailの保持削減と、構築・metadata・peakの増加を両方採用判断へ残す。

[原本](packed_chunk_adoption_2026_10_15.jsonl)・[集計と4 buildの来歴](packed_chunk_adoption_2026_10_15.summary.json)。3,276区間行。確保計数系列と内容検証系列の時間は記録せず、時間系列の未測定確保値はnullにした。

## 実VMのBuilderへの影響

既存の実VM checkpoint probeを、32 / 1 Task、保存前長さ1,024 / 16,384、Int / Stringの8条件で比較した。既存のshared方式同士を前後6 sample、各process8反復の96 fresh process、7,680区間行で測った。内容確認は別16 process。前後のrelease Cargo test executableと70 source / manifest / lockのhashを保存した。logical counterとtest用System wrapperは前後に残り、確保量は計数していない。

32 Task・長さ16,384の時間中央値と全期間peak RSS中央値は次のとおり。RSSは全phaseと8反復を含むため、unique完成だけの値ではない。

| payload | unique完成 ms 前→後 | shared完成 ms 前→後 | process peak RSS MiB 前→後 |
|---|---:|---:|---:|
| Int | 12.423 → 11.918 | 34.062 → 34.421 | 279.406 → 235.055 |
| String | 13.429 → 12.438 | 45.729 → 43.254 | 541.648 → 494.813 |

Intのunique完成の範囲は12.017〜12.943 / 11.650〜13.782 msと重なる。Stringは13.045〜13.566 / 12.317〜12.948 msだった。shared完成の範囲は両payloadで重なり、Intの中央値は増えている。すべての条件の高速化は主張しない。

保存後の初回変更・再変更での保存Value cloneは前後0、shared完成のsource cloneは32 × 16,384のまま。unique完成の遷移は32→2,304回となり、長いbufferの完成を複数の有界移譲へ分けた。native Vec後処理と異なり、Builderは初めから有界segmentへ構築し、そのbufferを移す。この2経路を混ぜない。

[実VM原本](../checkpoint/builder_chunk_adoption_2026_10_15.jsonl)・[集計と2 buildの来歴](../checkpoint/builder_chunk_adoption_2026_10_15.summary.json)。OI-039の以前のVec基準への遅化率や、allocator / OSのRSS原因を特定した結果ではない。通常CLIは未測定。

## 検証とレビュー

旧実装で保持上限と長い独占完成の2件が狙った理由で失敗し、変更後はSindr / Eldrの524件が成功した。境界255 / 256 / 257 / 511 / 512 / 513、payload pointer、過剰予約、invalid chunk、Weakによる保持と解放、巨大混在鎖、保存・取消・完成中復帰を確認した。独立レビューのchecked加減算の指摘を採用し、未解決指摘は0。

最初のworkspace CIは2,575 passed / LSP 1 timed out / skipped 0。release buildも並行していたが、タイムアウトの原因は断定しない。該当の注入executorエラーテストは単独で1 passed / 15 skipped（4.897秒）。その後buildを重ねず、最終sourceでCI 2,576 passed / 69 binaries / 失敗・除外0（111.777秒）と標準1,124件すべて成功、終了コードはともに0。標準のfailed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0。

[検証記録](packed_chunk_adoption_2026_10_15_validation.json)へ初回失敗も保存した。性能系列中はbuild・他のtestを行わず、最終sourceと保存したartifact / binary / runner hashの一致を確認した。Rust fmt、差分空白、Python構文と今回のリンクも確認した。独立レビューがnativeの504群・実VMの160群を原本から再計算し、統計と来歴の不一致0、未解決指摘0を確認した。公開言語API・Opcode・VM/schema versionは変更していない。

Packed tailの採用・製品移行は完了した。構築時の追加費用と他ownerの保持は方式上の制約として残す。Builderの通常CLIとOI-039の原因調査、Future / HashMap / Regex / FileOutと他RSS調査は[実施計画](../remaining_implementation_2026_10_14.md)で継続する。
