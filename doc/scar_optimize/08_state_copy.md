# `deferror` scope の変数表複製削減

2026-10-16。[03_compile_time.md](03_compile_time.md) のP3を実施する。
開始時HEADは `ce2fbd3c428fa6ba450344d3302ccad57d20ee43`。意味論・診断・公開API・保存形式を維持するlevel2の内部最適化とする。

## 対象の選定

P1後の `absorb_child_progress` は削除済みで、残る子Checkerは `trait_dependency_for_requirement` の失敗診断用probeだけである。P2後の既存profileで、summaryに出力された `child_spawn` はすべて0だった。成功コンパイルの短縮対象には選ばない。

一方、`with_deferror_body_scope` はすべての本体検査で `env.vars` を複製する。P2後のprofileでは、標準定義174件の `DeferrorDef` が86.662ms、disk hitの64 / 128 / 256宣言が30.662 / 61.567 / 124.704msだった。この全時間を複製費用とは解釈せず、変更前の別binaryで変数表cloneだけを計時してから実装する。

sessionのtransactional APIは失敗時の状態不変を保証する。ここをin-place APIへ置き換えない。P4、推論状態全体の共有、永続cache、process warm / incremental専用の性能harnessも対象外とする。

## 実装と状態契約

`deferror` の変数表だけを全体snapshotから変更差分のundoへ置き換える。通常scopeを越える正規化の伝播を維持するため、通常scopeへ一律に正規化のundoを追加しない。

| 状態 | 変更後の扱い |
|---|---|
| 通常の変数束縛 | 最内scopeと有効な隔離境界に最初の変更前値を記録する。同じframeへ二重記録しない |
| `normalize_env_bindings` による変数表の書換え | 有効な隔離境界だけに変更前値を記録する。通常scope終了後も推論結果を保持する |
| `deferror` 終了 | 成功・失敗とも隔離境界開始時の変数表へ戻す。内側で作成・削除されたbindingも残さない |
| substitutions / bounds / process marker | 既存どおり成功時に保持し、失敗時に復元する |
| obligations / ローカルprovenance | 既存どおり成功・失敗とも復元する |
| capabilities / warnings | 成功時だけ消費と新規warningを反映する。重複させない |
| typevar / tag / function index | 既存の予約・反映・復元規則を維持する |

隔離境界は同期的な `deferror` 本体検査に閉じる。`TypeEnv` 内の隔離frame indexは一時状態とし、serdeの保存対象に加えない。通常scopeのwire layout、公開checkpoint、schema / VM versionを変更しない。通常scopeだけの場合の束縛は従来と同じ定数時間に保つ。

親担当が変更前ソース・binary・測定条件を保存し、サブエージェントが直接テストと実装を担当する。対象は `env.rs`、`checker/patterns.rs`、`checker/definitions/nominal.rs`。正本の [Trait system](../../docs/dev/Trait_system_spec.md)、[Xldr](../../docs/dev/Xldr_spec.md)、[観測仕様](../../docs/dev/Rune_observability.md)、[テスト方針](../../docs/dev/テスト方針.md) の契約は変更しない。

## 検証と採用条件

1. 未変更の大きな変数表を複製しないテストでRedを確認する。通常scopeの正規化伝播、隔離境界内のbindとnormalizeの順序、入れ子scope、cloneしたsiblingのundo分離を直接検証する。
2. 既存の `deferror_scope_tests`、Scar全体、session / trait診断の回帰を確認する。最後に `rtk cargo nextest run --profile ci --workspace` と標準Surtrテストを実行し、別エージェントが最終差分と検証結果をレビューする。
3. 独立したbefore / afterのrelease target・binary・semantic cacheを用いる。source / binary / 入力hash、toolchain、cwd、電源条件を保存する。計測中はビルドとテストを止める。
4. 全24入口を各binary・cold / disk hitごとに10回、交互に測定する。採用目安はError 256宣言のdisk-hit `typecheck` 中央値を10%以上短縮すること。64 / 128 / 256宣言で全表cloneの除去と規模依存の改善を確認する。coldの `stdlib_load` / `compile_total` も別集計する。
5. 代表入力の `compile_total` に5%超の悪化が繰り返す場合は原因を確認する。1ms未満の差や分布が重なる差だけで効果・回帰を断定しない。profiler有効走行は通常測定に混ぜない。
6. 通常debug stackでgeneric chain正逆16 / 32 / 64段も確認する。失敗candidateからの漏れ、診断reason / span / hint、checkpoint復元は既存テストと最終全体検証で維持を確認する。

## 実施記録

### 変更前の局所計時

開始時HEADのarchiveに変数表cloneの計時だけを加え、通常測定とは別のrelease binaryで8走行を確認した。標準定義coldは174回・計176,958要素のcloneで41.378ms、Error 64 / 128 / 256宣言のdisk hitはそれぞれ69,184 / 146,560 / 325,888要素、14.796 / 29.764 / 59.844msだった。各1走行のprofile値であり、安定した速度差や採用比較には使わない。

### 実装

`TypeEnv` は一時的な隔離frameのindexだけを保持する。通常の束縛は有効な隔離frameと最内frame、正規化は有効な隔離frameだけに変更前の値を記録する。既存の `VarScopeFrame` の最初の書込みを記録する仕組みを使い、全表snapshotを削除した。隔離frameがない通常の束縛では祖先frameを走査しない。

隔離indexはin-memory cloneで保持するが、serdeの保存対象には加えない。公開sessionが状態を保存する時点では本体検査が終了し、隔離frameは閉じている。他の推論状態、capability、warningの保存・反映・復元はP1実装を保った。

### 契約テスト

未変更の4,096 binding中の型allocationが本体の成功・失敗後も維持されるテストを先に追加した。変更前はscope終了後にallocationが変わり、意図したassertionで失敗した。変更後は成功した。

通常scopeの正規化伝播、隔離境界内のbind→normalize / normalize→bind、入れ子の隔離境界、inner scope内の一時bindingの削除、cloneしたsiblingのundo分離を確認した。4,096 bindingの表でもundoは変更した2 bindingだけになる。隔離境界を閉じた後のbincode出力と復元も確認した。sessionの既存失敗→rollback→再試行テストには `deferror` 宣言とconstructor呼出しを加え、transactional / in-place双方を検証した。

### 性能と採用判断

Apple M2 / macOS 26.6.2 / Rust 1.94.1、AC電源で測定した。beforeは開始時HEADのclean archive、afterは変更した4 Rustファイルを反映した保存ソースとした。標準ソース130ファイル、Cargo設定、toolchain、build環境の一致と、after保存ソースと最終作業ツリーのRustソースの一致を確認した。

ビルドとテストを止め、全24入口 × before / after × cold / disk hit × 10回、計960回を交互順で実行した。全走行が成功し、期待するcache条件、artifact不使用、typecheck実行、execute省略、profiler無効を確認した。測定前後の入力・標準ソース・binary hashも一致した。OSのpage cache、CPU周波数、他アプリの負荷は固定していない。

単位はms、各10回の中央値。

| 入力 | cache条件 | 指標 | before | after | 短縮 |
|---|---|---|---:|---:|---:|
| Error 64宣言 | disk hit | typecheck | 32.881 | 8.211 | 75.03% |
| Error 128宣言 | disk hit | typecheck | 64.001 | 14.605 | 77.18% |
| Error 256宣言 | disk hit | typecheck | 127.284 | 27.232 | 78.61% |
| Error 256宣言 | disk hit | compile total | 157.462 | 57.195 | 63.68% |
| 小入力 `stdlib_prewarm` | cold | stdlib load | 337.443 | 269.317 | 20.19% |
| 小入力 `stdlib_prewarm` | cold | compile total | 347.142 | 278.675 | 19.72% |

Error 256宣言のtypecheckで10%の採用目安を満たした。三つの規模すべてでbefore / afterの最小最大が分離し、全表cloneの除去と変更したbindingだけを記録するテストも対応するため採用する。変数表cloneの局所計時はclone単独の時間であり、旧コードがscope終了時の変数表置換で行っていた表全体の破棄を含まない。通常測定の短縮量をclone単独の計時と同一視しない。

全24入口のcompile total中央値は、coldで12.87〜36.34%短縮、disk hitで63.68%短縮〜0.25%増加だった。5%超の中央値悪化はない。disk hitの最大増加はnominal chain 16段の0.080msだった。小入力coldのuser typecheckは1.345→1.392msで分布が重なり、coldの改善をuser suffixの短縮とは解釈しない。

[runs.jsonl.gz](evidence/p3/runs.jsonl.gz) に各回の値・stdout / stderr、[summary.json](evidence/p3/summary.json) に中央値・最小最大、[manifest.json](evidence/p3/manifest.json) にsource / binary / 入力hash・toolchain・電源条件・測定順を保存した。生成artifactとcache payloadは一時測定先に残した。

### 別走行の内部profile

全24入口を各binary・cold / disk hitで一回ずつ、計96回確認した。beforeは開始時ソースにcloneの計時だけを加えた別binary、afterは通常測定と同じ保存binaryである。計装の [差分](evidence/p3/before-profile.patch) と [manifest](evidence/p3/before-profile-manifest.json)、[全profile記録](evidence/p3/profiles.json)、最初の [対象選定の計時](evidence/p3/selection.json) を保存した。

標準定義coldの `DeferrorDef` 174件は90.583→18.801ms、disk hitの64 / 128 / 256宣言は31.543→6.237ms、63.200→12.345ms、127.272→24.631msだった。いずれも1走行であり、入れ子の時間を加算しない。beforeのclone計時後の出力は外側の時間へ影響するため、このprofileの差を採用判定の短縮率には使わない。全96走行が成功し、summaryが出力された66走行の `child_spawn` はすべて0だった。残る30走行ではScarの検査時間が5ms未満のためsummaryが省略される。残る診断用probeを成功コンパイルの改善へ含めていない。

### 検証とレビュー

| 検証 | 結果 |
|---|---|
| `deferror_scope_tests` | 7件成功 |
| `env::tests` | 10件成功 |
| 拡張したsessionの失敗・rollbackテスト | 1件成功 |
| 最初の `rtk cargo nextest run -p scar` | 420件成功、2件timeout、終了コード100。Greenとして扱わない |
| 最終 `rtk cargo nextest run --profile ci --workspace` | 2,640件成功、70 binaries、終了コード0 |
| after保存ソース・専用targetの `cargo run --release ... -- test --quiet --all` | 終了コード0。quiet出力に件数はない |
| 通常debug CLI、generic chain正逆16 / 32 / 64段 | 全6入口成功。8176 KiB stack、`RUST_MIN_STACK`未指定 |
| 変更4 Rustファイルの `rustfmt --check`、`git diff --check` | 成功 |

最初のScar全体検証はCI用・releaseのビルドと重なり、`full_standard_compile_and_checkpoint_resume_preserve_user_contracts` と `common_constructor_invocation_suite` が各15秒でtimeoutした。assertionの失敗はなかった。timeout値や除外条件は変えず、同じ最終ソースによる後続のCI profileで、この2件を含む全件成功を確認した。初回の失敗も [生ログ](evidence/p3/scar-full.log) に残す。

独立レビューは最終製品差分と保存ソースの一致、状態の分離、旧経路の削除、保存形式、テストを確認した。性能についてもrawから全96集合・各10件の中央値・最小最大を再計算し、全960走行の成功とcache条件、交互順、採用目安の達成を確認した。未解決の指摘はない。

コマンド・終了コード・未実施範囲は [validation.json](evidence/p3/validation.json)、実装差分は [after-source.diff](evidence/p3/after-source.diff)、全体検証は [CIログ](evidence/p3/workspace-ci.log)、[標準テストログ](evidence/p3/stdlib-tests.log)、[debug CLI記録](evidence/p3/debug-chains.json) を参照。

P4、session transactional cloneの削減、残る診断用child probeの最適化、process warm / incremental専用の性能harnessは未実施。公開API・言語仕様・checkpoint形式・schema / VM versionは変更していない。
