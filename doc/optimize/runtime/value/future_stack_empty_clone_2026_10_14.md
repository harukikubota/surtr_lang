# OI-036: 空のFuture追跡bufferをcloneしない

作業日: 2026-10-14。入力は[runtime一覧](../README.md)と[位置bufferの保持・clone費用の残件](future_stack_guard_optimization_2026_10_13.md#残件)。基準はmain `d6c753b1`。Eldr内のlevel2変更として、配送とrollbackの意味を保ち、空になった追跡bufferの不要な確保を減らす。

## 方式と受入条件

Futureをpop / truncate / clear / ready置換で除いた元のstackは、従来どおり位置VecとBoxを再利用する。Cloneは値と現在の追跡位置を複製し、位置が空なら複製先の位置bufferを確保しない。位置がある場合は独立したbufferを複製する。元の予約容量は変更しない。

- 空の追跡位置を持つcloneはBoxを持たず、元のBoxと位置Vecの容量・pointerを保つ。
- clone後の双方のpush、ready置換、popを独立に行い、保存側の値・位置を変更しない。
- cloneの値と位置は元と等しい。空の予約bufferの有無をprivate OperandStackの等値条件に含めない。値と論理位置を比較する。
- 未解決Future、同一IDの重複slot、Futureを返すFuture、context切替え・cleanup・checkpoint / rollbackを維持する。
- 値Vecのclone自体は残す。追加状態、公開API、Opcode、VM/schema versionは増やさず、位置不一致のfallbackは設けない。

局所的な所有条件の選択であり、利用者が観測する値や実行順序を変えない。private Debugに含まれる予約bufferの表示はClone前後で異なり得る。

## 手順と検証

1. pop / truncate / clear / ready置換後のcloneを対象に、不要なbufferのない複製と元の予約容量の維持を固定する。現在のderive CloneでRedを確認する。
2. Cloneを現在の値・位置に沿う方式へ置き換え、等値も論理値・位置で比較する。元のbuffer再利用と、clone枝の独立性を検証する。
3. 同じOperandStackソースを取り込む独立したrelease probeで、System allocatorへの確保要求と時間を別processで比較する。never / clear / pop / truncate / resolved / pendingの対照を分け、全内容検証は別processにする。VM全体やRSSの効果へ一般化しない。
4. 正本EldrVMとテスト方針を整合させ、関連テストからworkspace CIと標準Surtrの全件一回まで実行する。最終差分を自己点検し、結果と残件を追記する。

## Red / Greenと独立性

derive Cloneのまま追加したテストは、pop後の複製にも位置bufferのBoxがあるため失敗した（終了コード100）。実装後は関連17件が成功した（363 skipped、終了コード0）。同じテストでtruncate / clear / ready置換後も確認し、元のBox pointer・位置buffer pointer・容量を維持すること、複製先への再追加と解決が元の未解決Futureを変更しないことを固定した。

Cloneは値Vecを従来どおり複製し、位置が非空の場合だけBoxと位置Vecを複製する。PartialEqは値と論理位置を比較し、空の予約bufferの有無を除く。使用済みbufferを持つ元と、bufferなしのcloneも等しく扱う。置き換えたderive Clone / PartialEqは残していない。

## 確保要求の測定

[standalone probe](../../../../tests/profile/future_stack_clone/README.md)は、測定対象のOperandStackソースそのものを取り込み、Cargo artifactから得たSindrのrelease libraryへリンクする。Systemへ渡すRust GlobalAllocの確保・解放を区間内で数える。このallocatorは独立probeにだけ置き、Eldrの製品・通常テストをinstrumentしない。

6条件×普通値数0 / 16 / 256×前後×allocation / time×6 sampleで432 fresh process、clone / dropの864行を記録した。全内容と等値の確認は別processの36件。各条件・modeの前後6 sampleはAB/BA同数とし、測定中にbuild・testを重ねていない。1 processで1,000個を複製し、準備したroot Vecに全複製を保持してから破棄する。元のstack、root Vecの確保、準備・出力は区間外にする。

普通値16個、1,000 clone分の確保要求は次のとおり。UnitのValueはこのbuildでは136 bytes、位置VecのBox payloadは24 bytes。clearedは普通値も除いた状態、resolvedはFutureをUnitへ置換した状態、pendingは二つの同一IDのFutureを追加した状態である。

| 条件 | cloneのalloc回数 前→後 | requested bytes 前→後 | clone区間live / peak bytes 前→後 |
|---|---:|---:|---:|
| never | 1,000 → 1,000 | 2,176,000 → 2,176,000 | 2,176,000 → 2,176,000 |
| cleared | 1,000 → 0 | 24,000 → 0 | 24,000 → 0 |
| popped | 2,000 → 1,000 | 2,200,000 → 2,176,000 | 2,200,000 → 2,176,000 |
| truncated | 2,000 → 1,000 | 2,200,000 → 2,176,000 | 2,200,000 → 2,176,000 |
| resolved | 2,000 → 1,000 | 2,336,000 → 2,312,000 | 2,336,000 → 2,312,000 |
| pending | 3,000 → 3,000 | 2,488,000 → 2,488,000 | 2,488,000 → 2,488,000 |

pop / truncate / clear / ready置換で位置が空になった条件は、全入力長で1 cloneあたり1確保・24 bytesを除いた。未解決位置がある対照と、位置bufferを持ったことがない対照の確保要求は前後一致した。値Vecのcloneと、未解決Futureの位置コピーは残る。

clone区間は解放・reallocが0、drop区間は新しい確保が0で、cloneで確保した全bytesを解放し、区間内liveが0へ戻ることを各sampleで検証した。このlive / peakは区間内で数えたRust確保要求の合計で、allocatorの内部metadata・丸め・page保持やprocess RSSは含まない。元の予約bufferの保持量も区間外にある。RSSは測っていない。

## 時間の対照

allocator counterを無効にした別processでclone / dropを測った。無効判定のwrapper自体は前後に残るため、通常のallocatorだけの時間とは区別する。普通値16個、1,000個のcloneとdropそれぞれの中央値はmsで次のとおり。

| 条件 | clone ms 前→後 | drop ms 前→後 |
|---|---:|---:|
| never | 0.260 → 0.256 | 0.051 → 0.048 |
| cleared | 0.018 → 0.004 | 0.012 → 0.001 |
| popped | 0.233 → 0.226 | 0.054 → 0.042 |
| truncated | 0.248 → 0.237 | 0.056 → 0.042 |
| resolved | 0.248 → 0.234 | 0.059 → 0.045 |
| pending | 0.291 → 0.296 | 0.072 → 0.073 |

clear済みの空値stackでは、普通値数0 / 16 / 256の準備条件すべてで約0.018→0.004msへ減った。一方、neverの普通値0個は0.003646→0.004417ms、poppedの普通値256個は3.866→3.884msであり、一律の高速化は主張しない。値コピーが支配する条件では小さい時間差が残る。VmCheckpoint / VM clone / REPL全体の時間は今回測っていない。

[原本](future_stack_empty_clone_2026_10_14.jsonl)・[集計](future_stack_empty_clone_2026_10_14.summary.json)・[別process検証](future_stack_empty_clone_2026_10_14_verify.json)・[build来歴](future_stack_empty_clone_2026_10_14_builds.json)。source/library/binary hash、Cargo manifest、rustc versionとflagsを保存した。[基準patch](future_stack_empty_clone_2026_10_14_baseline.patch)は`d6c753b1`へ受入テストだけを追加した状態を再現する。独立した一時checkoutへ適用して、測定した基準sourceとのbyte一致を確認した。

standalone buildの初回はdependency directoryをrelease直下にしてしまい、Sindrをロードできなかった。Cargo artifactのdependency directoryへ修正した。採用した測定系列はすべて成功し、初回のbuild失敗はRedや性能原本に含めていない。

## 最終検証

- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/)'`: 17 passed、363 skipped、終了コード0。
- 同じtargetの`rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,560 passed、69 binaries、失敗・除外0、終了コード0、nextest部分115.222秒。
- 同じtargetの`rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。failed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0、終了コード0。
- Rust workspace fmt、製品・文書・harnessの差分空白検査、Python構文、ローカル文書リンク、sourceと測定binaryのhash一致を確認した。baseline patchは原本として保持する。

[検証記録](future_stack_empty_clone_2026_10_14_validation.json)に終了コード、標準summary、Cargo artifact、最終source hashを保存した。測定・CI・標準テスト中は製品ソースを変更していない。差分を自己点検し、元の予約容量・clone枝の独立性・等値・非空位置の複製と、既存の配送・cleanup・context・checkpoint / rollbackの維持を確認した。level2のため別エージェントは使っていない。

## 残件

元の使用済み位置bufferの保持、値Vecのclone、未解決位置のコピー、pointer幅metadata、Future表・cleanup検索は残る。今回の24 bytesはcloneごとの要求削減であり、VM全体の保持量やRSSの改善とは扱わない。通常CLIのFutureなしの小さい時間差についても新しい解消結果を主張しない。

その後、[非空位置表の共有](future_stack_position_sharing_2026_10_14.md)を追加した。上記のBox版の挙動と原本は測定時点の記録として保持する。
