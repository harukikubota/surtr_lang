# コード生成・実行時調査の修正方針・継続調査

2026-10-10、現行ソース・正本文書・既存テストから再調査した。2026-10-10の利用者判断は維持し、古い観測を現行の不具合として扱わない。

調査開始時のHEADは`717d0fd301f4d51b506ceb72347b74d4bf8134fc`。終了時のHEADは`c569e5bab2d229e7ce6d42a1d7428cbeec0a92cc`で、差分は別作業による`doc/callable_name_and_syntax_classification.md`の削除だけだった。調査対象の製品コード・実行可能テスト・正本文書は同じである。この再調査では本書と型検査の継続調査書だけを更新し、実装・正本文書・テストファイルは変更していない。以下には、別作業で完了した停止・回収修正と今回の文書整理を追記する。

プロセスの停止・回収は別作業で実装・検証を完了した。契約は[Process Runtime 正本](../docs/dev/ProcessRuntime_spec.md#3101-メッセージの開始と終了)へ反映し、入力仕様書は削除した。RT-02と停止済み本体の保持は解消済み、RT-12は今回の文書整理で対応済みとする。補充失敗の通知、policyの短名照合、init_policy省略は別件として残す。

## 現行の判定

| ID | 現行の判定 | 次の対応 |
|---|---|---|
| RT-02 | 停止・回収修正で解消済み | 実行権限付きの保存と受付閉鎖を正本へ反映。検証記録は末尾 |
| RT-05 | プロセス側の残件 | 補充initの言語Errの通知・target未達の扱いは停止・回収仕様だけでは未確定 |
| RT-06 | 修正対象の短名照合が残存 | プロセス側でcanonical identityへ統一。停止・回収仕様の対象とは別 |
| RT-09 | 省略補完が残存 | プロセス側で`init_policy`省略をparse errorへ変更 |
| RT-11 | 文書修正が必要 | `Result::recover`をspecial formとする古い説明を除く |
| RT-12 | 文書整理済み | 旧移行表を削除し、生成API・起動構成・runtimeの責務へ整理 |

プロセス以外で新たに対応する確定項目はRT-11。型検査側の対応項目は[型検査の継続調査書](typecheck_audit_followup_20261010.md)を参照する。

## RT-11: recoverの説明を現行の通常関数へ揃える

**対応が必要。文書のみの修正。** 元の「Lazy / Facet / recoverの特別loweringを再構成する」という前提は、recoverについて現行ソースと一致しない。

- 現行の`lib/types/result.srt:199`は通常の`def recover(value: Result<$A>, handler: (-> Result<$A>)) -> Result<$A>`である。`match`でOkを返し、Errのときだけ`handler()`を呼ぶ。`@builtin`も専用loweringもない。
- Sigilの`crates/sigil/src/resolver/expr.rs:220`、Scarの`checker/expr.rs:14803`、Forgeの`codegen.rs:12892`にある専用処理は`recover_kind`である。`ErrorKind`のcanonical identityをhidden ABIへ渡す契約と、通常のrecoverを混同しない。
- `docs/dev/EldrVM_spec.md:462`には「`Result::recover`はcompilerがloweringするspecial form」と残っている。これは修正対象。
- `docs/dev/Lazy_spec.md:3-5, 94-98`はLazy契約とErrorKindの解決・消去を既に分けて説明している。Facetも`EldrVM_spec.md:456-459`にcompile-time capabilityとruntime契約の説明がある。これらを新しいspecial formへ作り替える理由はない。

再現入力は次のとおり。現行CLIで終了コード0、`Ok(1)`を得た。

```surtr
print(inspect(Result::recover(Err(NoneError), {|| Ok(1)})))
```

修正先は`docs/dev/EldrVM_spec.md`のrecover説明を中心とする。関連する説明は`docs/dev/Lazy_spec.md`、`lib/types/result.srt`、`lib/kernel.srt`と照合する。受入条件は、通常のrecover、ErrorKindを使うrecover_kind、Lazyの評価規則、Facetの消去・loweringをそれぞれ現行経路で説明すること。署名・runtime ABI・名前解決規則は変更しない。

## プロセス側で管理する項目

停止・回収修正の完了状態と、引き続き別作業で扱う残件を区別する。

### RT-02: 保存Errと再入停止の境界

**解消済み。** [Process Runtime 第3.10.1〜3.10.2節](../docs/dev/ProcessRuntime_spec.md#3101-メッセージの開始と終了)に従い、開始済みwrapperは対象個体と未完了実行、Postprocessing段階が一致する場合に一度だけ保存できる。再入Stop後の外側Reply / Next / ReplyLaterは完了できるが、保存で受付を再開しない。後続の同PIDへの要求はProcessStoppedとなる。公開の再入fixtureと実行段階・早期Err・異常cleanupの内部テストで固定した。「停止後の保存を全て禁止」という旧観点で別修正しない。

停止済み本体の保持も解消した。未完了実行がなくなると本体と不要な管理参照を削除し、旧PIDはstate・mailbox・継続を保持しない。停止識別表はidentityのWeakだけを残し、参照のないentryを管理境界で除く。契約は[第3.12節](../docs/dev/ProcessRuntime_spec.md#312-worker-lifecycle)、件数と物理解放の確認は末尾の検証記録を参照する。

### RT-05: 補充失敗の通知とtarget未達

**残件あり。旧経路の説明は更新が必要。** 現行の`vm.rs:2607`は`vm/process_continuation.rs:1210`で補充用のdetached taskを登録する。`Refill::resume`（同ファイル`428`）はinitの言語Errをそのまま返し、refilling印を解除する。`295`のRust失敗cleanupも印を解除し、schedulerの`874`以降はRust失敗を上へ返す。旧報告の「Rust側失敗の握りつぶし」「init ErrをOkへ変換」を現行の説明として残さない。

残る経路は`DetachedCompletion::Future(None)`である。補充taskの言語Errは`complete_runtime_task`（同ファイル`1033-1052`）に渡され、`Future(None)`で結果が破棄される。所属はtarget未達のままで、失敗通知・記録・再試行をこの経路からは行わない。snapshotには`target`と`live_count`があり不足は観測できるが、失敗理由は分からない。

[Process Runtime 第3.11節](../docs/dev/ProcessRuntime_spec.md#311-supervisor--dynamicsupervisor)は補充開始の時点と重複防止を定めるが、補充init失敗の通知先・再試行は定めていない。公開入力で「初回init成功→補充init失敗」まで到達する再現は今回作っていない。後続では、この条件を再現して言語Err・Rust失敗・target不足を別々に確認し、通知／記録先と再試行方針を決める。一般のsupervisor再起動を既定の対応として追加しない。

### RT-06: policyの短名照合

**修正対象が残存。** `vm.rs:2238-2267`の`effective_supervisor_policy`は完全名に加えて末尾短名でもoverrideを選ぶ。さらに`2033-2052`の`apply_runtime_supervisor_overrides`は末尾が`DynamicSupervisor`ならbuiltinのkeyにも書き込む。この2経路をcanonical identityへ揃える必要がある。

表示用の`Global::`除去は、別namespaceの同名宣言の同一視を許可しない。受入条件は、同じ短名の異なるsupervisorでpolicyが混ざらず、canonical builtinだけがbuiltin policyを変更すること。通常の複数module入力からの到達性は今回未実測。停止・回収仕様はadopt順序を定めるが、このlookup修正を受入条件に含めていない。

### RT-09: init_policy省略

**省略補完が残存。** `crates/spire/src/parser/decl.rs:5066`は`init_policy.unwrap_or(InitPolicy::Eager)`を使う。Worker GenServerの正しい宣言から`init_policy: Eager`だけを除く最小入力は、現行`surtr check`で終了コード0だった。

利用者判断に従い、後続で省略をparse errorにする。明示Eager / Standbyの意味は維持し、parser・既存成功／拒否例・`docs/dev/ProcessRuntime_spec.md:120`の必須項目・利用者向け説明を揃える。[Process Runtime 第4.11節](../docs/dev/ProcessRuntime_spec.md#411-processstatus)の初期化境界だけでは、この必須化は実施されない。

### RT-12: 生成APIと起動規則の説明

**文書整理済み。** `ProcessRuntime_spec.md` 第2節の旧 `@agent` metadata、`Multi`、GenServer未実装を現行とする移行表を削除した。定義から生成公開API、common owner、canonical hidden builtin、runtimeへ進む責務を説明し、第3.10節の実行境界、第3.16節のsingleton PID省略／明示形式、第3.17節の起動構成、第4節の正規化へ参照を揃えた。

停止・回収の入力仕様で確定した契約は正本の対応節に維持し、`docs/site/process.md`と`lib/process.srt`の公開説明に追従した。入力仕様への参照は正本へ置き換え、作業・検証記録は本書末尾に移した。公開API、生成・起動規則は変更していない。補充init失敗、policy lookup、init_policy必須化の残件を、この文書整理で完了とは扱わない。

## 継続調査時の検証と未確認範囲

`surtr-test-strategy`に従い、実装を変えない調査として既存の対象テストを選択した。実行コマンドと件数は型検査の継続調査書末尾にまとめた。Rustの既存テストは3回の選択実行で計70件成功、失敗0件。`rtk cargo build -p rune`も成功し、そのCLIで型検査の最小入力と6件の実行入力を確認した。

この継続調査では、全workspace CI、標準Surtr全件、停止・回収・補充失敗の公開実行、複数namespaceのpolicy衝突、TTYでのREPL操作、メモリ／回収計測は未実施だった。別作業で行った停止・回収修正の検証は次節に記録する。補充失敗の通知、policy衝突、init_policy省略と型検査側の確定項目は引き続き後続作業で扱う。

## 2026-10-10 停止・回収修正の最終検証

サブエージェントでruntime、生成wrapper・builtin、正本文書、identityとcheckpointの回帰を分担した。新規問題とデグレはAstraに独立したコンテキストで相談し、実行開始時のstate snapshot、異常時の同個体cleanup、独立Task・initの実行ID分離、停止表の型検証、呼出し元source origin、leaseの共通受付へ反映した。独立レビューで見つかった保存段階の不足には、handler正常復帰後の `__process_postprocess` と `Handling → Postprocessing → Callback` の検査を追加した。Agent getの成功値は既存のcanonical constructor ASTで構築し、一般の関数呼出し規則は変更していない。

現行mailboxには要求の投入・dispatch経路がない。新しい未開始queueは作らず、受付確認・state snapshot取得・実行登録を無yieldで済ませたものを開始済みとする。非空の未使用mailboxは内部契約違反として拒否する。

短いtimeoutだけではhandler開始済みを保証できないため、公開timeout fixtureは開始・解放の合図を使う。期限なしTaskでcalleeを起動し、開始を確認してからcallerのawaitをtimeoutさせ、解放後のawaitでcalleeの保存・完了を確認する。通常のtimed Task callback取消と開始済みcalleeの分離は、Eldrで開始・待機・timeout・future解決の順序を制御して検証する。固定時間のsleepによる開始・完了の推測やtimeoutの延長を成功条件にしない。

- TDD: 境界外store・state読取、未読／marker前storeの旧成功、生成wrapperの旧経路、hidden参照の未登録、rollbackによるID再利用で意図した失敗を確認した。leaseの共通受付漏れとAgent getterのconstructor ASTのデグレは公開fixture・既存coldテストで再現してから修正した。
- 局所検証: Eldr全309件と追加の開始前／開始後timeout制御2件が成功。Spireの生成境界、Sigilのcanonical hidden参照とprocess仕様保持、Sindrのidentity共有テストも成功。Runeの公開script fixtureは9テスト成功し、broadcastの初期対象維持・supervisor移管・再入保存順・PID差し替えを含む追加fixtureを検証した。停止拒否のdirect / capture / 高階関数 / lease経由とWorkers選択失敗のsource origin、AgentのVM dumpはcoldテスト3件成功。
- 回収の確認: 32回のspawn / stopで本体・実行・task・不要futureを回収し、コピー・lease保持中の停止識別は1件、全handle破棄後の管理境界では0件になる。stateと待機frameのcaptureはWeakで実解放を確認した。checkpointが保持する値はcheckpoint破棄・復元処理後に解放される。
- サイトのPID差し替え例は終了コード0、出力`(False, 0)`で確認した。
- 最終CI: `rtk cargo nextest run --profile ci --workspace --features rune/tui` は2,420件成功（66 binaries、121.926秒、終了コード0）。TUIとcoldテストを含み、追加の除外は行っていない。
- 標準Surtr全件: `rtk proxy cargo run -- test --quiet --all` は終了コード0。quiet出力のため件数は記録していない。
- `cargo fmt --all -- --check` と `git diff --check` は成功。
- 独立レビュー: Astraが仕様・最終差分・検証結果を確認した。保存段階、Agent getter、テストの標準宣言と開始条件の指摘へ対応し、最終差分に未解消の指摘はない。公開fixtureとtimed Task取消の内部テストの役割は上記のとおり区別した。

Lazy初期化全体、Ready前FIFO、fairness、一般のsupervisor再起動・shutdown、BEAMの実装は本修正の完了範囲に含めない。

2026-10-10のmain文書整理後の反映・入力削除・分割コミットの作業では、利用者の指定によりテスト・buildを再実行していない。上記は停止・回収修正の実装時の結果であり、今回新たに取得した結果ではない。
