# CI テスト時間の増大と改善

## 目的と作業方針

2026-10-07、入力仕様ファイルなしで改善まで進める依頼に基づき実施する。
言語仕様と検査条件を変えない level 2 の改善とする。
製品テストと回帰テストの入力・期待値を維持し、標準環境の重複準備、
子チェッカーの不要なコピー、同じ契約に対する CLI の反復起動を減らす。
旧経路、曖昧なフォールバック、テスト除外、タイムアウト延長は追加しない。

計測コマンドは次のとおり。修正中は対象を絞った hot テストを使い、最終差分で同じ clean コマンドを実行する。

```sh
cargo clean && cargo build && cargo nextest run --all --profile ci
```

親エージェントがビルドとテストを直列実行する。Scarとテスト実行部の調査・実装を
サブエージェントへ分担し、新しい問題と最終差分を独立コンテキストのAstraが確認する。
コミット、マージ、push は依頼範囲に含めない。

## 依頼時の履歴値

| revision | nextest件数 | Summary |
|---|---:|---:|
| main | 2402 | 160.589s |
| `26ba910fb8e070ec830b1dfe2d5a1c45da670021` | 2300 | 139.152s |
| `c2ddc8aad7467732ee080fc831880c6e266a2b08` | 2257 | 82.600s |
| `f5614922a7c9146ea184e8b7fa705f61d49d059f` | 2034 | 66.172s |

これは依頼時の提供値であり、今回過去revisionを再計測してはいない。
2257件から2300件への増加は約1.9%だが、時間は約68.5%増えている。
ケース数だけで説明せず、標準定義の検査・復元を繰り返す固定費とCLI境界の巻き込みを調べた。
bucket化後はnextest件数が実行単位を表すため、件数だけから改善を判断しない。

## 基準計測

対象は `ab374367`、macOS arm64 の debug build、環境の cache 設定は変更していない。
この worktree の target を指定コマンドで clean した。
貼付された main の 160.589 秒とは実行環境や負荷が異なり得るため、変更前後は同じ worktree の実測で比較する。

| 区間 | 変更前 |
|---|---:|
| `cargo build` | 27.34s |
| nextest の test build | 37.61s |
| setup | 21.500s |
| nextest Summary（setup を含む） | 149.560s |
| コマンド全体 real | 252.54s |
| コマンド全体 user / sys | 1055.74s / 54.07s |
| nextest 件数 | 2402 passed / 0 skipped |

build と Summary の単純和はコマンド全体時間にならない。
Cargo の起動・テスト一覧取得など、各区間の外側の時間も含まれる。
setup は Scar example の追加ビルドを含み、先行 `cargo build` は workspace の default member である Rune をビルドする。

各テストプロセスの実行時間を足した値は並列実行を含む累積時間であり、CPU 時間や wall time とは異なる。

| 対象 | nextest 件数 | 累積実行時間 |
|---|---:|---:|
| Rune integration | 165 | 231.335s |
| analysis service | 39 | 138.129s |
| Xldr REPL core | 12 | 66.057s |
| LSP adapter | 19 | 63.439s |
| Scar surface | 9 | 56.114s |
| Forge | 99 | 47.161s |
| Scar type constructor carriers | 42 | 32.106s |
| Scar default trait methods | 26 | 22.597s |
| Scar return type arguments | 41 | 20.650s |

全テストの累積実行時間は877.172秒。

## 改善する契約

### Scar の局所改善

- 子チェッカーへ渡す env をそのまま使い、直後に破棄する親 env の clone を削除する。
- persistent state から移動済みの5フィールドを再 clone しない。
- 特殊化処理の呼出しごとに trait impl method の宣言 UID 集合を一度作り、
  各 definition の所属判定で同じ集合を参照する。
- 型変数、pending call、Pattern 要件の全検査と宣言 identity による判断は維持する。

### 標準環境を使うテストの実行単位

Scarの`type_constructor_carriers`42件、`return_type_arguments`41件、
`default_trait_methods`の通常22件は、各targetの8 bucketでprocess内のprefixを共有する。
標準を置き換える4件は個別実行を維持する。元109ケースからnextestの実行単位31件へ集約する。
共通registryは名前・関数の重複、指定された個別テスト属性の存在と個数、空bucketを検査し、
`dead_code` denyで属性なし・未登録のケース関数を拒否する。全109ケース本体が変更前と同一であることも照合した。

analysis service と LSP adapter は、プロセス内の `OnceLock` に標準環境を保持する。
個別nextestプロセスでは共有できないため、同じ標準環境だけで検査するケースをregistryとbucketへ集約する。
analysisの13ケースを4 bucket、LSPの6ケースを2 bucketにする。Projectと標準編集のケースは個別実行を保つ。
各ケースは従来どおり新しい service / host を作る。ケース名・関数の重複と、属性を持たないケース関数の登録漏れを検査し、
失敗時には入力ケースを特定できるようにする。
実行ケースを削減したように見えないよう、registry 件数と nextest 件数を分けて記録する。

### Test の拒否と診断

- 型・構文拒否27入力は Rune の製品 compile 入口を直接使い、同じ TestEnabled 標準環境、
  Script policy、前置宣言、診断段階と本文を検証する。各入力の compiler state は独立にする。
- JSON の script_errors と終了コードを検証する代表 CLI ケースを残す。
- assertion caption の63入力は公開関数名、実行位置、後続ケースの成功を維持する。
  CLI の出力契約を保ちながら同じ準備の反復起動を減らす。

## 受入条件と検証計画

- 改善対象のケースと重要な成功・拒否境界をすべて実行する。
- Scar の trait method instantiation / default trait methods / Lazy / constructor / surface を確認する。
- tooling の registry inventory、service / adapter の全ケースを確認する。
- Test の27拒否入力と caption の63入力、および代表 CLI を確認する。
- 最終差分の独立レビューを受け、指摘を解決する。
- 最終 clean コマンドが全件成功し、同じ基準計測より Summary と累積時間が減る。
- baseline と変更後のログ、build・setup・Summary・総時間、件数の変化を別々に記録する。

## Astra の助言と保留した案

子チェッカーの二重 clone 削除、呼出し内の UID 索引化、ケースを維持する bucket 化と
compile-time 拒否ケースの移管は、意味論を保つ局所改善として採用する。

specialization metadata の suffix 間の無条件キャッシュは採用しない。
型変数の bound と `resolve_ty` に依存するため、AST が同じという条件だけでは再利用できない。
特殊化が必要かを判断する検査条件も緩めない。

Project fixture prefix を Script の標準 snapshot に統合する案は、今回は保留する。
既存 helper には追加 module を標準権限で検査する差があり、Lazy 宣言の扱い、ExitCode の診断、
prefix と suffix をまたぐ ownerless module 衝突の検査を先に固定する必要がある。
広い policy 変更を速度改善に混ぜず、必要な成功・拒否境界を別途用意する。

## 実装・検証ログ

- Scarの二重コピー削除と宣言UID索引化を実装した。既存の判定条件と永続状態を維持した。
- Runeの27拒否入力を製品コンパイル入口の単体テストへ移した。全入力の期待フェーズを固定し、入力位置も検査する。代表CLIのJSONと終了コードは保持した。
- 公開アサーションの21入力×3形式をCLI2回へまとめた。63入力と後続成功63件をすべて実行し、各FAIL/PASSの所属と順序・失敗位置・公開名・全件集計を検査する。
- Astraのレビューで後続成功ケースの文言不在検査がFAIL区間だけへ狭まった点を指摘された。元の全stdoutへの否定検査も保持して修正した。
- toolingの最初の集約試行はservice29ケースを8 bucket、adapter12ケースを4 bucketへまとめたが、serviceの3 bucketが15秒でタイムアウトした。追加宣言に合わせた標準prefixの再検査が各Projectケースに残るため、プロセスを共有しても準備を十分に省けなかった。Astraと相談し、Projectと標準編集のケースを個別実行へ戻した。タイムアウト設定は変えていない。
- 修正後のserviceは元39ケースを13登録＋26個別、adapterは元19ケースを6登録＋13個別として保持する。bucketとinventoryを含むnextest件数は合計58から47になる。元の58ケース本体が変更されていないことも照合した。

### 局所検証

| 実行 | 結果 |
|---|---|
| 変更前tooling（service / adapter、default） | 58 passed、29.164s、1 leaky、0 skipped、exit 0 |
| 最初のtooling集約試行 | 28 passed、3 timeout、22.750s、exit 100。採用しない |
| 修正後tooling（同じtargetとprofile） | 47 passed、19.581s、0 timeout / 0 skipped、exit 0 |
| Runeの27拒否入力のunit | 1 passed、2.649s、88は選択対象外、exit 0 |
| Rune `test_command::` 全体（default＋`--ignore-default-filter`） | 39 passed、8.021s、126は選択対象外、exit 0 |
| Scar全体（default） | 329 passed、32.130s、0 skipped、exit 0 |

修正後toolingは局所wall timeが9.583秒、約32.9%短縮した。最初の失敗試行と比較して効果を算出しない。
基準のworkspace cleanでcaptionの2テストは10.367秒・12.555秒、型拒否CLIは7.319秒だった。
局所のhot検証ではそれぞれ0.568秒・0.504秒・0.266秒で、移した27入力のunitは2.646秒だった。
workspace並列と局所hotでは負荷が違うため、全体の短縮率は最終cleanの同条件比較で判定する。

### 最終clean計測

基準と同じworktree、同じコマンド、同じCI profileで実行した。変更後の終了コードは0。

| 区間 | 変更前 | 変更後 |
|---|---:|---:|
| `cargo build` | 27.34s | 23.40s |
| nextestのtest build | 37.61s | 32.97s |
| setup | 21.500s | 20.315s |
| nextest Summary（setupを含む） | 149.560s | 125.475s |
| コマンド全体real | 252.54s | 220.60s |
| コマンド全体user / sys | 1055.74s / 54.07s | 914.20s / 47.37s |
| nextest件数 | 2402 passed / 0 skipped | 2314 passed / 0 skipped |
| テストプロセスの累積実行時間 | 877.172s | 728.306s |

Summaryは24.085秒、約16.1%短縮。コマンド全体は31.94秒、約12.6%短縮。
累積実行時間は148.866秒、約17.0%短縮した。各1回のclean測定であり、実行負荷の揺らぎを含む。
変更しなかった対象にも小さい時間差があるため、すべての差を特定のコード変更の効果とは扱わない。

| 対象 | 変更前の累積実行時間 | 変更後の累積実行時間 |
|---|---:|---:|
| Rune integration | 231.335s | 194.941s |
| analysis service | 138.129s | 91.718s |
| LSP adapter | 63.439s | 40.550s |
| Scar type constructor carriers | 32.106s | 17.312s |
| Scar return type arguments | 20.650s | 6.153s |
| Scar default trait methods | 22.597s | 15.921s |
| Scar surface | 56.114s | 53.020s |
| Forge | 47.161s | 44.987s |
| Xldr REPL core | 66.057s | 62.002s |

件数差は、Scar109→31で78件減、tooling58→47で11件減、Runeの拒否入力unitが1件増、
合計88件減による。元のケース・入力を削除したものではない。
Testの対象3テストは計90回のCLI起動から3回に減らした。27拒否入力は製品compile入口で別途検査する。

Astraの最終実装レビューでは必須のコード指摘がなかった。変更したRustファイルの
`rustfmt --check`と`git diff --check`は成功した。言語意味論、特殊化の条件、
strictな準備状態復元、schema / VM版、タイムアウト設定は変更していない。

### 残る改善候補と未検証範囲

- Projectと標準編集のanalysisは、全宣言indexへ合わせた標準前置部の再検査を行う。今回の集約対象から外し、現行の成功・拒否とsource identityを維持した。IDの割当てを含む変更は別の検討が必要。
- Scarのsuffix特殊化は永続definition全体のcloneと走査を続ける。現在のbound / substitutionに依存する検査を省かず、共有するなら変更時の分離とcheckpoint・span・関数indexの回帰を先に固定する必要がある。
- Forgeの標準環境構築、XldrのREPL core、各ケースの標準AST解析・prefix復元は残っている。今回の局所コード改善だけでこれらの費用が解消したとは主張しない。
- 過去revisionの再計測やfirst-badの特定、将来のstdlib増加を使った負荷測定は実施していない。

### 計測ログ

生ログは`/tmp/surtr-test-perf-d975/`へ保存した。上の表はこれらから集計した。

- `baseline-clean.log` / `final-clean.log`: 指定cleanコマンドと`/usr/bin/time -p`。
- `tooling-before.log` / `tooling-after-focused.log`: 同じtarget / default profileの対照測定。
- `tooling-after.log`: 採用しなかったtimeout試行。
- `scar-profile-before.log`: profilerを使った95件の変更前観測。`--no-capture`により逐次実行となるため、並列CIとのwall time比較には使わない。
- `scar-after.log`: Scar全体のhot検証。
- `test-types-after.log` / `test-command-after.log`: 型拒否unitとCLI全体のhot検証。
- `comparison.json`: clean2実行の各テスト時間と対象別集計。
- `standard-srt.log`: 標準Surtrテストの検証結果。

`cargo run -- test --quiet --all`は終了コード0で成功した。File / FS / Shellの既存テストが
使う`tmp/sandbox`を用意して実行した。quiet実行のためケース件数はログから報告しない。
最終実装後の失敗・タイムアウト・未実行の必須検証は残っていない。
コミット・マージ・pushは行っていない。

## 第2回の改善

ユーザーの継続指示に従い、入力ファイルなしで内部の所有表現とテスト準備を改善する。
ScarとForgeの調査・実装をサブエージェントに分担し、Astraには独立コンテキストで
共有値の変更分離、checkpoint、serialize、意味論の境界を確認してもらった。

### 方針と変更前測定

Scarの永続definitionとtrait実装本体を`Arc`で共有し、変更する値だけをコピーする。
特殊化中の一時mapでも同じ共有値を用いるが、定義の走査と特殊化の判定条件は維持する。
trait義務の更新は、method名と宣言UIDが一致する実装を選んでからその値だけを変更する。
serializerは共有値の中身を直接出力するため、schemaとVM版は変更しない。
保持checkpointと兄弟sessionへの変更漏れを製品の更新経路で検査する。

Forgeは、標準prefixを使う4つの個別ケースを既存4 bucketへ登録する。
元の入力とassertionを保持し、各processで繰り返していた標準準備を減らす。

Projectの標準prefix再検査は維持する。追加宣言によって標準関数内のlocal UIDの開始位置も
変わるため、suffixのUID下限だけを動かしても既存localと追加宣言の衝突を防げない。
既存の32追加関数を使うanalysis回帰もこの境界を検査している。
標準ASTのみの共有は可能性があるが、semantic環境の取得で余分な型検査が発生しない
準備の分割と費用の確認が必要なため、今回の対象から外す。

変更前に`cargo nextest run -p forge -p scar -E 'package(forge) | binary(typecheck_surface)'`
を実行した。108 passed、0 skipped、Summary 13.364s、exit 0。
この局所hot測定と指定clean測定を、それぞれ同条件の変更後実行と比較する。

### 実装確認

関数indexの同期処理には、実際の置換がなくても共有ASTを複製する経路があった。
Astraに再帰helperまで確認してもらい、mappingを従来どおり構築した後でidentity置換を除き、
実効的な置換がない場合だけASTの書換えを省く。mappingの上書き順とrekeyのremove / insert順は
維持し、特殊化cacheのmaterialized検証と次のindexの更新は常に実行する。

初回の変更後buildは、型構築子の候補結果`ConstructorProjectionOutcome::Applicable`に
所有型が残っていたため型不一致で失敗した。Astraへ独立相談し、この候補情報も共有型に
統一する。呼出し側が所有して返すslot列とtarget型だけをコピーし、impl全体のコピーへ戻さない。
失敗した試行は成功測定として扱わない。

### 局所検証

| 実行 | 結果 |
|---|---|
| Forge＋Scar surface、変更前hot | 108 passed、0 skipped、13.364s、exit 0 |
| Forge＋Scar surface、変更後hot | 104 passed、0 skipped、9.134s、exit 0 |
| Scar＋Xldr、default profile | 421 passed、84 skipped、35.437s、exit 0 |
| `cargo fmt --all -- --check` / `git diff --check` | 成功 |

局所Summaryは4.230秒、約31.7%短縮した。累積実行時間はForgeが45.104秒から13.177秒、
Scar surfaceが52.911秒から48.421秒へ変わった。全体並列の短縮率はclean計測で判断する。
nextestの4件減はForgeの個別テストをbucketへ登録した差で、元の19ケースは保持した。

追加した3回帰はすべて成功した。

- `retained_definitions_isolate_reconciled_sessions_and_checkpoints`: identity同期で共有を保ち、実際の関数index変更が保持checkpointと別sessionへ漏れない。rollbackで元の状態へ戻る。
- `retained_definition_sharing_preserves_wire_and_checkpoint_restore`: 共有値と中身のserialize結果が一致し、checkpoint往復後も独立にindexを変更できる。
- `checked_method_obligations_detach_only_the_child_implementation`: 製品のtrait本体検査で義務を更新し、親・兄弟・checkpointは取り込み前に変わらず、取り込み後に親へ反映される。未変更implの共有とtrait値のwireも検査する。

defaultで除外された84件は、続くCI profileで実行した。
Astraの修正後の独立レビューでは追加の指摘がなかった。

### 指定clean計測

同じworktreeで`cargo clean && cargo build && cargo nextest run --all --profile ci`を直列に実行した。
終了コード0、2,313 passed、0 skipped。

| 区間 | 第1回後 | 第2回後 |
|---|---:|---:|
| `cargo build` | 23.40s | 23.31s |
| nextestのtest build | 32.97s | 34.33s |
| setup | 20.315s | 20.727s |
| nextest Summary | 125.475s | 110.869s |
| コマンド全体real | 220.60s | 207.76s |
| コマンド全体user / sys | 914.20s / 47.37s | 852.76s / 54.34s |
| nextest件数 | 2314 passed / 0 skipped | 2313 passed / 0 skipped |
| テストプロセスの累積実行時間 | 728.306s | 611.329s |

前回からSummaryは14.606秒、約11.6%短縮した。全体realは12.84秒、約5.8%、
累積実行時間は116.977秒、約16.1%短縮した。setupには改善が見られず、test buildとsysは増えた。
各1回の対照測定なので、負荷の揺らぎとコード変更の効果を完全には分離していない。

作業開始時の同worktree基準と比較すると、Summaryは149.560秒から110.869秒へ約25.9%、
全体realは252.54秒から207.76秒へ約17.7%短縮した。

| 対象 | 第1回後の累積時間 | 第2回後の累積時間 |
|---|---:|---:|
| Forge | 44.987s | 13.579s |
| Rune integration | 194.941s | 168.720s |
| Scar surface | 53.020s | 40.345s |
| Scar type constructor carriers | 17.312s | 10.526s |
| analysis service | 91.718s | 79.638s |
| LSP adapter | 40.550s | 31.284s |
| Xldr REPL core | 62.002s | 53.715s |

Forgeのbucket登録でnextest件数は4件減り、共有状態の回帰3件が増えたため、差引1件減となる。
既存ケースの入力とassertionは削除していない。検査・特殊化の条件、source identity、
strictなcache復元、schema / VM版、タイムアウト設定は維持した。

### 残件とログ

- Scarの特殊化には、永続definition全体の走査とmetadataの再計算が残る。今回省いたのは不変値のdeep cloneで、走査を省くcacheは導入していない。
- env・signature等のmapコピー、processごとのAST parseとprefix deserialize、初回の標準準備は残る。setup時間を短縮したとは扱わない。
- Projectの準備共有を広げる変更と、過去revisionの再計測は行っていない。

第2回の生ログは`/tmp/surtr-test-perf-d975/round2/`に保存した。
`focused-before.log` / `focused-after.log`は局所対照、`scar-xldr-after.log`は回帰、
`final-clean.log`は指定計測、`comparison.json`はclean前後の各テスト時間と集計を記録する。
`fmt-check.log`と日本語lintの結果も同じディレクトリに保存した。

`cargo run -- test --quiet --all`も終了コード0で成功した。実行ログは`standard-srt.log`。
最終差分の`git diff --check`は成功し、必要な検証の失敗や未実行は残っていない。
コミット・マージ・pushは行っていない。

## 第3回の改善

継続指示に従い、意味論を変えないRust内部とテスト準備の改善をlevel 2として進める。
Scarとanalysisの調査・実装をサブエージェントへ分担し、新たなAstra顧問には
独立コンテキストで変更境界と再発検査を確認してもらう。前2回の差分は保持する。

### 方針と変更前測定

- Scarのtrait宣言とcanonical callable signatureを共有値にする。traitの継承slotは全件の計算後に更新し、値が変わる場合だけ分離する。呼出しごとのfresh mappingと具象化したsignatureは独立した所有値を保つ。継承計算中の不要snapshotと登録時の不要cloneも除く。
- analysisの標準構文とsemantic環境を同じ所有root内で分け、semantic環境だけ遅延構築する。Projectは構文だけを再利用し、全宣言indexに合わせた標準resolve/typecheckは維持する。標準編集中は共有を使わず、現在documentを解析する。
- source文字数上限のCLI回帰は、成功上限の入力を単一の長いコメントにする。従来の約25万個の改行tokenの解析は、このsource文字数契約には不要だった。文字数上限直前、マルチバイト文字、実行結果、超過main / includeの拒否とファイル識別を維持する。超過入力とassertionは変更しない。

`cargo nextest run -p scar -p surtr-analysis -p surtr-lsp -E 'binary(typecheck_surface) | binary(service) | binary(adapter)'`
の変更前hot測定は56 passed、0 skipped、21.838s、exit 0。
既存のintegration binaryでsource上限回帰だけを直接実行した変更前測定は1 passed、
libtest 3.47s、real 3.48s、exit 0。164件は選択対象外。
孤立した空の`SURTR_STDLIB_CACHE_DIR`で標準checkを観測すると、real 1.56s、
Scar型検査724.219ms、そのうちchild spawn 50回63.916msだった。
このprofiler測定はCLIの単独cold semantic準備であり、並列CIのwall timeと混同しない。

### 実装確認と局所検証

変更後の初回対照は56 passed、0 skipped、22.668s。新しいprefix cacheの構築を含むため、
温まった条件でも実行し、56 passed、0 skipped、20.462sとなった。変更前21.838sとの差は
1.376秒、約6.3%。初回の増加も記録し、都合のよい試行だけで大幅な短縮を主張しない。

追加したgeneric signature回帰は、最初の宣言で引数の`$A`をRTAでも重複導入し、
現行の`DuplicateReturnTypeArgumentInput`で失敗した。製品側の検査は正しい。
Astraへ相談し、通常genericのInt / String推論呼出しと、Boolean注釈の型不一致による拒否へ
入力を修正した。拒否診断の型、別sessionの成功、宣言の共有・型変数、checkpoint往復を検査する。
最初の局所全体試行は482 passed、1 failed、253は選択対象外、exit 100であり、成功扱いしない。
次の試行では宣言の型検査が成功した後、テストが架空の`Global::cow_identity`をregistryのkeyとして
仮定したため、同じ追加回帰だけが失敗した。Astraへ再相談し、解決済み`Resolved::Def`のUIDを
取得する形へ修正した。呼出し入力を解決した際にも同じUIDを検査し、名前のfallbackは追加しない。

source上限回帰の直接対照は、変更前real 3.48sから変更後3.41s、どちらも1 passed、exit 0。
この単独測定では改善幅は小さい。元の文字数・マルチバイト境界と全assertionを保持し、
大量改行の解析を文字数契約へ混ぜないようにした変更として記録する。

CI setupの`cargo run -p scar`にも別の改善候補を確認した。workspaceのtest buildと
同source / 同profileで依存fingerprintの異なるexampleが残り、追加のRust buildが発生していた。
[Cargoの対象選択](https://doc.rust-lang.org/cargo/commands/cargo-test.html)と
[nextest setupのtarget基準](https://www.nexte.st/docs/configuration/setup-scripts/)では、
workspace全体のbuild済みexampleを直接使う案がある。ただし`--test`付き部分実行では
exampleのbuildを保証できず、既存のcold CLI運用を壊し得る。今回はsetupを変更しない。

修正後の`shared_registry_tests`は2 passed、1.772s、332は選択対象外、exit 0。
analysisの新規構文再利用回帰と既存ケースは局所全体試行で成功し、最終CIで追加回帰も含めて
全件を確認する。Astraのfixture修正後レビューでは追加の指摘がなかった。

孤立した空のsemantic cacheで標準checkを再観測した。

| 観測 | 変更前 | 変更後 |
|---|---:|---:|
| CLI全体real | 1.56s | 1.37s |
| Scar型検査 | 724.219ms | 592.713ms |
| child spawn（どちらも50回） | 63.916ms | 24.384ms |
| isolated body（どちらも618回） | 345.860ms | 297.975ms |

この局所観測では型検査が約18.2%、child spawnの累積時間が約61.9%減った。
各1回の測定であり、変更していない処理にも時間差がある。並列CI全体の効果とは分けて扱う。

### 指定clean計測

同じworktreeで`cargo clean && cargo build && cargo nextest run --all --profile ci`を直列実行した。
終了コード0、2,316 passed、0 skipped。追加した回帰3件も成功した。

| 区間 | 第2回後 | 第3回後 |
|---|---:|---:|
| `cargo build` | 23.31s | 21.91s |
| nextestのtest build | 34.33s | 32.89s |
| setup | 20.727s | 20.006s |
| nextest Summary | 110.869s | 103.700s |
| コマンド全体real | 207.76s | 204.44s |
| コマンド全体user / sys | 852.76s / 54.34s | 833.42s / 51.20s |
| nextest件数 | 2313 passed / 0 skipped | 2316 passed / 0 skipped |
| テストプロセスの累積実行時間 | 611.329s | 568.193s |

前回からSummaryは7.169秒、約6.5%、全体realは3.32秒、約1.6%短縮した。
累積実行時間は43.136秒、約7.1%短縮した。各1回の測定であり、負荷の揺らぎを含む。
source上限回帰はCIで7.391秒から7.199秒となり、この変更だけで大幅に短縮したとは扱わない。

| 対象 | 第2回後の累積時間 | 第3回後の累積時間 |
|---|---:|---:|
| Rune integration | 168.720s | 156.728s |
| Scar surface | 40.345s | 36.035s |
| analysis service | 79.638s | 71.927s |
| LSP adapter | 31.284s | 27.732s |
| Xldr REPL core | 53.715s | 48.619s |

作業開始時の同worktree基準149.560秒と比べると、Summaryは45.860秒、約30.7%短縮した。
全体realは252.54秒から204.44秒へ、約19.0%短縮した。
今回、既存テストは減らさず、Scarの共有状態2件とanalysisの構文再利用1件を追加した。
検査・特殊化条件、宣言UID、source provenance、schema / VM版、timeout設定は維持した。

### 残件とログ

CI setupの追加Rust build、Scarのenv等のmapコピー、各processの標準AST parseとprefix復元は残る。
Projectの標準resolve/typecheckを省く変更は導入していない。

第3回のログは`/tmp/surtr-test-perf-d975/round3/`へ保存した。

- `focused-before.log` / `focused-after.log` / `focused-after-warm.log`: 局所対照と変更後初回の差。
- `local-after.log` / `local-after-fixed.log`: 追加回帰fixtureが不適切だった失敗試行。
- `registry-fixed.log`: fixture修正後の追加回帰2件。
- `large-source-before.log` / `large-source-after.log`: source上限の単独対照。
- `std-profile-before.log` / `std-profile-after.log`: 孤立したsemantic準備のprofiler観測。
- `final-clean.log` / `comparison.json`: 指定計測と各テスト時間の前後集計。
- `fmt-check.log` / 各日本語lintログ: 最終差分の整形確認。

`cargo run -- test --quiet --all`も終了コード0で成功した。ログは`standard-srt.log`。
最終の`cargo fmt --all -- --check`と`git diff --check`は成功した。
必須検証の失敗・未実行は残っていない。コミット・マージ・pushは行っていない。

## 第4回の改善

継続指示に従い、入力ファイルなしでlevel 2の改善を続ける。前3回の差分を保持し、
Scarのコピーとテスト準備をサブエージェントへ調査分担した。新しいAstra顧問には
独立コンテキストで変更境界を確認してもらう。ビルド・テストは親が直列実行する。

### 方針と変更前測定

`trait_method_type_lists`の22ケースと`nominal_constructor_parameters`の21ケースを、
既存の8 bucketとinventoryへ登録する。43ケースの本文・入力・assertion・helperは保持し、
各`check`が標準checkpointから新しいsessionを作る境界も維持する。共有するのは
process-localの標準準備だけ。標準上書きや環境変更がないことをAstraが独立確認した。
登録漏れと重複は既存inventoryと`dead_code` denyで検査する。

変更前の`cargo nextest run -p scar --test trait_method_type_lists --test nominal_constructor_parameters`
は43 passed、0 skipped、2.433s、exit 0。CI setupの追加ビルドについても、
部分targetで必要なartifactを正しく構築する条件を保てるか調べる。

### 内部コピーとsetupの変更

Scarの環境正規化は、`BuiltinFunc` / `UserFunc` / `Func`を除外する前に型をcloneしていた。
借用した型を同じ条件で判定し、必要な型だけ`resolve_ty`で所有値を作って更新する。
全keyの走査、除外条件、解決と更新の順は保持する。特殊化用のmapも、
保持mapのcloneを別の空mapへ再挿入せず、cloneを直接初期値にする。
新しいgeneric定義のArc再利用は、同一fun indexの重複時に保持対象が変わり得るため導入しない。

setupの局所probeでは、workspace test build後の
`cargo build -p rune -p scar --bin surtr --example scar-test-prewarm --message-format=json`が
追加compileなし、Finished 0.22s、real 0.27s、exit 0だった。
この結果からCargo JSONの実行ファイルを直接使うhelperを試作したが、Astraの独立レビューで
`cargo run`のtarget runnerと共有ライブラリ探索環境が失われることを確認した。
[Cargo設定](https://doc.rust-lang.org/cargo/reference/config.html)の`build.target`と
`target.*.runner`は環境変数だけでなく階層configからも適用されるため、
文書へのnative限定追記だけでは従来の実行境界を保持できない。
直接実行の案とPython helper・mock testは取り下げ、Python依存も追加しない。

代案はScar prefixを準備する既存exampleの所属をRuneへ移し、Cargo経由の実行を維持する。
Runeの既存Eldr依存により、workspaceと同じ依存featureをexampleへ適用できるか確認する。

Astra確認後、既存`scar-test-prewarm` exampleをRuneへ移設する案を採用した。
旧Scar exampleとtarget宣言は削除し、Runeのdev依存はworkspaceに既存の
serde（derive）・bincode・sha2・scar-test-cache-keyだけを追加する。
example本文はsupportの相対pathだけを変更し、prefix準備と`NEXTEST_ENV`への公開は保持する。
RuneとScarは同階層なのでhelperのcache場所は同じで、独立key crateの生成方法も保持する。
`cargo run`を継続するため、Cargoのconfig解決、runner、共有ライブラリ環境を自作しない。

43ケースの準備共有後の局所対照は18 passed、0 skipped、1.008s、exit 0。
元の43ケース本文をそのまま実行し、inventory 2件を追加した18プロセスで検証する。
Scarのコピー変更後の`cargo nextest run -p scar -p xldr`は398 passed、
84はdefault profileの選択対象外、33.956s、exit 0だった。

### 局所検証と独立レビュー

Runeへ移したexampleを使う`cargo nextest run -p scar --test trait_method_type_lists --profile ci`は
9 passed、0 skipped、Summary 9.590s、setup 9.015s、exit 0だった。
Rune targetを選ばない部分実行でもsetupが必要なexampleを構築し、Scarの厳密なprefix読込が成功した。
初回setupには変更後の必要artifactのbuildを含む。温まった状態のsetup単独実行は
real 4.85s、user 0.54s、sys 0.14s、exit 0。最終clean計測とは分けて扱う。

同じ標準checkを、それぞれ孤立した空のsemantic cacheで観測した。

| 観測 | 変更前 | 変更後 |
|---|---:|---:|
| Scar型検査 | 611.154ms | 608.273ms |
| 環境bindingの正規化（どちらも62回） | 58.578ms | 14.890ms |
| 特殊化フェーズ | 26.333ms | 32.458ms |
| CLI全体real | 2.31s | 1.36s |

環境bindingの正規化はこの観測で約74.6%短縮した。一方、型検査全体はほぼ同じで、
特殊化フェーズは増えた。各1回の測定なので、内部コピー削減だけでCLI全体の差を説明しない。
特殊化mapの二重構築削減についても、局所時間の短縮を確認したとは扱わない。

Astraの最終具体diffレビューでは、新たな指摘がなかった。全43ケースの登録順と本文、
exampleの移設前後本文を機械的に照合し、旧targetと撤回helperの削除、
Cargo.lockのRune依存追従、cache-keyとwireの維持を確認した。
2026-10-10に作業を再開し、残る指定clean計測を実行する。

### 指定clean計測

同じworktreeで`cargo clean && cargo build && cargo nextest run --all --profile ci`を直列実行した。
終了コード0、2,291 passed、0 skipped。

| 区間 | 第3回後 | 第4回後 |
|---|---:|---:|
| `cargo build` | 21.91s | 26.08s |
| nextestのtest build | 32.89s | 36.42s |
| setup | 20.006s | 4.789s |
| nextest Summary | 103.700s | 89.910s |
| Summaryからsetupを引いた参考値 | 83.694s | 85.121s |
| コマンド全体real | 204.44s | 193.66s |
| コマンド全体user / sys | 833.42s / 51.20s | 827.23s / 55.46s |
| nextest件数 | 2316 passed / 0 skipped | 2291 passed / 0 skipped |
| テストプロセスの累積実行時間 | 568.193s | 574.913s |

前回からsetupは15.217秒、約76.1%短縮した。Summaryは13.790秒、約13.3%、
全体realは10.78秒、約5.3%短縮した。主な短縮はsetupであり、
setupを引いた参考値とプロセス累積時間は増えている。型検査コピー削減の効果を
全体wall timeの短縮として断定しない。build時間も増えており、各1回の測定には負荷の揺らぎがある。

| 対象 | 第3回後の累積時間 | 第4回後の累積時間 |
|---|---:|---:|
| nominal constructor parameters | 8.956s | 3.872s |
| trait method type lists | 12.177s | 5.782s |
| Rune integration | 156.728s | 156.956s |
| Scar surface | 36.035s | 36.491s |
| analysis service | 71.927s | 72.501s |
| LSP adapter | 27.732s | 31.016s |
| Xldr REPL core | 48.619s | 51.868s |

集約した2 targetの累積時間は21.133秒から9.654秒へ減った。nextestの件数は25減ったが、
元の43ケースは削除していない。43個の個別processを16 bucketと2 inventoryへ変更した差である。
作業開始時の149.560秒からはSummaryが約39.9%、全体real 252.54秒からは約23.3%短縮した。

### 残件とログ

Scar envのmapコピー、各processでの標準AST parseとprefix復元、Projectの全体semantic検査は残る。
setupの追加buildを完全に除去したとは扱わない。`cargo run -- test --quiet --all`の準備でも
XldrとRuneの追加build（3.32秒）を観測しており、Cargo targetごとのartifact差は残っている。

第4回のログは`/tmp/surtr-test-perf-d975/round4/`へ保存した。

- `start.patch` / `case-inventory.json`: 開始時差分と43ケース本文の照合情報。
- `focused-before.log` / `focused-after.log`: 2 targetの準備共有の局所対照。
- `scar-xldr-after.log`: 型検査とREPLの回帰検査。
- `setup-build-probe.jsonl` / `setup-build-probe.log`: 撤回案のCargo build観測。
- `partial-ci-after.log` / `setup-hot-after.log`: 移設後の部分実行とsetup単独観測。
- `std-profile-before.log` / `std-profile-after.log`: 孤立したsemantic準備の観測。
- `final-clean.log` / `comparison.json`: 指定計測と各テスト時間の集計。
- `standard-srt.log` / `fmt-check.log` / 日本語lintログ: 最終確認。

`cargo run -- test --quiet --all`も終了コード0で成功した。
最終の`cargo fmt --all -- --check`、`git diff --check`、setupのshell構文確認は成功した。
日本語lintは参考警告を確認し、意味を変える警告消しは行っていない。
必須検証の失敗・未実行は残っていない。コミット・マージ・pushは行っていない。

## 第5回の改善

入力ファイルなしの継続指示に従い、意味論を変えないlevel 2の改善を続ける。
前4回の差分は保持し、Scarとanalysis / Runeの調査をサブエージェントへ分担する。
Astraは独立コンテキストで、新しい共有境界とテストの配置を確認する。
ビルド・テストは親が直列実行し、最終計測前にRust差分を固定する。

### 方針と受入条件

- TypeEnvの型定義、enumのconstructor / tag索引、enum一覧を不変の間だけ共有する。型定義のsignature / span / field policyとenum一覧への追加は、変更する値だけを分離する。公開lookupの参照型、変数binding、fresh ID、scope undo、rollbackの範囲は維持する。外側mapのコピーは残す。
- enumの3索引、宣言順、空enum一覧、重複ID / tagの拒否前後の不変性を保持する。保持状態・兄弟envの分離、serialized metadataの形、TypeEnv往復とcounterをcrate-localで確認する。deserialize後の索引間pointer共有は要求せず、再intern経路を追加しない。schema / VM版は変更しない。
- analysisでuser ASTが空の場合だけ、先に収集済みの同一宣言結果を借用する。非空入力の追加収集、Projectの全体resolve / typecheck、宣言UID、標準sourceのauthority、標準編集中の現在document優先を保持する。
- RuneのCLI / source位置を観測しない純粋なVM回帰2件と、CLI回帰末尾の独立したVM部分1件を、既存language_features registryへ移す。入力、source名、expect、assertを保持し、VMはケースごとに作る。CLI / structured span / bytecode / file境界を観測する7件は元のcold実行を保持する。移したVMケースはdefault対象へ分類を改める。timeoutは延長しない。

標準checkの変更前は孤立した空のsemantic cacheを使い、Scar 563.188ms、real 1.31sだった。
この単独profiler観測は並列CIのwall timeと分ける。

### 変更前の局所計測

`cargo nextest run -p surtr-analysis -p surtr-lsp -p rune --profile ci -E 'binary(service) | binary(adapter) | test(/^language_features::/) | test(/^error_source_locations::/)'`
は64 passed、354は選択対象外、0 failed、Summary 41.925s、setup 2.299s、exit 0だった。
既存の言語bucketとCLI source位置の回帰、analysis service、LSP adapterを一緒に観測した。
宣言収集の省略やVMケースの配置変更前の対照として保持する。

### 受入テストと実装

metadata共有の受入2件を先に追加した。`cargo nextest run -p scar --lib -E 'test(/^env::tests::cloned_/)'`
はcompile成功後、共有を要求する参照pointerのassertで2件とも失敗し、exit 100だった。
構文・環境のエラーではなく、従来の独立コピーと追加する不変値共有の契約差を確認したRedである。
82件は選択対象外。変更時の分離、保持値、serialized metadata、counterとscope undo、
enum追加と重複拒否は、実装後のGreenで続けて検査する。

analysisとRuneの担当サブエージェントは3ファイルを変更し、Rust差分を固定した。
既存VM registryは62から65ケースへ増え、旧個別test 2件を削除した。
機械照合で2件の本文と分離した末尾、残るCLIの入力・assertionが同一であることを確認した。
ケース名のsource位置prefixを外したため、移管先bucketは3 / 2 / 6になった。

Astraの受入テストレビューで、署名更新後のmutable lookupだけでは、
mutable lookup単独の分離を検査できないとの指摘があった。共有中の別cloneから
spanとpolicyを更新する経路を追加し、保持状態・兄弟の不変性と変更値だけの分離を確認する。
署名確定前のcloneがDeclared / 空fieldsを保つ検査も既存テストへ追加した。

実装後のenvテストは7 passed、77は選択対象外、0.021s、exit 0。
`cargo nextest run -p scar -p xldr`は400 passed、84はdefault profileの選択対象外、
44.435s、exit 0。前回の33.956sより増えており、この局所実行を短縮の証拠にはしない。
新しく追加した所有状態・wireの回帰も成功した。

Astraの最終具体diffレビューで新しい指摘はなかった。共有粒度、各変更口、
重複拒否、empty一覧、直列化形、lookupの参照型、宣言stage / UID / policy、
移管した3ケースとCLI 7件の本文維持を確認した。残る実行検証は親が進める。

変更後の同じ局所選択は62 passed、354は選択対象外、Summary 25.928s、
setup 2.161s、exit 0だった。元の個別VM 2件をbucketへ移したためRust test件数は2減るが、
元の入力・assertionは保持し、分離したVM部分も含めregistryから実行する。
変更前41.925sとの差は15.997秒、約38.2%。各1回の局所計測であり、
400件のScar / REPL実行が増加した事実と併せて記録し、負荷の差も含む結果として扱う。
CLI 7件はこのCI選択で実行し、移管先の既存bucketも15秒の制限内で成功した。

孤立した空のsemantic cacheで標準checkを観測した。

| 観測 | 変更前 | 変更後 |
|---|---:|---:|
| CLI全体real | 1.31s | 1.31s |
| CLI全体user | 2.99s | 2.93s |
| Scar型検査 | 563.188ms | 588.396ms |
| child spawn（どちらも50回） | 24.663ms | 25.487ms |
| isolated body（どちらも618回） | 260.022ms | 272.295ms |

この単独観測ではmetadata共有による時間短縮を確認できなかった。
値が複製時に共有され、更新時に分離されることは受入テストで確認しているが、
structuralなコピー削減とwall timeの改善は分けて扱う。各1回の測定であり、
変更していない型比較等も増えているため、時間差だけでデグレの原因を断定しない。

### 指定clean計測

Rust差分を固定し、同じworktreeで`cargo clean && cargo build && cargo nextest run --all --profile ci`を
直列実行した。終了コード0、2,291 passed、0 skipped。追加したenv回帰2件と
個別VMテスト2件のregistry移管が相殺し、Rust test件数は前回と同じである。
元のVM入力・assertionとCLI 7件は維持した。

| 区間 | 第4回後 | 第5回後 |
|---|---:|---:|
| `cargo build` | 26.08s | 23.46s |
| nextestのtest build | 36.42s | 36.75s |
| setup | 4.789s | 4.071s |
| nextest Summary | 89.910s | 94.912s |
| Summaryからsetupを引いた参考値 | 85.121s | 90.841s |
| コマンド全体real | 193.66s | 186.49s |
| コマンド全体user / sys | 827.23s / 55.46s | 821.69s / 56.35s |
| nextest件数 | 2291 passed / 0 skipped | 2291 passed / 0 skipped |
| テストプロセスの累積実行時間 | 574.913s | 621.249s |

全体realは7.17秒、約3.7%短縮した。一方、Summaryは5.002秒、約5.6%増え、
setupを引いた参考値とプロセス累積時間も増えた。第5回でnextest全体が速くなったとは扱わない。
局所選択の短縮、構造上のコピー削減、指定コマンド全体の短縮を、それぞれ別の結果として記録する。
各1回の測定であり、負荷の揺らぎやbuild以外の起動・準備も含むため、全体差を単一の変更へ帰属させない。

| 対象 | 第4回後の累積時間 | 第5回後の累積時間 |
|---|---:|---:|
| Rune integration | 156.956s | 163.985s |
| Scar surface | 36.491s | 37.679s |
| analysis service | 72.501s | 80.360s |
| LSP adapter | 31.016s | 35.908s |
| Xldr REPL core | 51.868s | 57.496s |
| Scar unit | 10.528s | 10.353s |

VMを移管したsource位置targetは9 testから7 testへ減り、累積19.263秒から14.029秒になった。
移管先のlanguage_features 8 bucketは累積32.268秒から34.759秒になった。
両者の合計は51.531秒から48.788秒へ減ったが、他のtargetの増加を相殺していない。
作業開始時との比較では、Summary 149.560秒からは約36.5%、全体real 252.54秒からは約26.2%短縮した。

### 残件とログ

TypeEnvの外側map、変数binding、scope undoは引き続き所有する。
標準AST parseとprefix復元、非空user ASTの宣言収集、Project全体のsemantic検査も残る。
今回の単独profilerでmetadata共有による時間短縮は確認できていない。
最終計測の増加についても独立したAstraへ相談した。増加は複数targetに広がっており、
並列負荷だけでなくArc値の復元時の割当て費用も未分離である。原因は未特定で、
今回の機能検証を終える前に追加切り分けを必須とはしない判断だった。
次に時間効果を確かめるなら、metadata共有化だけを切り替え、同じ選択を交互に測定する。
clone内のmap走査・metadata複製とprefix復元の割当てを分けてから、共有粒度の追加変更を判断する。
言語意味論やエラー境界を変更する案は採用していない。

第5回のログは`/tmp/surtr-test-perf-d975/round5/`へ保存した。

- `start.patch`: 前4回を含む開始時差分。
- `focused-before.log` / `focused-after.log`: analysis / LSP / Runeの局所対照。
- `metadata-red.log` / `metadata-green.log`: 不変metadataの共有と更新時の分離の受入検査。
- `scar-xldr-after.log`: 型検査とREPLの回帰検査。
- `std-profile-before.log` / `std-profile-after.log`: 孤立したsemantic準備の観測。
- `final-clean.log` / `comparison.json`: 指定計測と各テスト時間の集計。
- `standard-srt.log` / `fmt-check.log` / 日本語lintログ: 最終確認。

`cargo run -- test --quiet --all`は終了コード0で成功した。
この実行前にXldrとRuneの追加build 3.39秒を観測しており、Cargo targetごとのartifact差は残る。
最終の`cargo fmt --all -- --check`と`git diff --check`は成功した。
日本語lintの参考警告を確認し、技術的な意味を変える警告消しは行っていない。
必須検証の失敗・未実行は残っていない。コミット・マージ・pushは行っていない。

## 分割コミット

2026-10-10の依頼に従い、第1〜5回の改善差分を責務ごとに分割した。
保存先ブランチは`codex/ci-test-scaling`、ワークツリーの物理パスは
`/Users/haruca/.codex/worktrees/d975/surtr`である。

| コミット | 内容 |
|---|---|
| `59c63824` | Scarの永続definition・trait・callable共有と冗長なコピーの削減 |
| `0c681cc5` | TypeEnvの型定義・enum metadata共有と更新時の分離 |
| `8e507d90` | Scar / Forgeの標準prefixを使うケースのregistry集約 |
| `9b4f8f45` | analysisの標準構文準備と空入力の宣言収集結果の再利用 |
| `071e015f` | analysis / LSPの同じ標準環境を使うテストのbucket集約 |
| `cf1086ce` | Runeのassertion拒否入力のcrate-local移管とCLI caption入力の集約 |
| `742a3ac2` | source位置を観測しないVM回帰のlanguage_features移管 |
| `1890d0af` | Scar prefixのprewarm exampleをRuneへ移設し依存graphを共通化 |

改善ログは最後の独立した文書コミットにまとめた。
Scar内部共有とTypeEnv共有は、`predeclare.rs`のenum一覧挿入を差分単位で分けた。
Cargoの`serde rc`追加とexample移設、テスト方針の各説明も対応するコミットへ分配した。
担当サブエージェントが読み取りで依存境界を確認し、不足は見つからなかった。

コミット前後の33パスの内容を照合し、検証済みの実装・テスト・manifest・正本文書を変更していない。
今回はGit操作とこの記録の追加だけを行い、前回成功したclean CI 2,291件と標準テストは再実行していない。
マージ・pushは行っていない。


## 第6回: タイムアウト余裕を作るテスト分割

2026-10-10の依頼で、直前の読み取り調査を入力としてlevel 2の変更を行う。
追加テスト群の入力・期待値・ケース本体は維持し、既存テストの実行単位を分割する。
対象はワークツリー`/Users/haruca/.codex/worktrees/d975/surtr`の`590e1eb2`である。
言語・製品コード、timeout設定、テストの除外条件は変更しない。

### 入力調査と変更方針

- source長上限の既存テストは、上限直前の成功、上限超過mainの拒否、上限超過include先の拒否を独立した3テストへ分ける。元の入力と全assertionを維持し、CLI起動回数は3回のままにする。
- Scar surfaceは既存8 bucketのうち0・3・6・7だけを二分し、12実行単位にする。各ケースの順序と独立したsessionを維持する。登録ケースと生成される実行単位の対応をinventoryで検査する。
- REPL coreは8 bucketから16 bucketへ分ける。ケース関数の内部にある同じengineでの状態遷移は分割しない。ケース名・関数・入力・期待値を維持し、実行単位の一意性と非空をinventoryで検査する。
- analysisのProjectケースは既に個別実行へ戻されている。LSPの失敗から復帰するhostの状態遷移、約3秒で均衡するForge、短縮済みのcaption・型境界テストは変更しない。

保存済みの第5回ログにはtimeoutがない。source長上限は第5回の変更前局所計測で14.679秒、最終cleanで8.041秒だった。分割は1テスト当たりの実行時間に余裕を作るためであり、総時間の短縮とは区別する。

### 受入条件と検証

元のケース・入力・assertionが保持され、全登録ケースが実行単位のちょうど一つに所属することを確認する。親エージェントがビルドとテストを直列に実行し、分担エージェントはビルド・テストを実行しない。

対象の同条件比較には次のコマンドを使う。分割後はsource長上限の3テストを選択するよう、そのフィルターだけ変更する。

```sh
cargo nextest run --profile ci -p scar -p xldr -p rune -E 'binary(typecheck_surface) | binary(repl_core) | test(/^error_source_locations::error_source_location_rejects_sources_that_exceed_the_span_encoding_range$/)'
```

変更前は22 passed、714は選択対象外、Summary 18.255秒、exit 0だった。
最終差分では入力文書のcleanコマンドと標準Surtrテストを直列実行し、timeout・失敗・未実行を記録する。分割前後の最大テスト時間と総時間を分けて評価し、独立レビューでケース保持と実行単位の完全性を確認する。

ログは`/tmp/surtr-test-perf-d975/round6/`へ保存する。


### 実装と局所計測

Scarの428登録ケースとREPLの173登録ケースは、registry・ケース本体の変更前後の内容一致を確認した。Scarは単一の分割設定から12テストとinventoryの設定を生成し、REPLは単一の宣言から16テストとbucket IDを生成する。両inventoryは全ケースの一意な所属と各実行単位の非空を検査する。
source長上限は独立した一時ディレクトリを使う3テストへ分けた。上限直前の文字数・成功・stdout・空stderrと、超過入力の終了失敗・LoadError・対象ファイル・Bootstrap誤帰属禁止・stdout空をすべて保持した。CLI起動は3回のままである。
独立レビューではケースの欠落、期待値の弱化、状態復元の変更は見つからなかった。関連する既存のテスト方針文書も実行単位数に追従した。

分割後の局所コマンドは次のとおり。

```sh
cargo nextest run --profile ci -p scar -p xldr -p rune -E 'binary(typecheck_surface) | binary(repl_core) | test(/^error_source_locations::error_source_location_.*_the_span_encoding_range$/)'
```

| 観測 | 変更前 | 変更後 |
|---|---:|---:|
| 選択対象 | 22 passed | 36 passed |
| 選択対象外 | 714 | 714 |
| Summary（setupを含む） | 18.255s | 15.387s |
| Scar surfaceの最大テスト時間 | 5.586s | 4.053s |
| Scar surfaceの累積時間 | 33.155s | 35.878s |
| REPL bucketの最大テスト時間 | 9.417s | 3.685s |
| REPL bucketの累積時間 | 59.742s | 47.346s |
| source長上限の最大テスト時間 | 7.465s | 6.688s |
| source長上限の累積時間 | 7.465s | 6.871s |

両実行ともexit 0、timeoutなし。増加した14実行単位はScarが4、REPLが8、source長上限が2であり、元の製品ケースを増減したものではない。各1回の局所測定で、並列負荷の揺らぎを含む。Scarの累積時間は増えており、分割で準備費も減ったとは扱わない。
source長上限は正常入力側が6.688秒、main拒否が0.108秒、include先拒否が0.075秒だった。三等分の時間短縮ではなく、正常入力の検査が依然支配的である。入力を小さくして上限境界を失わせる変更は行わない。


### 最終clean CI

Rust差分を固定した後、入力文書の`cargo clean && cargo build && cargo nextest run --all --profile ci`を直列実行した。exit 0、2,305 passed、0 skipped、timeout・失敗・leakなし。前回の2,291件との差14件は実行単位の分割による。

| 区間 | 第5回の保存済み計測 | 第6回 |
|---|---:|---:|
| cargo build | 23.46s | 24.67s |
| nextest test build | 36.75s | 37.32s |
| setup | 4.071s | 4.023s |
| nextest Summary（setupを含む） | 94.912s | 95.775s |
| コマンド全体real | 186.49s | 197.01s |
| コマンド全体user / sys | 821.69s / 56.35s | 835.95s / 56.77s |
| テストプロセスの累積時間 | 621.249s | 624.714s |

| 対象 | 前回の最大時間 | 今回の最大時間 | 前回の累積時間 | 今回の累積時間 |
|---|---:|---:|---:|---:|
| Scar surface bucketのみ | 6.287s | 4.018s | 37.653s | 39.035s |
| REPL bucketのみ | 7.628s | 5.622s | 52.937s | 69.251s |
| source長上限 | 8.041s | 7.636s | 8.041s | 7.829s |

bucketのみの集計はinventoryや同target内の個別テストを含まず、第5回のtarget全体の表とは範囲が異なる。
ScarとREPLは最大テスト時間が小さくなり、15秒制限に対する余裕を確認した。一方で累積時間は増え、REPLではプロセス増加による準備の反復が残る。source長上限は今回も正常入力側が支配的で、最大時間の減少は小さい。
全体Summaryは0.863秒、約0.9%増え、コマンド全体realは10.52秒、約5.6%増えた。今回の変更を全体速度の改善とは扱わない。前回保存ログと今回各1回の測定であり、負荷や起動・一覧取得を含む区間の差もあるため、全体増加を分割だけへ帰属させない。

生ログ・照合結果は`/tmp/surtr-test-perf-d975/round6/`に保存した。

- `focused-before.log` / `focused-after.log` / `focused-comparison.json`: 同じ対象を選んだ局所比較。
- `final-clean.log` / `clean-comparison.json`: clean CIと第5回保存ログの集計比較。
- `preservation.json`: registry・ケース本体・他のsource位置テストの内容一致。
- `fmt-check.log` / `input-doc-lint.log` / `policy-doc-lint.log`: 書式と日本語文書の確認。
- `standard-srt.log`: 標準Surtrテストの結果。


`cargo run -- test --quiet --all`はexit 0、real 20.50秒で成功した。quiet実行のためケース件数は報告しない。最終Rust差分の`cargo fmt --all -- --check`と`git diff --check`は成功し、独立レビューに必須指摘はなかった。日本語lintの警告は参考として確認し、既存記録の用語・引用・検証値は変更していない。
変更はテスト3ファイルと直接関連する既存文書2ファイルのみで、製品コード・追加ケース本体・timeout設定は変更していない。必須検証の失敗・timeout・未実行は残っていない。コミット・マージ・pushは行っていない。


### main統合の依頼

2026-10-10、検証済みの第1〜6回の改善をmainへ統合し、作業ワークツリーとブランチを削除する依頼を受けた。本ターンではテストを再実行しない。対象は`codex/ci-test-scaling`と`/Users/haruca/.codex/worktrees/d975/surtr`に限定する。
統合前のmainは`ab374367`で、未コミット変更はなかった。最後の分割差分5パスのみを明示してコミットし、既存の改善コミットとともにno-ff mergeする。Gitの親コミット・祖先関係・統合treeとファイル内容の一致を確認し、その後に対象リソースを削除する。
