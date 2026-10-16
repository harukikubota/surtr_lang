# Specialization と trait 前宣言のコンパイル時間短縮

2026-10-16。[03_compile_time.md](03_compile_time.md) の P2a / P2b を実施する。
開始時HEADは `61267fd25081b44223e806a7512f12f4573a8c4d`。意味論・診断・公開API・保存形式を維持するlevel2のScar内部最適化とする。

## 対象と手順

- P2a: 一回の `specialize_program` で使う定義の不変summaryを共有し、要求の伝播を逆辺worklistに置き換える。保持済み定義の最終検査、specialization key、再帰用function indexの予約、生成順を維持する。
- P2b: trait / implを追加するsuffixの前宣言を局所計測する。新規宣言に必要な検査と既存implの再検査を分け、追加によって影響されない仕事を減らす。重複・曖昧性・循環、親impl被覆、constructor slot mapping、methodの型引数とwhere制約を維持する。
- 永続summary、checkpointのwire layout、schema / VM version、trait proofのglobal cacheは変更しない。P3 / P4とprocess warm / incremental専用の性能harnessは対象外とする。

サブエージェントがP2aとP2bを分担し、主担当が変更前ソース・binaryの保存、全体検証、性能比較を行う。各領域は契約と処理量を直接確認するRustテストから始め、変更前のRedと変更後のGreenを記録する。最終差分は別エージェントがレビューする。

正本は [Trait_system_spec](../../docs/dev/Trait_system_spec.md)、[Xldr_spec](../../docs/dev/Xldr_spec.md)、[Rune_observability](../../docs/dev/Rune_observability.md)、[テスト方針](../../docs/dev/テスト方針.md) とする。宣言順非依存、source span、失敗後のrollbackとsibling sessionの分離を既存・追加テストで確認する。

## 測定と採用条件

変更前の clean HEAD を一時ディレクトリへ保存し、before / after の release target と semantic cache を分離する。同じ入力・標準ソース・Rust toolchain・build profile で比較し、source commit / diff、binary hash、電源条件を記録する。測定中は他のビルド・テストを止める。

[cases.json](../../tests/profile/scar_compile/cases.json) の全24入口を各 binary・cold / disk hit ごとに10回測る。[measure.py](../../tests/profile/scar_compile/measure.py) で実行順を交互にし、成功、cache 状態、artifact 不使用、execute 省略を検査する。profiler は無効にし、原因の帰属と処理量は別走行で確認する。

採用目安はdisk-hitのgeneric chain正逆64段およびtrait impl 64件で `typecheck` 中央値を10%以上短縮することとする。各系列16 / 32 / 64の処理量も調べ、走査順による悪化がないことを確認する。代表入力の `compile_total` に5%超の悪化が繰り返す場合は原因を確認する。差が1ms未満の場合や分布が重なる場合は、小さい差だけで効果・回帰を断定しない。各領域の効果が確認できない変更は採用しない。

Scarの局所テスト後、CI profileのworkspaceで全件成功を一回確認し、標準Surtrテストを確認する。通常debug stackのgeneric chain正逆16 / 32 / 64も実行する。全体テストの時間は性能比較へ含めない。

## 実施記録

### 実装

P2aは [summary.rs](../../crates/scar/src/checker/specialize/summary.rs) で、一回のcompileに閉じたsignature・Pattern要求・呼出し辺・pending trait / do metadataを収集する。呼出し辺は元定義のsubstitutionとspanを借用するため、substitutionを複製しない。direct Patternとordinaryの伝播は逆辺worklistを共有し、それぞれの投影規則を保つ。

- direct Pattern: `node.ty`のUserFuncを対象に、substitutionが直接 `Ty::Var` の場合だけ宣言のslotへ要求を伝える。
- ordinary: Patternに保持されたcallableも対象にし、`resolve_ty`後の複合型内の型変数を伝える。必要slotの欠落は元call spanのinvariant errorとして拒否する。

保持済み定義のsummaryもcompileごとに作る。sessionをまたぐsummaryの保存は追加していない。キー・再帰index予約・rewrite・全体の最終auditは既存処理を保つ。

P2bの調査では、constructor mappingとmethod signature構築は、すでに新規宣言を対象としていた。そこで [traits.rs](../../crates/scar/src/checker/predeclare/traits.rs) のoverlap / Convert排他検査を既存のbase trait indexへ限定し、新規methodの検索をcompile内のdeclaration ID→impl key mapへ置き換えた。通常traitではConvert / TryConvert固有の短名検索も省く。全head収集→constructor mapping・親被覆→method構築の順序を維持する。

複数の既存implとoverlapする不正入力では、診断に出る既存targetの選択順が、従来の未規定HashMap順からindexの登録順になる場合がある。拒否条件、新規宣言のspan、hintは維持する。

### 処理量と契約テスト

P2aのcrate-localな16段の鎖は、変更前に240辺を走査し、線形上限30のassertionで意図どおり失敗した。変更後の正逆16 / 32 / 64段は、それぞれ15 / 31 / 63辺だった。この値はsyntheticな要求伝播テストの計数であり、CLI全体の処理回数ではない。

P2bは、追加traitと無関係な既存implを147件走査するRedを確認し、変更後は0件となった。新規mappingとmethod lookupは各1件で、保持済みmetadataのArcを変更しないことも確認した。

focused検証は、P2aが14件、P2bと関連するmapping / registryが6件成功した。直接Patternと複合型の投影差、相互再帰の収束、seedのsort、欠落slotの診断span、生成開始後の失敗→checkpoint復元→異型specializationとsiblingの一致、suffix overlapと失敗sessionの不変性を検証した。

### 性能と採用判断

Apple M2 / macOS 26.6.2 / Rust 1.94.1、AC電源で測定した。他のビルド・テストは止め、OSのpage cache、CPU周波数、他アプリの負荷は固定していない。beforeは開始時HEADのclean archive、afterは上記P2a / P2bを合わせたソースである。標準ソース130ファイル、Cargo設定、toolchain、build環境の一致と、最終作業ツリーの全Rustソースがafter保存ソースに一致することを確認した。

全24入口 × before / after × cold / disk hit × 10回、計960回が成功した。cache条件、artifact不使用、typecheck実行、execute省略、profiler無効を検査し、測定前後の入力・標準ソース・binary hashも一致した。次の表は **P2a＋P2b全体** の比較であり、各行を個別候補だけの効果とは扱わない。単位はms、各10回の中央値。

| 入力・disk hit | 指標 | before | after | 短縮 |
|---|---|---:|---:|---:|
| generic chain 正順64段 | typecheck | 8.932 | 8.488 | 4.97% |
| generic chain 逆順64段 | typecheck | 10.526 | 8.538 | 18.88% |
| trait impl 64件 | typecheck | 5.545 | 4.825 | 12.99% |
| generic chain 正順64段 | compile total | 48.897 | 48.555 | 0.70% |
| generic chain 逆順64段 | compile total | 50.555 | 48.866 | 3.34% |
| trait impl 64件 | compile total | 33.724 | 32.891 | 2.47% |

**正順64段の10%目安は未達**。差は0.444msで、beforeの8.767〜9.191msとafterの8.430〜8.855msは一部重なる。逆順64段の約1.99ms短縮、正逆の時間差の縮小、処理量テストの線形化を合わせてP2aを採用する。

trait impl 16 / 32 / 64件のtypecheckは13.01 / 14.15 / 12.99%短縮した。絶対差は各1ms未満だが、三つの規模すべてでbefore / afterの最小最大が分離し、対応する10対すべてでafterが短かった。後述の候補走査削減と局所profileを合わせてP2bを採用する。12.99%すべてがP2b単独の効果という意味ではない。

全24入口のdisk-hit compile totalは3.34%短縮〜0.052%増加、coldは3.43%短縮〜1.67%増加だった。5%超の中央値悪化はない。coldのstdlib loadは3.09%短縮〜1.92%増加であり、今回の変更によるcold全体の短縮は主張しない。

各回の値・stdout / stderrは [runs.jsonl.gz](evidence/p2/runs.jsonl.gz)、中央値・最小最大は [summary.json](evidence/p2/summary.json)、source / binary / 入力hash・toolchain・電源・順序は [manifest.json](evidence/p2/manifest.json) に保存した。生成 `.eldr` とcache payloadは一時測定先に残した。

### 別走行の内部profile

全24入口を各binary・cold / disk hitで一回ずつ、計96回実行した。通常測定と混ぜず、入れ子の時間を加算しない。beforeは開始時ソースにP2bの局所計装だけを加えた別binaryを使い、[差分](evidence/p2/p2b-profile-before.patch) と [manifest](evidence/p2/before-profile-manifest.json) を保存した。afterは通常測定と同じ保存binaryである。

disk hitの `specialize_program` は、正順64段で2.911→2.422ms、逆順64段で4.554→2.670msだった。trait implの登録・coherence・method lookupは次のとおり。

| impl数 | 登録時間 before→after（ms） | coherence候補訪問 before→after | method lookup訪問 before→after |
|---|---:|---:|---:|
| 16 | 0.195→0.094 | 2,456→120 | 1,258→16 |
| 32 | 0.352→0.186 | 5,168→496 | 2,898→32 |
| 64 | 0.737→0.400 | 11,360→2,016 | 6,929→64 |

同じtrait内の必要な組合せ検査は残る。constructor mappingの対象はbefore / afterとも16 / 32 / 64件で、既存implの再mappingを除いた結果ではない。旧method lookupの訪問数はHashMapの順序で変わる。一回ずつのprofile時間を安定した速度差とは扱わず、通常測定と処理量の根拠を補う。[全profile記録](evidence/p2/profiles.json)を参照。

### 検証とレビュー

| 検証 | 結果 |
|---|---|
| P2aのspecialization内部テスト | 14件成功 |
| P2bのincremental predeclare・mapping・registry | 6件成功 |
| `rtk cargo nextest run --profile ci --workspace` | 最終Rustソースで2,635件成功、70 binaries、除外・失敗なし |
| `rtk proxy cargo run --release --manifest-path AFTER/Cargo.toml -p rune --bin surtr -- test --quiet --all` | after保存ソース・専用targetで終了コード0。quiet出力に件数はない |
| 通常debug CLI、generic chain正逆16 / 32 / 64段 | 全6入口成功。8176 KiB stack、`RUST_MIN_STACK`未指定 |
| 対象3 Rustファイルの`rustfmt --check`、`git diff --check` | 成功 |
| 独立レビュー | 最終差分・検証結果・960走行の集計を確認。未解決指摘なし |

レビュー担当はrawから中央値を再計算し、全96集合が各10件、全走行の成功とcache条件を確認した。正順64段の目安未達、P2a / P2b合算比較、P2bの小さい絶対差を区別した採用判断も確認した。

[検証一覧](evidence/p2/validation.json)、[CIログ](evidence/p2/workspace-ci.log)、[標準テストログ](evidence/p2/stdlib-tests.log)、[debug CLI記録](evidence/p2/debug-chains.json)、[辺数ログ](evidence/p2/p2a-edge-counts.log)、[ビルド時の製品差分](evidence/p2/after-source.diff)を保存した。

P3 / P4、永続summary、変数表clone削減、process warm / incremental専用の性能harnessは未実施。公開API・言語仕様・checkpoint形式・schema / VM versionは変更していない。
