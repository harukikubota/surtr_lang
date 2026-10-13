# OI-039 直接所有末尾の機械語照合

`before-binary` は全体をRc segmentに置く採用版、`direct-binary` は各Builder専有のRefCell内で末尾Vecを直接所有し、Clone時にだけfreezeする候補。旧main Vec全clone版との比較ではない。入力binaryを読み取るだけで、ビルド・テスト・性能測定は実行していない。候補sourceは `direct-list-builder.rs` に固定されており、現在のcheckoutの一時的な内容から推測していない。

## 再現

```sh
python3 /var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-9ea8-owned-tail-ggh5r2z4/inspect_asm.py
```

スクリプトはnmでsymbolを解決し、objdumpの `--disassemble-symbols` でrun_quantum、step_context、FlatMapContinuation::resume、ListBuilder::pushを読む。binary SHA-256、実コマンド、全関数の命令数・prologueは `asm-manifest.json` に記録する。`before/direct-asm-*.txt` が原出力である。ソースやbinaryは変更しない。

## 通常のOwned追加

両版ともFlatMap内へpushがinlineされ、FlatMapから独立したpush関数への呼出しはない。独立symbolは別callsite用に残る。

直接所有版FlatMapの `0x10020f2d4`〜`0x10020f2e0` はOwned/Frozenのtag判定。Ownedでは `0x10020f33c` へ分岐する。FrozenにだけRc所有確認がある。

```asm
10020f2d4: ldr   x22, [x21, #0xd0]!
10020f2d8: mov   x8, #-0x8000000000000000
10020f2dc: cmp   x22, x8
10020f2e0: b.ne  0x10020f33c
; 以下のweak/strong確認はFrozenだけ
10020f2e8: ldr   x8, [x22, #0x8]
10020f2ec: cmp   x8, #0x1
10020f2f0: b.ne  0x10020fa3c
10020f2f4: ldr   x8, [x22]
10020f2f8: cmp   x8, #0x1
10020f2fc: b.ne  0x10020fa3c
```

Ownedでは `0x10020f33c`〜`0x10020f344` でlen/capacityを比べ、余裕があれば `0x10020f4a4` へ進み、 `0x10020f35c` のreserve呼出しを避ける。続く `0x10020f4d0`〜`0x10020f4dc` のgrow呼出しも余裕時には避ける。

RefCellのborrow flagはBuilder先頭の位置にあるが、通常pushは `RefCell::get_mut(&mut self)` を使うため、そのflagを読んだり確認したりする機械語はない。enumのOwned/Frozen判定は残る。checkpointのCloneはborrow_mutを使い、そこでfreezeする。すべてのRefCell検査が消えたという意味ではない。

Valueの受け渡しには一時領域が残る。`0x10020f4a8`〜`0x10020f4cc` でstack上の別領域へコピーし、 `0x10020f4ec`〜`0x10020f518` でVecへ読む。Value strideは以前と同じ `0x88 = 136 bytes`。Owned化はRc検査を除いたが、Valueのstackコピーまで除去した版ではない。

独立push関数でも `0x100248ae4`〜`0x100248af0` のstate判定がOwnedを `0x100248c60` へ送り、Frozen専用のRc検査を避ける。Ownedのreserve/growはcapacity不足時にだけ呼び、stack経由のValue追加を行う。

## 型幅とVMへの波及

機械語でBuilderの総長fieldはoffset `0x8` から `0x28` へ移った。FlatMap内のBuilder先頭は両版とも `0xc8`、総長は `0xd0`→`0xf0`。次のawaiting_mapper fieldも `0xd8`→`0xf8` なので、今回のlayoutではBuilderは16→48 bytesと読み取れる。想定56 bytesより小さいのはenumのniche配置による。これはこのcompiler/binaryでの内部layoutで、Rustの安定ABIや将来のサイズ保証ではない。

| 項目 | before | direct | 差 |
|---|---:|---:|---:|
| pending invocationの移動byte数 | 728 (`0x2d8`) | 760 (`0x2f8`) | +32 |
| ExecutionContextの移動byte数 | 904 (`0x388`) | 936 (`0x3a8`) | +32 |
| cancellationのcontext内offset | `0x360` | `0x380` | +32 |
| run_quantumのprologue stack確保 | `0xa40` | `0xaa0` | +96 |
| step_contextのprologue stack確保 | `0x1050` | `0x10d0` | +128 |
| FlatMap resumeのprologue stack確保 | `0x3e0` | `0x3f0` | +16 |

移動幅の証拠はbefore run_quantum `0x100032560` の728-byte移動、`0x1000326bc` の904-byte移動に対し、direct `0x100032668` の760-byte移動、`0x1000327c4` の936-byte移動。cancellation読取はbefore `0x1000324f0`、direct `0x1000325f8`。幅増が実際のVM移動とstackへ波及している。

FlatMap全関数は1,022→1,157命令、独立pushは225→373命令。Frozenのfreeze解除・枝分離・unwindも含む全関数規模であり、通常Owned追加の実行命令数を表していない。検査削減と型幅増のどちらが支配的かは交互測定で判断する。

## sourceの所有境界点検

各Builderが独立したRefCellを持ち、共有するのは不変segmentのRcだけ。通常追加はOwned Vecへ直接行い、CloneだけがOwned VecをRcへ移して元と複製をFrozenにする。freeze中にValue cloneや利用者callbackを呼ばない。Debugはlenだけを表示し、内部borrowを外へ公開しない。

空Cloneは元の予約を維持して新側Defaultを返す。Frozenの独占再追加はreserve成功後にunwrapする。共有再追加も新Vecのreserve・push成功後にstateを移すので、capacityエラーまでの論理内容と長さを保つ。総長overflowは先にchecked_addで拒否する。finish stateとsegmentの反復Dropは変更していない。読み取り点検で追加の正しさの懸念は見つからなかった。速度や全条件の改善を証明した点検ではない。

## Boxでcellを別体にした候補

`box-binary` と `box-list-builder.rs` の証拠を追加した。再現スクリプト・manifestは三者を読み、clone/finishの独立symbolもある場合は保存する。候補は `Option<Box<RefCell<BuilderState>>>` をBuilderが専有する。空DefaultはNoneなので最初の追加までcellのallocationもない。

| 項目 | before | direct | box |
|---|---:|---:|---:|
| Builder幅 | 16 | 48 | 16 |
| pending invocation移動幅 | 728 | 760 | 728 |
| ExecutionContext移動幅 | 904 | 936 | 904 |
| cancellation offset | `0x360` | `0x380` | `0x360` |
| run_quantum stack | `0xa40` | `0xaa0` | `0xa40` |
| step_context stack | `0x1050` | `0x10d0` | `0x1050` |
| FlatMap resume stack | `0x3e0` | `0x3f0` | `0x3f0` |
| FlatMap全関数命令数 | 1,022 | 1,157 | 1,207 |

box版の幅復帰はrun_quantum `0x100032698` の728-byte移動、`0x1000327f4` の904-byte移動、`0x100032628` のcancellation読取で確認できる。Builder総長fieldは独立push `0x100264150` にoffset0x8であり、Box pointerとlenを合わせて16 bytes。directで生じたVM移動幅と大きいstackの増加が戻ったことを確認した。FlatMap関数内のstackやコード規模は元に戻らない。

### Owned追加

FlatMap `0x100212170` / `0x100212174` はcell Box pointerの存在を確認する。存在すれば `0x1002121bc` へ進み、`0x1002121c0`〜`0x1002121cc` のOwned/Frozen判定を行う。Ownedは `0x10021222c` へ進み、Frozen専用の `0x1002121d8`〜`0x1002121ec` のRc weak/strong確認を避ける。RefCellのborrow flag読取/確認もOwned pushにはない。

Valueは `0x100212398`〜`0x1002123bc` の一時領域へのコピーと `0x1002123dc`〜`0x100212408` のVecへのコピーを経由する。通常追加のRc検査は除いたが、Box indirectionと空pointer確認とstate判定、一時コピーは残る。

### allocationを実行する位置

- **最初の追加**: BoxがNoneの場合だけ、FlatMap `0x10021219c` が40-byte cellをallocateする。予約容量を持つconstructorもこのcellを持つ。既存のOwned末尾への通常追加にcell allocationはない。
- **Clone**: `0x1001f04ac` はOwned Vecをfreezeする48-byte Rc Segmentのallocation。すでにFrozenの場合はこのallocationを避ける。どちらの場合も非空の複製には `0x1001f053c` で独立した40-byte cell Boxをallocateする。新側と元側で可変cellは共有しない。
- **Cloneのborrow**: `0x1001f0424` / `0x1001f0428` がborrow flagを調べ、`0x1001f0430` でmutable borrowを記録する。これは通常pushではなくClone時の検査。
- **finish**: Owned非空のとき `0x1001eceb4` で48-byte Rc Segmentをallocateし、`0x1001ecedc` で40-byte cell Boxをfreeする。FrozenではRc allocationなしでheadを移し、cell Boxをfreeする。完成継続の共有・予算経路は既存と同じ。

Box版は直接cell版の幅増を避けられた一方、cellの確保・解放と間接参照が追加された。これらは保存・最初の変更・完成・準備の別々の位置にあるため、通常追加だけの軽さを全処理の改善と扱わない。交互測定と割当counterで採否を判断する。
