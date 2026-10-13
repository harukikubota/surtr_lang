> 取り込み元: `/Users/haruca/.codex/worktrees/list-runtime-remaining/surtr`（detached、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# RT-2: 完成した Vec から chunk へ移す費用

測定日: 2026-10-13。入力は [指定された元レポート](../baseline/list_runtime_measurement_report_2026_10_11.md) と、main の [2026-10-11 の保持方式比較](list_tail_retention_comparison_2026_10_11.md)。前回のchunkモデルはpayloadを直接生成し、完成済みVecの移譲を測っていなかった。本書はその残件を補う。

手動モデルだけを追加したlevel2相当の作業であり、Listの製品表現は変更しない。測定rootはmain `72154707`から作成した作業treeで、同時に進めたRegex入力共有の差分を含む。ListHandleの実装はmainと同じで、使用するValueサイズは136 bytes。既存のBuilder改善が未コミットで存在する `9ea8` を書き換えず、VM/Builder経路の比較とも区別する。

## 結果と判断

完成済みVecからchunkへ移せば短いsuffixの保持量は減るが、元Vecと移譲先の外側bufferが一時に同時生存する。10万要素では外側bufferとchunk nodeで約12.97〜13.04 MiBの追加requested allocationが必要だった。Stringのpayload bufferはmoveしており、全文cloneは行わない。

したがって、前回の直接生成モデルだけを根拠に、完成したPacked/Vecをchunkへ置き換える構築費用を見積もれない。Builderから直接chunkを作る場合と、完成後に移す場合は別の経路である。採用方式、chunkサイズ、Consとの混在、scheduler予算・取消・checkpointの製品仕様は未決のままとする。

## 測定方法

[manual harness](/Users/haruca/.codex/worktrees/list-runtime-remaining/surtr/tests/profile/list_chunk_handoff/README.md)で、Packedの公開`ListHandle::from_items(Vec<Value>)`、所有Vecを`IntoIter`でmoveするchunkモデル、chunkへの直接生成を比較した。後方からmoveし、chunk内をreverseして論理順を戻す。空になった元Vecの解放までhandoff区間に含める。元Vecの空bufferを区間外まで残す比較にはしない。

n=10,000/100,000、位置番号付きInt、256-byte String、Stringを3段のsingleton Listで包むnested。capacity64/256/1024、所有条件は末尾1、中点suffix＋末尾、root clone＋末尾の3条件。root cloneはcheckpointの所有代理で、実VM checkpointではない。

126条件を各5回、割当counter有効/無効の2系列、合計1,260process・8,820区間行・1,764集計。方式とcounter/timeを交互にし、sampleごとに順序を反転した。準備、handoff、tail走査、root/sibling/checkpoint/最後のsuffixの破棄を分けた。JSONは全所有者の破棄後に出し、内容検証は別processとした。ビルド・テストとは重ねていない。

release/System allocator、root lockと同じ依存版・checksumを使用し、両binaryは`--locked`でbuildした。raw metadataにbinary/source/lock/runner hash、dirty状態、toolchain/OS、allocator環境を保存した。

## 完成済み Vec の移譲

n=100,000、末尾1のみの所有条件。時間はcounter無効系列、割当/論理peakは有効系列の中央値。論理peakはhandoff開始前のworkload liveに区間peak増分を足した値で、process RSSではない。

| payload | 方式/capacity | handoff ms | prebuild＋handoff ms | handoff追加requested MiB | handoff論理peak MiB | root破棄後の残存bytes |
|---|---|---:|---:|---:|---:|---:|
| int | packed/0 | 0.002042 | 2.561 | 0.000 | 16.022 | 16,800,008 |
| int | chunk-handoff/64 | 2.075791 | 4.679 | 13.042 | 29.063 | 5,424 |
| int | chunk-handoff/256 | 2.166666 | 4.811 | 12.988 | 29.010 | 26,928 |
| int | chunk-handoff/1024 | 1.922916 | 4.495 | 12.974 | 28.996 | 112,944 |
| string | packed/0 | 0.003000 | 17.880 | 0.000 | 37.384 | 39,200,040 |
| string | chunk-handoff/64 | 2.070958 | 19.756 | 13.042 | 50.426 | 12,592 |
| string | chunk-handoff/256 | 2.119834 | 19.906 | 12.988 | 50.372 | 62,768 |
| string | chunk-handoff/1024 | 1.907417 | 19.737 | 12.974 | 50.358 | 263,472 |
| nested | packed/0 | 0.000125 | 28.524 | 0.000 | 87.738 | 92,000,040 |
| nested | chunk-handoff/64 | 1.912958 | 30.411 | 13.042 | 100.780 | 29,488 |
| nested | chunk-handoff/256 | 1.991458 | 30.488 | 12.988 | 100.726 | 147,248 |
| nested | chunk-handoff/1024 | 1.759375 | 30.253 | 12.974 | 100.712 | 618,288 |

`prebuild＋handoff`は各process内で2区間を足してから集計した。中央値同士を足していない。`total_build_ns`は各phaseに同じ値を保存するが合算しない。Packed handoffは非空でRcの1割当・reallocation/free0で、元bufferをそのまま移す。微小区間の時間はallocatorの準備状態に左右され、payloadごとの固定費を推定しない。

chunk-handoffは2×chunk数のnode/buffer allocation、reallocation0、元Vecの`capacity×136` bytesだけをhandoffで解放した。10万要素/capacity256では782割当、13,618,768 requested bytes、元Vecの13,600,000 freed bytesだった。Value::cloneを呼ばないことと、Valueの物理移動bytes/allocator内部のコピーが0であることは別で、後者は計測していない。

## 共有所有と保持量

10万要素/capacity256のStringについて、root破棄後のrequested残存量を示す。

| 残る所有者 | Packed bytes | chunk-handoff bytes |
|---|---:|---:|
| 末尾1 | 39,200,040 | 62,768 |
| 中点suffix＋末尾 | 39,200,040 | 19,640,768 |
| root clone＋末尾 | 39,200,040 | 39,218,768 |

root cloneが生存する間はchunkも全要素を保持する。中点suffixがあればそれより前のchunkだけを解放できる。末尾だけなら最後のpartial chunkを保持し、capacity64/256/1024では32/160/672要素となる。サイズを大きくすれば必ず保持量が良くなるわけではない。

最後のsuffix破棄は軽くなるが、大量のpayload解放はroot/sibling/checkpointの破棄へ移る。全方式・全条件で最後の破棄後のworkload論理残存量は0、tail走査のallocation/reallocationも0だった。process resident memoryが0へ戻るという意味ではない。

## 直接生成との違いと RSS

chunk-directはprebuildが空でhandoffにpayload生成を含むため、完成済みVecのhandoff単独とは比較しない。同じprocessのprebuild＋handoffを使うと、10万String/capacity256はPacked17.880ms、chunk-handoff19.906ms、chunk-direct18.160msだった。これは今回の3モデルの観測であり、前回原本との速度差でも実VM/Builderの改善率でもない。

同条件のprocess peak RSSはPacked39.188MiB、chunk-handoff57.516MiB、chunk-direct44.578MiB。全期間高水位なので初期生成・全7区間・JSONを含む。保持量の減少だけでpeak RSSの低下を主張できない。

## 検証と残件

- release/count・time build成功、各126条件×5回の両系列は終了コード0。
- [別processの検証](list_chunk_handoff_2026_10_13_verify.json)720件成功。空/1/2要素、chunk境界と2倍境界の前後、大きい入力で長さ・順序・内容・所有元破棄後の有効性を確認した。Stringの元buffer pointerと移譲後pointerも一致した。nestedの内部pointerは取得していない。
- counter無効3smokeで未測定allocation/live/peakがnullになることを確認した。
- 製品Listの公開from_itemsによるbuffer移譲は既存Sindr unit testが正本となる。新モデルのPointer/APIを製品へ追加していない。
- Astraの静的レビューで必須修正なし。Astraが8,820行/1,764群の統計、各processのbuild和、最後の論理残存0、検証binaryのhashも照合した。Astraはビルド・テストを再実行していない。

[生データ](list_chunk_handoff_2026_10_13.jsonl)・[集計](list_chunk_handoff_2026_10_13.summary.json)を保存した。製品変更・採用・Cons混在・Builder/VM接続・予算/取消/rollback・実checkpointは未実施で、RT-2は未解決である。次は完成済みVecを後処理する方式と、Builderから直接chunkを構築する方式を分け、所有者と構築peakを含む移行仕様を決める。
