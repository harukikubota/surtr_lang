# コード生成・実行時調査の修正方針・継続調査

2026-10-10、現行ソース・正本文書・既存テストから再調査した。2026-10-10の利用者判断は維持し、古い観測を現行の不具合として扱わない。

調査開始時のHEADは`717d0fd301f4d51b506ceb72347b74d4bf8134fc`。終了時のHEADは`c569e5bab2d229e7ce6d42a1d7428cbeec0a92cc`で、差分は別作業による`doc/callable_name_and_syntax_classification.md`の削除だけだった。調査対象の製品コード・実行可能テスト・正本文書は同じである。本書と型検査の継続調査書だけを更新し、実装・正本文書・テストファイルは変更していない。

プロセスの停止・回収は[修正中の仕様書](process_stop_and_reclamation_revision_spec.md)で扱う。本調査ではその実装を進めず、仕様の範囲との重複と、含まれていない残件を整理する。仕様に書かれていることだけを理由に実装済みとしない。

## 現行の判定

| ID | 現行の判定 | 次の対応 |
|---|---|---|
| RT-02 | 再入停止と開始済み処理の保存が残件 | 再入Stop・開始済み処理の保存は停止・回収仕様へ統合 |
| RT-05 | プロセス側の残件 | 補充initの言語Errの通知・target未達の扱いは停止・回収仕様だけでは未確定 |
| RT-06 | 修正対象の短名照合が残存 | プロセス側でcanonical identityへ統一。停止・回収仕様の対象とは別 |
| RT-09 | 省略補完が残存 | プロセス側で`init_policy`省略をparse errorへ変更 |
| RT-11 | 文書修正が必要 | `Result::recover`をspecial formとする古い説明を除く |
| RT-12 | プロセス文書の整理が必要 | 生成APIの説明と旧移行表を整理。停止・回収側の文書更新と調整 |

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

この節は停止・回収の別作業への引継ぎであり、本調査の実装対象ではない。

### RT-02: 保存Errと再入停止の境界

停止後の`process_store`と再入処理は別問題である。修正中仕様の第6.1節は、停止前に開始したwrapperの保存を実行権限付きで許し、受付の再開を禁止する。この契約に統合し、「停止後の保存を全て禁止」という旧観点で別修正しない。

### RT-05: 補充失敗の通知とtarget未達

**残件あり。旧経路の説明は更新が必要。** 現行の`vm.rs:2607`は`vm/process_continuation.rs:1210`で補充用のdetached taskを登録する。`Refill::resume`（同ファイル`428`）はinitの言語Errをそのまま返し、refilling印を解除する。`295`のRust失敗cleanupも印を解除し、schedulerの`874`以降はRust失敗を上へ返す。旧報告の「Rust側失敗の握りつぶし」「init ErrをOkへ変換」を現行の説明として残さない。

残る経路は`DetachedCompletion::Future(None)`である。補充taskの言語Errは`complete_runtime_task`（同ファイル`1033-1052`）に渡され、`Future(None)`で結果が破棄される。所属はtarget未達のままで、失敗通知・記録・再試行をこの経路からは行わない。snapshotには`target`と`live_count`があり不足は観測できるが、失敗理由は分からない。

停止・回収仕様第6.3節は補充開始の時点と重複防止を定めるが、補充init失敗の通知先・再試行は定めていない。公開入力で「初回init成功→補充init失敗」まで到達する再現は今回作っていない。後続では、この条件を再現して言語Err・Rust失敗・target不足を別々に確認し、通知／記録先と再試行方針を決める。一般のsupervisor再起動を既定の対応として追加しない。

### RT-06: policyの短名照合

**修正対象が残存。** `vm.rs:2238-2267`の`effective_supervisor_policy`は完全名に加えて末尾短名でもoverrideを選ぶ。さらに`2033-2052`の`apply_runtime_supervisor_overrides`は末尾が`DynamicSupervisor`ならbuiltinのkeyにも書き込む。この2経路をcanonical identityへ揃える必要がある。

表示用の`Global::`除去は、別namespaceの同名宣言の同一視を許可しない。受入条件は、同じ短名の異なるsupervisorでpolicyが混ざらず、canonical builtinだけがbuiltin policyを変更すること。通常の複数module入力からの到達性は今回未実測。停止・回収仕様はadopt順序を定めるが、このlookup修正を受入条件に含めていない。

### RT-09: init_policy省略

**省略補完が残存。** `crates/spire/src/parser/decl.rs:5066`は`init_policy.unwrap_or(InitPolicy::Eager)`を使う。Worker GenServerの正しい宣言から`init_policy: Eager`だけを除く最小入力は、現行`surtr check`で終了コード0だった。

利用者判断に従い、後続で省略をparse errorにする。明示Eager / Standbyの意味は維持し、parser・既存成功／拒否例・`docs/dev/ProcessRuntime_spec.md:120`の必須項目・利用者向け説明を揃える。停止・回収仕様第6.4節の初期化境界だけでは、この必須化は実施されない。

### RT-12: 生成APIと起動規則の説明

**プロセス文書の整理が必要。** 正本には既に`ProcessRuntime_spec.md:478-512`の生成公開API、`809-821`のsingleton PID省略／明示形式、`827-892`のBootPlanとbuiltin supervisor・標準I/O登録がある。

一方、同文書第2.1節には`@agent`の旧meta、`Multi`、GenServer未実装などを「現行」とする移行表が残る。現在の生成helper・common owner・hidden lower・builtinへの流れと整合しない。現在の契約を先に置き、不要な旧移行説明を除く。`docs/site/process.md`と`lib/process.srt`の説明も同じ公開APIへ揃える。

停止・回収仕様T1/T5には同文書の更新があるため、その作業と調整する。ただし停止の契約だけを追従して旧移行表も整理済みとしない。公開APIや生成・起動規則の変更は今回の文書整理には含めない。

## 検証と未確認範囲

`surtr-test-strategy`に従い、実装を変えない調査として既存の対象テストを選択した。実行コマンドと件数は型検査の継続調査書末尾にまとめた。Rustの既存テストは3回の選択実行で計70件成功、失敗0件。`rtk cargo build -p rune`も成功し、そのCLIで型検査の最小入力と6件の実行入力を確認した。

全workspace CI、標準Surtr全件、停止・回収・補充失敗の公開実行、複数namespaceのpolicy衝突、TTYでのREPL操作、メモリ／回収計測は未実施。これらを今回の成功範囲へ含めない。プロセス修正中仕様と本書・型検査側の確定項目を、後続の実装入力として使う。
