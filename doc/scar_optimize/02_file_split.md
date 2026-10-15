# Scar のファイル分割と状態境界

調査日: 2026-10-15。対象: `a0d430a609da73f7f35bbba7a01f41f9b28dfc09` の `crates/scar/src`。

現行の `Checker` と型検査順序を保ち、責務ごとに実装を移す案を推奨する。まず `expr.rs` の Facet・process・呼出し処理と、`mod.rs` の session・計測処理を分ける。その後、状態の保存・復元を直接検証できる単位で最適化する。ファイルを分けるだけで `.srt` のコンパイルが速くなるとは見込まない。

初回調査ではファイル数・行数・関数宣言数を計測し、実装と既存計画を照合した。ファイル分割は 2026-10-16 に実施した。実施範囲と検証結果は末尾の実施記録を参照。状態の保存範囲やアルゴリズムの最適化は今回の分割に含めない。

## 対象と変更しない契約

対象は Scar 内部の配置、責務、状態の所有関係である。受理・拒否する構文、型推論、Trait 選択、診断の理由・span・順序、Typed IR、公開 API、チェックポイントの形式は変えない。旧経路を残す委譲 wrapper、曖昧な入力を通す fallback、schema / VM version の更新は導入しない。

参照する正本は [Trait system](../../docs/dev/Trait_system_spec.md)、[診断](../../docs/dev/diagnostics.md)、[テスト方針](../../docs/dev/テスト方針.md)。用語は同仕様に従い、`ReturnTypeArgument`、`TypeCtorTrait`、`callable instantiation` を区別する。

単純な移動と Scar 内部に閉じた状態整理は **level2** と判断する。行数が多いことだけを理由に level4 とはしない。推論規則、候補の確定条件、フェーズ間契約を変える必要が生じたら、その変更を切り離して **level4** の仕様にする。今回の大規模作業では、level2 の段階もサブエージェントのレビューを使う。

## 調査時の規模

調査対象 commit の `crates/scar/src/**/*.rs` は **24 ファイル、61,572 行**。空行・コメント・`#[cfg(test)]` 内を含む。関数宣言数は行頭の `fn` / `pub(...) fn` を数えた参考値で、テスト関数・入れ子関数も含む。実行コストや公開 API 数ではない。

| ファイル | 行数 | 関数宣言数 | 主な責務 |
|---|---:|---:|---|
| `checker/expr.rs` | 16,407 | 328 | 式の入口、通常/Trait 呼出し、Facet、process、capture、Lazy、IR 監査 |
| `checker/specialize.rs` | 6,044 | 75 | callable の具体化、IR の走査・置換、具体化後の Pattern |
| `checker/mod.rs` | 5,424 | 156 | 公開 API、session/checkpoint、Checker 状態、実行順序、計測、process 契約 |
| `checker/types.rs` | 5,374 | 125 | 型構文、型関係、置換、IR 型確定、型名表示 |
| `checker/predeclare.rs` | 5,255 | 85 | 宣言登録、where、Trait / impl 登録、constructor mapping |
| `checker/definitions.rs` | 4,269 | 62 | builtin 契約、関数・型・impl の本体、constructor / Error 呼出し |
| `checker/trait_selection.rs` | 3,845 | 84 | canonical 変換、unifier、候補証明、dispatch |
| `checker/provenance.rs` | 2,679 | 46 | constructor capability の由来・伝播 |
| `checker/patterns.rs` | 1,943 | — | Pattern 型検査 |
| `checker/signatures.rs` | 1,729 | — | canonical signature と constructor 入力 |
| `checker/matching.rs` | 1,723 | — | match と網羅性 |

既に `expr/do_block.rs`（1,085 行）と `expr/lazy_calls.rs`（316 行）が分かれている。`trait_selection.rs` は途中にも大きな内部テスト群があるため、行数をそのまま製品処理の大きさと扱わない。

再計測は `rg --files crates/scar/src -g '*.rs'` で対象を列挙し、ファイルごとの `wc -l` で行う。将来の行数上限を合否条件にはしない。

## 既存整理との関係

[010_scar_src_cleanup_plan.md](../optimize/010_scar_src_cleanup_plan.md) は 2026-05-10 の実施記録であり、今回の現行設計そのものではない。ただし、次の整理は現在にも残っている。

| 既存計画 | 現行で確認したもの | 今回の扱い |
|---|---|---|
| session 永続状態の集約 | `PersistentCheckerState`、`with_persistent_state`、`persistent_state_with_env`、`into_persistent_state` | 再導入せず、保存対象と移譲対象を明文化する |
| 引数検査の共通化 | `typecheck_positional_call_args` に加えて `typecheck_ordered_call_args` | named / positional を新しい別実装へ分岐させない |
| process intrinsic の入口集約 | `try_check_process_intrinsic_app` | 入口と実装一式を同じ移動単位にする |
| signature 型構文の共通化 | `SignatureTyMode` と `resolve_signature_like_ast_ty_in_context` | 通常・Trait・builtin の文脈差を保持する |
| Facet 入力の共通化 | `PreparedFacetInput` と `prepare_facet_input` | read / update の共通処理を保持する |

現行の special-form 契約型名は `SpecialFormContract`。古い記録の型名や過去のテスト件数を現在の証拠として転記しない。

## 分割の方針

初期段階では `Checker` を別々のサービス型へ作り替えず、現在の `impl Checker` を責務別ファイルへ移す。これなら移動とアルゴリズム変更を別々に検証できる。以下の表は調査時の移動案であり、末尾の実施記録に実際の配置と追加の責務単位を記録する。

### 1. module の入口と session

| 移動元 | 移動先案 | 移す型・関数群 | 境界 |
|---|---|---|---|
| `mod.rs` | `profile.rs` | `ProfileEvent`、`ProfileCounter`、`ProfileData`、`ProfileSnapshot`、`TypecheckProfiler`、`profile_stmt_*` | 型推論を実行しない。出力形式・有効化条件・集計範囲を保持 |
| `mod.rs` | `bootstrap.rs` | `initialize_env`、`builtin_ty_from_meta`、`BuiltinSignatureParser`、`builtin_pid_marker_from_ty`、型引数 suffix 整形 | Sindr builtin metadata から初期 `TypeEnv` を作る |
| `mod.rs` | `session.rs` | `ScarSession` と公開 typecheck API、`ScarCheckpoint`、`PersistentCheckerState`、checkpoint/rollback | 永続化と成功時 commit。初期段階は型配置を変えるだけ |
| `mod.rs` | `session/function_indices.rs` | `reconcile_*function_indices`、`rekey_specializable_defs`、`rewrite_fun_indices_in_*` | function index の対応付けと IR 全体への反映を一単位で移す |
| `mod.rs` | `program.rs` | `check_program`、constructor application 位置検証、完了時監査の呼出し | 宣言前処理→本体→具体化→監査の順序を所有 |
| `mod.rs` | `warnings.rs` | `collect_unused_type_parameter_warnings`、`collect_unused_value_warnings_*`、収集・判定 helper | warning の発生順序と child からの取り込みを保持 |
| `mod.rs` | `state.rs` | `Checker` の構築、`spawn_child_checker`、`absorb_child_progress`、永続状態との変換 | 状態ごとの copy / merge / discard を一箇所で確認可能にする |

`mod.rs` には module 宣言、既存公開項目の再 export、共通型の最小集合だけを残す。`CallableInstantiationKey` は `checker` が public module のため外部から参照可能であり、`lib.rs` の re-export 一覧だけを見て公開範囲を判断しない。

### 2. expr の責務

| 移動元の具体的な群 | 移動先案 | 移動時に一緒に扱う状態・型 |
|---|---|---|
| `check_node`、`check_non_structural_node`、`check_node_with_expected*`、`check_non_structural_node_with_expected` | `expr/mod.rs` | 式種別の振り分けと期待型の伝達だけを入口に残す |
| `check_registered_callable_application`、`instantiate_named_callable_signature`、`apply_call_site_return_type_arguments`、`complete_call_constraint_set`、`check_app*`、`check_callable_application` | `expr/calls.rs` | canonical signature、ReturnTypeArgument、通常 callable の完成条件 |
| `typecheck_user_function_args`、`typecheck_positional_call_args`、`typecheck_ordered_call_args`、`check_ordered_call_argument`、`check_call_argument_relation` | `expr/arguments.rs` | `OrderedCallArgument`、`CallArgumentMode`。Facet capture の遅延判定を含む引数順序 |
| `check_trait_method_call`、`check_trait_invocation*`、`prepare_trait_invocation`、`check_prepared_trait_arguments`、`finish_trait_invocation`、receiverless target 検査 | `expr/trait_calls.rs` | `TraitInvocationContext`、`PreparedTraitInvocation`、`TraitInvocationPreparation` を同じ場所へ |
| `check_pipe*`、`check_context_apply*`、`check_context_map*`、`check_context_bind*`、`check_*compose*`、`flow_operator_trait_call` | `expr/operators.rs` | `CompositionOperator` と callable 期待型の文脈。Trait 選択本体は複製しない |
| `try_check_process_intrinsic_app`、`try_check_supervisor_*`、`try_check_worker_message_template_app`、`try_check_singleton_explicit_pid_app`、`synthesize_supervisor_worker_init` | `expr/process_calls.rs` | process 用の引数検査。process 宣言の検証とは入口を分ける |
| `check_closure*`、`check_capture*`、`check_constructor_capture`、`check_extractor_closure` | `expr/closures.rs` | 関数 scope、capture、private field owner、scope の保存と復元 |
| `check_list_*`、`check_hash_map_literal`、`check_tuple_literal`、`check_range_literal`、`fold_*range*`、`lower_*range_runtime`、`check_interpolated_str` | `expr/literals.rs` | range の合成 UID と runtime helper の参照を移動で変えない |
| `LazyInput`、`check_lazy_input`、`normalize_lazy_*`、`check_lazy_argument*` と既存 `lazy_calls.rs` | `expr/lazy_calls.rs` | 二つの Lazy 正規化経路を新設せず、既存 module を拡張 |
| `CheckedSafeBindInput`、`check_safebind_input`、`check_safebind` と既存 do の処理 | `expr/do_block.rs` | Pattern 失敗、carrier、戻り値の伝播。通常 binding の処理はここへ混ぜない |

Facet は別節のまとまりで移す。補助関数の呼出し先を追い、ここに挙げた入口だけを移して元ファイルへ往復する構成を避ける。

### 3. Facet と process のまとまり

Facet は `expr.rs` の離れた位置にあり、連続した行範囲の切り取りでは完結しない。

| 移動先案 | 移す主な群 |
|---|---|
| `expr/facet/mod.rs` | `try_check_facet_intrinsic_app`、`PreparedFacetInput`、`FacetPathInput`、`prepare_facet_input`、read/update 引数 parser |
| `expr/facet/path.rs` | `resolve_facet_segment_for_source_ty`、`check_type_root_facet_path_with_expected`、`check_facet_segment_access_with_expected`、`specialize_pending_facet_path`、`compose_*facet_paths`、pending/typed segment 変換 |
| `expr/facet/update.rs` | `rebuild_facet_source_type`、`collect_facet_rebuild_tyvar_replacements`、`ensure_*facet_rebuild_is_unique`、`finalize_facet_update`、set/put/over/case の入口、`check_mutating_facet_path_permissions` |

`pending_facet_capture_source` / `facet_capture_dependency_focus` は Facet 側で依存型を判定し、引数側はその結果を使って再検査を制御する。新たな返却型が必要なら、まず現在の返値と状態変更をそのまま保持した移動を完了させ、別変更として設計する。

process 宣言契約は `mod.rs` の `set_process_handler_dependencies`、`validate_process_state_contracts`、`validate_*handler*state`、`stop_constructor_allowed`、PID capability 判定を `process.rs` へ移す。`expr/process_calls.rs` はこの契約を呼び、同じ handler / state 規則を持たない。

### 4. 型構文、宣言、具体化

| 移動元 | 移動先案 | 主な群 |
|---|---|---|
| `types.rs` | `type_syntax.rs` | `SignatureTyMode`、`resolve_ast_ty_in_context*`、`resolve_signature_like_ast_ty_in_context*`、`resolve_*return_type_argument*`、型構文の位置制約 |
| `types.rs` | `inference.rs` | `types_compatible*`、`bind_tyvar`、`resolve_ty`、`ty_contains_var`、fresh instantiation と bounds 検証 |
| `types.rs` | `resolve_typed.rs` | `resolve_typed_node`、`resolve_typed_pattern`、`resolve_typed_match_pattern`、Facet / origin の型確定 |
| `types.rs` | `type_display.rs` | `diagnostic_ty_name*`、`ty_name`、`format_signature`、戻り値診断に必要な tail span helper |
| `definitions.rs` | `definitions/builtins.rs` | `SpecialFormContract`、shape validator、`check_builtin_*decl`、`ensure_builtin_type_contracts` |
| `definitions.rs` | `definitions/functions.rs` | `check_def`、`check_extractor_def`、`check_body_in_isolated_scope`、return context と capability 使用検査 |
| `definitions.rs` | `definitions/traits.rs` | `check_trait_impl_items`、`check_trait_impl_def`、derive dependency / failure 補足 |
| `definitions.rs` | `definitions/nominal.rs` | struct / record / enum / error 定義検査 |
| `definitions.rs` | `expr/constructors.rs` | `check_struct_lit`、`check_constructor_call`、`check_explicit_enum_constructor_call`、Error 引数と value 検査 |
| `predeclare.rs` | `predeclare/mod.rs` | 宣言登録の入口。登録順序は `program.rs` が維持 |
| `predeclare.rs` | `predeclare/nominal.rs`、`predeclare/functions.rs`、`predeclare/traits.rs` | 各 `predeclare_*` と関連 helper。where、constructor mapping、parent coverage は Trait 宣言群と一緒に移す |
| `predeclare.rs` | `trait_selection.rs` 内の責務対応先 | `compiler_trait_impl_exists`、`compiler_trait_dispatch_target`、`trait_impl_exists_for_args`、`rigid_tyvar_entails_trait`、`trait_bound_entails`。宣言時だけの処理と誤認させない |
| `specialize.rs` | `specialize/mod.rs` | `specialize_program`、`ensure_specialized_def`、worklist と具体化 cache |
| `specialize.rs` | `specialize/rewrite.rs` | `rewrite_*specializations*`、`specialize_callable_reference` |
| `specialize.rs` | `specialize/substitute.rs` | `substitute_typed_*_with_mapping`、`substitute_ty_with_mapping`、`substitute_selected_dispatch` |
| `specialize.rs` | `specialize/patterns.rs` | `concretize_specialized_*pattern`、pin / extractor の dispatch、payload 検証 |
| `expr.rs` と `specialize.rs` | `audit.rs` | `first_pending_trait_helper`、`unresolved_executable_constructor_application`、`first_unresolved_executable_type_argument` と、それぞれの終了条件を検査する helper |

`specialize` は現存の module 名として初期段階では保持する。文書上は callable instantiation と呼ぶ。言語に impl specialization を加える意味ではない。

`canonical_ty_key` と `CanonicalTyKey` は候補証明の identity と具体化 cache の双方から参照されるため、最終的には `identity.rs` に同居させる候補になる。表示名に置き換える案は採用しない。`trait_selection.rs` 自体は、まず内部テストを `trait_selection/tests.rs` へ移して製品処理の境界を見えるようにし、canonical 変換 / unifier / 証明の更なる分割はその後に判断する。

## 状態の所有者と保存範囲

現行 `Checker` は永続状態と式・scope ごとの状態を同じ struct に持つ。これを一律の `Context` や `Arc<Mutex<_>>` に包む案は採用しない。以下の境界が異なるためである。

| 境界 | 現行の所有者・処理 | 保持すべき条件 |
|---|---|---|
| REPL / session 間 | `ScarSession.state: PersistentCheckerState` と `process_specs` | callable / Trait metadata、型定義、具体化 cache、Facet・Lazy・capability の binding を次回へ渡す |
| transactional typecheck | `typecheck_*with_context_with_warnings` | 状態の clone から検査し、成功時だけ session を更新。失敗時に一部登録しない |
| one-shot typecheck | `typecheck_*in_place_with_context` | 永続状態を移動して検査。失敗時は部分状態が残るという既存の契約を維持 |
| 通常関数・closure・match の scope | `push_var_scope` / `pop_var_scope` と各入口の状態保存・復元 | 既存の scope 化を維持。binding の復元と、推論・進行状態の反映を区別する |
| Error 宣言と Trait requirement の probe | `spawn_child_checker`、Error 宣言側の `absorb_child_progress` | 残る child 作成の実経路。何を clone し、何を取り込むかを個別に保持。子の全フィールドで親を上書きしない |
| 候補試行 | `CandidateProbeCheckpoint` / `rollback_candidate_probe` | 型環境、substitution、bounds、pending obligation、capability、Lazy 由来、warning、operator 由来など列挙済みの対象を戻す |
| 型関係だけの試行 | `relations.rs` の `with_type_relation_probe` | 型関係専用の復元範囲を、式候補用 checkpoint と混同しない |
| top-level 文 | `check_program` | 各文の開始で `substitutions` と `safe_operator_results` を消去し、兄弟文への推論・診断由来の漏れを防ぐ |
| 具体化 pass | `direct_pattern_requirement_tyvars` | pass ごとに作り直し、session に永続化しない |
| constructor mapping 解決 | `ConstructorMappingResolution` | pending / resolving / requests は解決作業の一時状態。失敗・循環後に再利用状態を汚さない |

`PersistentCheckerState` の保存対象は `Checker` の単なる subset であり、`CandidateProbeCheckpoint` とは同じではない。field を追加する実装では「session に保存するか」「child に渡すか」「child から取り込むか」「候補失敗で戻すか」を同時に確認する。

現行の `SpecializableDefinitions` / `TraitImplementations` / `TraitDefinitions` / `CallableSignatures` は値を `Arc` で共有する map である。これを既に「metadata が深く clone される設計」と決めつけない。map 自体の clone と `Arc` の clone、`Arc::make_mut` による更新分離を区別する。読み取り共有を増やす場合も saved branch / checkpoint への書込み漏れを拒否条件にする。

## 依存方向と可視性

目標とする責務の流れは次のとおり。現在のメソッド呼出し全体が非循環であるという意味ではない。式の再帰や候補試行から型関係への呼出しは残る。

```text
public typecheck / ScarSession
  -> program（順序、commit 境界、完了時監査）
      -> predeclare / definitions / expr
          -> signatures / trait_selection / provenance / inference
      -> specialize（具体化、rewrite、substitute）
      -> audit

bootstrap -> TypeEnv / Sindr metadata
profile   -> 計測値だけ
state     -> 型検査状態の生成・保存・復元だけ
```

型構文の入口が式を再検査したり、表示名の整形が候補を選択したりする依存を新設しない。`TypeEnv`、`Ty`、Typed IR は現在の公開表現を保持し、分割のためだけに Sigil や Forge の内部を参照しない。

可視性は module 内 private を基本とする。`expr` の兄弟子 module 間で使う項目は `pub(super)`、`checker` の別子 module からも必要なものは `pub(in crate::checker)` に限定する。既に公開されている API は `checker` から明示的に re-export し、下位 module の全公開で辻褄を合わせない。`use super::*` の全面除去と移動を同時に行わず、境界が確定した module から必要 import を明示する。

状態型を別 module に移す場合は、Rust の private field へのアクセス範囲が変わる点に注意する。保存のためだけに全 field を `pub(crate)` にするのではなく、`session` / `state` 間で必要な変換と可視性を限定する。

## 実施順序と受入条件

1. **現在の契約を固定する。** 成功・拒否境界、IR・metadata、rollback を対象にテストを増やす。既存テストの単なる重複や、既存の成功を壊して Red を作る作業はしない。
2. **独立しやすい module を移す。** profile、bootstrap、session の index rewrite、内部テストを先に移す。既存 API の import path と checkpoint roundtrip を確認する。
3. **expr の意味ごとの単位を移す。** process、Facet、literal、closure、通常/Trait 呼出しの順に独立した変更とする。最終的に入口の分岐と期待型伝達を `expr/mod.rs` へ残す。
4. **宣言・型・具体化を移す。** 型構文と inference、宣言登録と本体、具体化と IR 置換を分離する。Pattern、Facet 内式、Trait origin、DoSafeBind の metadata を各走査が保持することを確認する。
5. **状態と性能を改善する。** 移動済みコードの計測から clone / scan / 具体化回数の支配箇所を一つ選ぶ。状態の保存範囲を変更するものは、ファイル移動と別の差分にする。

各段階の受入条件:

- 元ファイルから実装を除去し、同じ責務の新旧実装や迂回経路が残っていない。
- 受理/拒否、診断 reason・primary span・source origin、warning の順序、typed output の契約を維持する。
- public API、callable identity、関数 index の再調整、checkpoint の保存・復元契約を維持する。
- 通常呼出しの成功で候補 checkpoint を不要に作らない。既存の `ordinary_successful_applications_do_not_create_candidate_probe_checkpoints` がこの条件を検査している。
- candidate rollback と session rollback を混同せず、失敗後の再検査、別 session、saved checkpoint が独立する。
- 走査を移した場合、`DoSafeBind`、Pattern 内 dispatch、Facet の segment 内式、Trait instantiation / call origin を欠落させない。
- 変更箇所の focused test と `rtk cargo nextest run -p scar` を確認し、fixture / REPL に影響する段階はそれぞれの境界も確認する。最終統合前に `rtk cargo nextest run --profile ci --workspace` を一度実行し、結果を記録する。新しい失敗やコード修正がなければ同じ全体検証を繰り返さない。
- `.srt` の所要時間は同条件の before / after で別途評価する。ファイル分割の完了を速度改善の証拠としない。

## 避ける分割

- 行数だけで `expr_1.rs` / `expr_2.rs` に割る。同じ契約の変更箇所が分散したままになる。
- `helpers.rs` / `utils.rs` に型構文・推論・診断・IR 走査を集める。責務の曖昧さを移すだけになる。
- 型変数の探索・置換・index rewrite・完了時監査を、最初から一つの汎用 visitor にする。各走査で generic declaration、metadata、Pattern の扱いが異なる。先に走査契約を列挙し、完全に共通の子列挙だけを後で検討する。
- 分割と同時に whole-program の pass 順序を変える。前方参照、Trait mapping、診断の優先順位が変わる恐れがある。
- 名前が近いという理由で `check_constructor_call` と TypeCtorTrait の constructor projection を統合する。値の constructor と型レベルの constructor は別の契約である。
- `relations.rs` を再び `types.rs` に吸収する。構造化診断と型関係の適用位置という既存の境界を失う。
- 将来の高速化を想定して公開 trait / plugin interface / 複数 checker backend を新設する。現行の一つの型検査経路を小さな責務に分ける範囲を守る。

## 実施記録（2026-10-16）

開始時の HEAD は `189702c5ed6e15e522c518756c4c33ed2bdc9dfa`。level2 の実装として、3つのサブエージェントが式、session・状態、宣言・型・具体化を分担し、担当外の領域を相互レビューした。既に追加済みの T-01〜T-12 と既存テストを使い、成功動作を壊す Red や重複した回帰テストは追加していない。

| 領域 | 実施後の配置 |
|---|---|
| 入口・session | `checker/mod.rs` は module 宣言・再 export・共通 metadata 型。公開 API、checkpoint、永続状態は `session.rs`、関数 index 再調整は `session/function_indices.rs` |
| 状態・実行順序 | `state.rs` に構築・child・永続化、`program.rs` に既存 pass 順序、`process.rs` に宣言契約、`warnings.rs` に warning 収集を移動 |
| 初期化・計測 | `bootstrap.rs` と `profile.rs` に型・関数を移動。builtin metadata、計測形式・有効化条件を維持 |
| 式 | `expr/mod.rs` に6つの Checker 入口を残し、calls / arguments / trait_calls / operators / process_calls / closures / literals へ移動。binding・分岐は `control.rs`、候補保存・復元は `probes.rs`、Extractor 適用は `extractors.rs` |
| Facet・Lazy・do | Facet の入力・path・update を `expr/facet/{mod,path,update}.rs` へ移動。依存型判定も Facet 側へ配置。Lazy と SafeBind は既存 `lazy_calls.rs` / `do_block.rs` に統合 |
| 型 | `type_syntax.rs` / `inference.rs` / `resolve_typed.rs` / `type_display.rs` に分離。型スロット検査は `type_syntax.rs` |
| 宣言 | `definitions/{builtins,functions,traits,nominal}.rs`、`predeclare/{mod,functions,traits,nominal}.rs` へ移動。値の constructor は `expr/constructors.rs`、候補選択 helper は `trait_selection.rs` |
| 具体化・監査 | `specialize/{mod,rewrite,substitute,patterns}.rs` に分離し、終了時監査は `audit.rs` に集約 |
| 内部テスト | expr、session、trait_selection の専用ファイルへ移動。bootstrap / process などの小さなテストは対象 module 内に配置 |

分割直前は25ファイル・62,418行、分割後は65ファイル・62,737行。`checker/mod.rs` は5,426行から553行、`expr.rs` は16,563行から入口の `expr/mod.rs` 1,018行と各責務へ分かれた。行数には空行・コメント・内部テストを含む。

元の `expr.rs` / `types.rs` / `definitions.rs` / `predeclare.rs` / `specialize.rs` と移動前の session 契約テストは削除した。委譲 wrapper は追加していない。`CanonicalTyKey` と `canonical_ty_key` の追加分離は候補のままとし、候補証明や cache identity の構成を変えていない。

`CallableInstantiationKey` は `checker` 直下に残し、他の公開 API は明示的に再 export した。外部 crate から元の13公開項目を import する smoke check も成功した。Checker と永続状態の field は必要な checker 内範囲に限定した。`ScarSession` は既存内部テストが参照する `state` だけを checker 内に公開し、他の field は private のまま保持した。

可視性、移動した参照先パス、整形を正規化し、移動前後の Rust 構文木で関数・型・定数・static・型 alias を比較した。元の1,454定義に追加・欠落はなく、正規化後の内容は一致した。比較用に展開した入れ子関数を再走査する確認では1,497定義・1,441キーが一致した。比較用ツールは `/tmp` に置き、製品やテストの依存には追加していない。

相互レビューで不要な可視性拡大、Facet helper の配置、移動時の doc コメント欠落を修正した。Pattern 内 dispatch、Facet segment 内式、Trait origin / instantiation、DoSafeBind の走査と保存範囲を維持していることを最終配置で確認した。

検証結果。

| コマンド・確認 | 結果 |
|---|---|
| 分割前の `rtk cargo nextest run -p scar` | 404 passed / 39 binaries |
| 最終変更後の `cargo check -p scar --tests` | 成功、警告なし |
| 最終変更後の `rtk cargo nextest run -p scar` | 404 passed / 39 binaries |
| `rtk cargo nextest run --profile ci --workspace` | 2,622 passed / 70 binaries。cold CLI、fixture、REPL を含む全件成功 |
| `cargo fmt --all -- --check` | 成功 |
| `git diff --check`、変更した文書の相対リンク | 成功、相対リンク24件 |

標準ソース・API・診断仕様・schema / VM version は変更していない。`cargo run -- test --quiet --all` とコンパイル性能の反復測定は未実施。clone / scan / 具体化回数を減らす段階と、状態所有者の再設計は別作業として残す。
