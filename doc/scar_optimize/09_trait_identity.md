# canonical Trait proof の identity 検索

2026-10-16。[03_compile_time.md](03_compile_time.md) のP4を実施する。
開始時HEADは `dcbac571b8a0a081fe54f7bc027a45c6c97b5bd6`。意味論・診断・公開API・保存形式を維持するlevel2の内部最適化とする。

## 対象と手順

`prove_canonical_constraints` と `prove_canonical_capability` に残るUIDからTrait keyへの線形検索を対象にする。impl候補のindexとは独立した検索である。proof結果のcache、推論やcapability消費の変更、sessionのtransactional clone、process warm / incremental専用の性能harnessは対象外とする。

1. 開始時ソースをarchiveし、専用release targetでbefore binaryを保存する。別の計装binaryで二つの検索の回数・比較要素数・局所時間を確認する。
2. Checker内の初回検索時にUID→keyのindexを構築し、以後の検索で共有する。trait登録で無効化し、新しいCheckerでは正本から再構築する。追加indexの構築費用と保持期間を明示する。
3. サブエージェントがScar内の契約テストでRedを確認してから実装する。別担当がtrait数とobligation数を独立に変える規模別入力を生成する。
4. Scarの対象検証後、CI profileのworkspace全体と標準Surtrテストを実行する。別エージェントが最終差分と測定根拠をレビューする。

対応する正本は [Trait system](../../docs/dev/Trait_system_spec.md)、[Xldr](../../docs/dev/Xldr_spec.md)、[テスト方針](../../docs/dev/テスト方針.md)、[観測仕様](../../docs/dev/Rune_observability.md)。UIDは表示名や型引数から復元しない。parameterized obligation、generic/specific implの選択、曖昧性・循環の拒否、constructor carrier、capability消費、診断位置の契約を維持する。これらの仕様変更は行わない。

## 状態と検証の契約

| 境界 | 扱い |
|---|---|
| 初回canonical lookup | 正本のTrait定義からindexを構築する。検索をしないCheckerでは構築しない |
| 同じCheckerでの反復 | indexを共有し、全Traitの反復走査をしない |
| Trait登録 | 以前のindexを無効化し、次回検索で新しい正本を使う |
| checkpoint復元・rollback・sibling session | 保存した正本だけから派生させる。追加したTraitが別sessionや復元前の状態へ漏れない |
| proofの結果 | 毎回現在の推論状態・impl・active capabilityを使う。Deferredや失敗をcacheしない |
| 保存形式 | indexをserializeしない。schema / VM versionを上げない |

直接テストは反復lookup、登録後の再構築、未知UIDの拒否、checkpoint復元とsession分離を中心にする。意味論の成功・拒否境界は既存のtrait / constructor / session / 診断テストも利用する。

## 性能比較と採用条件

既存24入口に、trait数を五規模、obligation数を四規模で変える入力を加える。ほかの軸は系列内で固定する。入力生成の再現性と全入口のbuild成功を確認する。

before / afterでソース・binary・標準ソース・入力のhash、toolchain、build条件、cwd、電源条件を保存する。targetとsemantic cacheを分離し、各条件10回以上、before / afterの順を交互にする。ビルド・テストを停止して測る。cold / disk hitの中央値・最小最大を別々に記録し、cache条件、typecheck実行、execute省略、profiler無効、終了コード0を検証する。

最大trait規模でindex構築後の検索を定数回のmap lookupへ減らすこと、局所lookup時間とdisk-hit typecheckの改善が観測されることを採用判断に使う。目安は局所検索の比較要素数を90%以上削減すること。小入力の初回構築とcoldのstdlib load / compile totalも比較し、代表入口で5%超のcompile total悪化が繰り返す場合は原因を確認する。1ms未満の差や分布が重なる差だけで効果・回帰を断定しない。局所計装は通常の時間比較に混ぜない。

実測値・実行結果・採用判断は以下へ追記する。

## 実施記録

### 実装と契約テスト

Checkerに `OnceLock<HashMap<u32, String>>` を置き、最初のcanonical lookupでUID→keyの対応だけを作る。二つのproof入口で共有し、旧来の `traits.iter().find(...)` は削除した。TraitInfoは毎回正本から読むため、登録後のconstructor slot更新も古いmetadataへ固定しない。trait登録の唯一の製品経路でindexを無効化する。

indexはPersistentCheckerState / ScarCheckpointに持ち込まない。checkpoint復元・rollback・子Checkerは新しいCheckerで空のindexから始める。追加impl・推論変数・active capabilityは引き続きproof実行時の状態で扱う。missing UIDの内部エラーとspanも維持した。

反復検索の直接テストは、変更前の23 traitに対して624要素を再走査し、意図したassertionで失敗した。変更後は成功した。追加3テストで反復走査の抑制、trait登録後の再構築、serialized checkpoint復元、rollback、sibling分離、Deferred / Satisfied / Unsatisfied、missing UIDの診断を確認した。`rtk cargo nextest run -p scar canonical_` は29件成功・396件除外、終了コード0だった。構文・型の誤りによる初回テストcompile失敗はRedの根拠に含めていない。

### 入力と局所profile

trait数64 / 256 / 1024、dispatch数64 / 256 / 1024の2系列を追加した。最初の局所profileで検索費用が1ms未満だったため、両系列へ4096の第四規模を追加した。trait数系列はdispatch256箇所、dispatch数系列はuser trait64個で固定し、両系列ともnominal型1個・impl2個・method本体2個を保つ。dispatch数は内部proof呼出し数とは区別する。生成物は23ファイル、測定入口は既存24件と追加8件の計32件となった。

before / afterの計装binaryで、全32入口×cold / disk hitの計128走行を確認した。全走行が成功し、cache / phase条件とuser suffixのlookup回数が一致した。標準定義の先頭レポートとuser / module suffixを分離した。計装は製品差分へ加えず、[before差分](evidence/p4/before-profile.patch)・[after差分](evidence/p4/after-profile.patch)と各manifestを保存した。

最大trait規模のdisk hitでは、512 lookupが線形に訪問した786,944要素を、初回index構築の4,118要素へ減らした。線形走査・構築に触れる定義数は99.48%減であり、HashMap内部のキー比較回数は含まない。lookup時間は1.103→0.207ms、初回構築0.151msはafterのlookup時間に含まれる。これらの時間を加算しない。

一方、trait64のlookupは0.012→0.057ms、trait64・dispatch4096では0.593→0.910msだった。小さい集合では2段のmap lookupと初回構築の費用が増える場合がある。代表24入口の多くではuser lookupは0だった。lookupがないCheckerではindexを作らない。

これは各条件1回のprofiler有効走行で、一部はrelease buildと並行した。HashMapの反復位置もprocessごとに変わる。回数・処理量の確認と費用の帰属に使い、安定した短縮率や回帰の採用判断には使わない。[全原文](evidence/p4/profiles.json.gz)、[局所集計](evidence/p4/profile-attribution-summary.json)、[入力の検証](evidence/p4/input-validation.json)を参照。

### 最初の32入口の未計装binaryによる反復比較

Apple M2 / macOS 26.6.2 / Rust 1.94.1、AC電源で実行した。beforeは開始時HEADのclean archive、afterは最終変更の5 Rustファイルを反映した保存ソースから、それぞれ専用release targetでビルドした。標準ソース130ファイル、Cargo設定、toolchainの一致と、after保存ソース・最終作業ツリーのRustソースの一致を確認した。indexの追加は4製品ファイル、契約テストは1ファイルに閉じる。

ビルドとテストを止め、全32入口×before / after×cold / disk hit×10回、計1,280回を交互順で実行した。全走行が成功し、期待するcache条件、artifact不使用、typecheck実行、execute省略、profiler無効を確認した。測定前後の入力・標準ソース・binary hashは一致した。OSのpage cache、CPU周波数、他アプリの負荷は固定していない。

単位はms、各10回の中央値。増減はafterのbeforeに対する変化率で、負が短縮を表す。

| 入力 | cache | 指標 | before | after | 増減 |
|---|---|---|---:|---:|---:|
| trait 64・dispatch 256 | disk hit | typecheck | 17.967 | 17.887 | -0.45% |
| trait 256・dispatch 256 | disk hit | typecheck | 18.751 | 19.071 | +1.70% |
| trait 1024・dispatch 256 | disk hit | typecheck | 21.605 | 21.398 | -0.96% |
| trait 4096・dispatch 256 | disk hit | typecheck | 36.635 | 34.669 | -5.37% |
| trait 4096・dispatch 256 | disk hit | compile total | 141.541 | 141.063 | -0.34% |
| trait 64・dispatch 64 | disk hit | typecheck | 6.503 | 6.706 | +3.12% |
| trait 64・dispatch 1024 | disk hit | typecheck | 78.988 | 78.151 | -1.06% |
| trait 64・dispatch 4096 | disk hit | typecheck | 480.686 | 477.625 | -0.64% |
| 小入力 `stdlib_prewarm` | cold | stdlib load | 271.805 | 269.062 | -1.01% |
| 小入力 `stdlib_prewarm` | cold | compile total | 282.071 | 278.404 | -1.30% |
| 小入力 `stdlib_prewarm` | disk hit | compile total | 26.119 | 26.377 | +0.99% |

最大trait規模のtypecheckはbefore 32.344〜42.855ms、after 32.968〜38.176msで分布が重なった。交互に対応する10組のうち7組がafterで短かったが、安定した5.37%短縮が確立したとは扱わない。dispatch数4096も分布が重なり、10組中5組だけがafterで短かった。

全32入口のcompile total中央値の変化は、coldで-3.14〜+3.08%、disk hitで-1.00〜+2.56%だった。5%を超える中央値悪化はなかった。trait数を64から4096へ増やしても、afterは各Checkerで一度だけ表を走査することを直接テストとprofileの処理量で確認できた。一方、今回のCLI比較では、コンパイル全体の明確な短縮は確認できなかった。

定義走査件数90%削減の目安は99.48%で満たしたが、この時点では時間による採用条件を満たしたと判断できなかった。独立レビューでもこの点を指摘されたため、採用を保留し、製品コードを変えずにtrait数だけをさらに増やして確認した。

[全走行値・stdout / stderr](evidence/p4/runs.jsonl.gz)、[中央値・最小最大](evidence/p4/summary.json)、[source / binary / 入力hashと測定条件](evidence/p4/manifest.json)を保存した。生成artifactとcache payloadは一時測定先に残した。

### trait数16384への拡大と採用判断

trait数系列へ16384の第五規模を追加した。dispatch256箇所、nominal型1個・impl2個・method本体2個は既存系列と同じ。生成物は計24ファイル、測定入口は計33件となった。最初の32入力・製品コード・binaryは変更せず、新規入力だけを二つの独立した10反復batchで比較した。各batchでbefore / afterを交互順にし、cold / disk hitと専用cacheを分離した。追加80走行もすべて成功し、cache / phase条件と測定前後のhashが一致した。最初の1,280走行と合わせて計1,360走行である。各入力・batchの対応は [comparison-index.json](evidence/p4/comparison-index.json) に示す。

各20回の中央値、単位はms。

| cache | 指標 | before | after | 増減 |
|---|---|---:|---:|---:|
| cold | typecheck | 110.840 | 100.384 | -9.43% |
| disk hit | typecheck | 109.820 | 97.131 | -11.55% |
| cold | compile total | 669.843 | 663.133 | -1.00% |
| disk hit | compile total | 408.773 | 400.444 | -2.04% |

disk hitのtypecheckは20組中16組がafterで短く、対応するafter-beforeの差の中央値は-11.108msだった。対応組を再標本化した中央値の95% percentile bootstrap区間は-16.195〜-2.286msで0を含まず、方向のexact sign testは片側p=0.00591だった。bootstrapはseed `20261016`、100,000回の再標本化とした。単純な最小最大の範囲は引き続き重なるが、対応組の差と局所処理量も合わせ、対象フェーズの短縮がばらつきだけでは説明しにくいという採用条件を支持する。これはこの入力・測定条件での観測であり、任意の入力や機械へ11.55%を保証するものではない。

追加規模の別profileはbefore / after・cold / disk hit各1回、計4走行で確認した。disk hitの512 lookupは線形に4,147,712要素を訪問し、afterは初回index構築16,406要素だけとなった。定義走査件数は99.60%減、lookupは8.659→0.817msで、afterに構築0.750msを含む。この4走行ではビルド・テストを停止した。先の128走行と合わせ、profileは計132走行となった。

意味論・保存形式を保ち、反復検索の規模依存と大きなtrait集合のtypecheck費用を減らす変更として採用する。最初の32入口で5%を超えるcompile total中央値の悪化はなく、小集合のlookup準備費用はその比較にも含まれる。一方、追加規模のcompile totalのpaired差の95%区間はcold / disk hitとも0を含んだ。コンパイル全体の明確な短縮は確認できておらず、typecheckの短縮と区別する。

[追加80走行の値・stdout / stderr](evidence/p4/extended-runs.jsonl.gz)、[追加規模20回の集計](evidence/p4/extended-combined-summary.json)、[対応組の差と解析条件](evidence/p4/extended-paired-analysis.json)、[最初の10反復の条件](evidence/p4/extended-manifest.json)、[確認用10反復の条件](evidence/p4/confirm-manifest.json) を保存した。全33入口の集計は [summary.json](evidence/p4/summary.json) にまとめた。

### 全体検証と未実施範囲

| 検証 | 結果 |
|---|---|
| `rtk cargo nextest run -p scar canonical_` | 29件成功、396件除外、終了コード0 |
| `rtk cargo nextest run --profile ci --workspace` | 2,643件成功、70 binaries、終了コード0 |
| after保存ソース・専用targetの `cargo run --release ... -- test --quiet --all` | 終了コード0。quiet出力に件数はない |
| 通常debug CLI、generic chain正逆16 / 32 / 64段 | 全6入口成功。8176 KiB stack、`RUST_MIN_STACK`未指定 |
| `generate.py --check` | 24生成入力が一致、全33入口に整合 |
| 変更Rustファイルの `rustfmt --check`、`git diff --check` | 成功 |

検証コマンド・終了コードは [validation.json](evidence/p4/validation.json)、全体検証は [CIログ](evidence/p4/workspace-ci.log)、[標準テストログ](evidence/p4/stdlib-tests.log)、[debug CLI記録](evidence/p4/debug-chains.json)を参照。実装差分は [after-source.diff](evidence/p4/after-source.diff)に保存した。

process warm / incremental専用の性能harnessは未実施。checkpoint・rollbackの正しさは契約テストで確認したが、その性能は今回のcold / disk hit比較へ含めていない。公開API・言語仕様・checkpoint形式・schema / VM versionは変更していない。

独立レビューでは、製品差分の状態分離・旧探索の削除・保存形式を確認した。最初の32入口だけでは時間による採用条件が未確定という指摘を受け、16384 traitの追加20反復で再評価した。最終レビューは1,360走行と全phase集計、対応組の解析、profile132走行、source / binary / 入力hashを独立に照合し、この指摘の解決と修正必須の残件がないことを確認した。[review.json](evidence/p4/review.json)に結果を保存する。
