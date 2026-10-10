# コード生成・実行時調査の修正方針・継続調査

2026-10-10の利用者判断に基づき、バグ修正・継続調査・文書再構成の残作業を記録する。本書の判断は、前回調査書 `spec_bypass_audit_codegen_runtime.md` の「未選択」「要判断」および併記した選択肢に優先する。対応済みの項目は掲載しない。

前回調査の対象HEADは `7458fe8d8d16aa795d038345d7f86b9d6209ccee`。以下の実装箇所と観測内容はその調査記録から引き継いだもので、最新実装で再確認した結果ではない。今回は利用者の決定を記録し、追加調査・実装・正本文書の修正・テストは行っていない。

## 対応一覧

| ID | 対応 | 対象 |
|---|---|---|
| RT-02 | 継続調査 | state保存の内側ErrとReply / ReplyLater / Nextの失敗伝播 |
| RT-03 | 継続調査 | function remapの欠落targetとgeneric注入先の具体化 |
| RT-04 | バグ修正 | error constructorの欠落・非対応arityをliteral生成で補う経路 |
| RT-05 | 継続調査 | worker refillの失敗処理とtarget未達の扱い |
| RT-06 | バグ修正 | supervisor policyの短名一致による選択 |
| RT-09 | バグ修正 | `init_policy`省略時の`Eager`補完 |
| RT-10 | 変更要否の調査 | REPL向けの空chunk stackからの`Unit`返却 |
| RT-11 | 文書再構成 | Lazy / Facet / recoverの特別lowering |
| RT-12 | 文書再構成 | 生成process helperとbuiltin supervisor |

## バグ修正

### RT-04: error constructorの契約違反をliteral生成で補わない

前回調査では、Forgeの`emit_error_value`とstack版がconstructorのlookup不成立・非対応arityをliteral / templateによるError生成へまとめていた。この補完をバグとして修正する。

必須constructorの欠落と非対応arityを明示的なエラーにし、別の生成経路で隠さない。内部Error生成の正規契約とconstructorを要求する経路の境界を整理する。builtin / process errorの直接生成も関連箇所として確認し、内部Errorの生成方法すべてを一律に変更する判断は本書では追加しない。

対象の手掛かりは`crates/forge/src/codegen.rs`の`emit_error_value`、`crates/eldr/src/builtin.rs`のbuiltin error生成、`crates/eldr/src/vm.rs`のprocess error生成。関連文書は`docs/dev/EldrVM_spec.md`、`docs/site/error-handling.md`、`lib/types/error.srt`。

### RT-06: supervisor policyをcanonical identityで選ぶ

前回調査では、`effective_supervisor_policy`が完全名の一致に加えて短名の一致でもoverrideを選んでいた。別namespaceの同名supervisorへpolicyを流用し得る照合をバグとして修正する。

policy lookupをcanonical identityに揃え、metadataの不一致を短名で補わない。builtin supervisorとの接続もcanonical builtin identityに限定する。表示上の`Global::`正規化と、異なる宣言の同一視を区別する。

対象は`crates/eldr/src/vm.rs`のpolicy選択とbuiltin policy接続、関連正本は`docs/dev/ProcessRuntime_spec.md`。同じ短名を持つ別identityのpolicyが混ざらないことを確認する。前回未確認だった通常入力からの到達性を、確認済みとは扱わない。

### RT-09: `init_policy`省略を拒否する

前回調査の`init_policy.unwrap_or(InitPolicy::Eager)`による省略補完をバグとして修正する。`init_policy`の明示を要求し、省略をparse errorにする。旧来の省略成功例は拒否例へ変更する。

対象は`crates/spire/src/parser/decl.rs`のprocess meta解析。明示した`Eager` / `Standby`の意味は維持し、必須項目の説明を`docs/dev/ProcessRuntime_spec.md`と関連する利用者向け文書へ揃える。

## 継続調査

### RT-02: state保存失敗の伝播

前回調査では、`process_store`が返すSurtr側の`Err`を、`Reply` / `ReplyLater` / `Next`が捨てて成功処理へ進む内部経路があった。調査タスクとして残す。

最新実装で内側ErrとRust側の失敗を区別し、通常の生成wrapperから到達できる条件、保存前後のPID・state検査、callback / futureへの影響を確認する。低層の不正PIDで成立する条件を、通常Surtr入力での再現と混同しない。調査結果から修正要否と失敗の伝播先を決める。

対象は`crates/eldr/src/vm.rs`の`process_store`と各GenServer結果処理。関連正本は`docs/dev/ProcessRuntime_spec.md`、`docs/dev/EldrVM_spec.md`。

### RT-03: function remapとgeneric注入先

前回調査では、remap表にないfunction IDが残存し、`[1, 1, 2] |> List::dedup()`がverifierで拒否された。現在の修正状況を含め、調査タスクとして残す。

generic注入先の具体化、function tableの正規化、verifierの検査を追い、欠落IDが残る条件を確認する。範囲外IDの拒否と、範囲内の別function IDへ誤接続する可能性を分ける。最新ソースでの最小再現と正常な直接呼出しを比較し、原因・影響範囲・修正要否を確定する。

対象は`crates/forge/src/codegen.rs`の`normalize_function_table` / `InjectDirectCall`、`crates/sindr/src/ir.rs`のverifier、および注入先を具体化する型検査経路。

### RT-05: worker refill失敗とtarget未達

前回調査では、membership除去時のrefill呼出しがRust側の失敗を捨て、refill内でもinitのSurtr Errorを成功終了へ変える経路があった。調査タスクとして残す。

最新実装で、初回pool生成と補充時の失敗処理、補充initが失敗する条件、target未達の観測方法を確認する。失敗の通知先やsupervisor lifecycleとの関係を調べ、必要な修正を決める。前回は公開Surtr入力でrefill失敗まで到達する再現を得ていないため、その境界の確認も含める。

対象は`crates/eldr/src/vm.rs`のmembership除去・`refill_worker_set`・`supervisor_spawn`。関連文書は`docs/dev/ProcessRuntime_spec.md`、`lib/process.srt`。

### RT-10: REPL向けの空chunk返却契約を変えるべきか

空chunk stackから`Unit`を返す挙動は、REPLのための仕様として扱う。バグ修正とはせず、現行から変更したほうがよいかを調査する。

定義のみの入力・空chunk・値を返すchunkについて、VMの返却値とREPLの表示処理の役割を整理する。現行の`Unit`返却を維持する場合と変更する場合を、契約の明確さ、実装の単純さ、REPLへの影響で比較する。変更を選ぶ場合は、定義chunkの正規出力契約を先に定める。通常opcodeのstack underflowとは区別する。

対象は`crates/eldr/src/vm.rs`の`execute_chunk`、`crates/eldr/src/interactive.rs`とREPLの結果表示。関連正本は`docs/dev/EldrVM_spec.md`、`docs/dev/Xldr_spec.md`。調査完了時に変更要否とその根拠を残す。

## 文書再構成

### RT-11: Lazy / Facet / recoverの説明を整理する

実装の変更項目にはせず、文書再構成だけを残す。Lazyの評価規則、Facetのcompile-time capability、recoverのspecial formを、それぞれの規則とlowering後の実行契約が分かる構成に整理する。

`docs/dev/Lazy_spec.md`、`docs/dev/EldrVM_spec.md`、`lib/kernel.srt`の関連説明について正本と参照関係を整理し、言語側の署名とruntime ABIの役割を区別する。文書整理を理由に新たなspecial formや名前による救済経路を追加しない。

### RT-12: 生成process helperとbuiltin supervisorの説明を整理する

実装の変更項目にはせず、文書再構成だけを残す。公開owner helper、SingletonのPID省略と明示PID形式、Worker API、hidden lowerへの接続を順に説明する。

`docs/dev/ProcessRuntime_spec.md`を中心に、builtin supervisor・標準I/Oの自動登録と、通常Singletonの明示bootを区別する。公開APIと内部生成契約の説明を整理し、関連する利用者向け文書・標準定義の説明との重複や参照関係を揃える。現在の生成・起動規則の変更は含めない。
