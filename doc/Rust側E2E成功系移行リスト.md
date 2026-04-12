# Rust側E2E成功系移行リスト

> 目的: `tests/integration` に残っている「純粋 Surtr コードの成功系」を `tests/spec/**.srt + .expected` へ寄せ、Rust 側 integration を CLI / artifact / runtime 観測 / stderr 契約に絞る。

## 1. 判定基準

### 1.1 `spec` へ移す条件

- 入力が純粋な Surtr source だけで完結する
- 判定が `stdout` 一致だけで済む
- `stderr` 観測、`VmObservation`、temp dir、CLI 引数、`.eldr` ファイル、JSON 出力を必要としない
- 1 source file の成功ケースとして表現できる

### 1.2 Rust integration に残す条件

- `VmObservation` を使う
- `stdout` ではなく `stderr` の内容や順序を見る
- REPL / `surtr test` / `build` / `dump` / `run` の CLI 契約を見る
- `.eldr` の encode / decode / roundtrip を見る
- multi-source module fixture や staged loader を使う

## 2. 現在の対象

- 主対象: `tests/integration/language_features/*.rs`
- 本リストは「成功系」の移行計画のみ扱う
- `assert_compile_error(...)` のケースは別途 `tests/compile_errors/**` への移行対象として扱う

## 3. 優先度A: 既存 spec と重複が強いもの

下記は既存 `tests/spec/**` と役割がかなり近い。まず内容差分を吸収してから Rust 側を削る。

### 3.1 `tests/spec/types` に寄せる

- `core_language::bindings_basic_print`
  - 近い既存: `tests/spec/types/basic_bind_and_print.srt`
- `core_language::bindings_shadowing_last_wins`
  - 近い既存: `tests/spec/types/shadowing_bindings.srt`
- `core_language::annotations_accept_matching_types`
  - 近い既存: `tests/spec/types/annotated_bindings.srt`
- `core_language::primitives_render_to_string`
  - 近い既存: `tests/spec/types/primitives.srt`
- `core_language::int_negative_literal`
  - 近い既存: `tests/spec/types/negative_int_literal.srt`
- `core_language::list_literal_int`
- `core_language::list_literal_string`
- `core_language::list_empty_with_annotation`
- `core_language::list_cons_expr`
  - 近い既存: `tests/spec/types/list_literals.srt`

### 3.2 `tests/spec/operators` / `tests/spec/stdmod` に寄せる

- `core_language::arithmetic_int_ops`
- `core_language::arithmetic_float_ops`
- `core_language::comparison_int_ops`
- `core_language::equality_string`
- `core_language::inequality_boolean`
- `core_language::concat_strings`
  - 近い既存: `tests/spec/operators/arithmetic_comparison_concat.srt`
- `core_language::arithmetic_precedence`
- `core_language::expr_class_operators_are_same_precedence`
  - 近い既存: `tests/spec/operators/precedence_basic.srt`
- `core_language::func_literal_infix_invocation_works`
- `core_language::closure_literal_invocation`
  - 近い既存: `tests/spec/operators/func_literal_basic.srt`
- `core_language::kernel_eq_neq_helpers_match_operator_behavior`
- `core_language::kernel_ordering_and_concat_helpers_match_operator_behavior`
- `core_language::safe_xxx_zero_returns_zero_division_error_display`
  - 近い既存: `tests/spec/stdmod/kernel_logic_compare_helpers.srt`
  - 近い既存: `tests/spec/stdmod/int_boolean_float_helpers.srt`
- `core_language::closure_builtin_capture`
  - `stdmod` か `operators` へ統合候補

### 3.3 `tests/spec/functions` に寄せる

- `core_language::function_forward_reference_succeeds`
  - 近い既存: `tests/spec/functions/forward_ref_def.srt`
- `core_language::function_named_args_reordered`
  - 近い既存: `tests/spec/functions/named_args_basic.srt`
- `core_language::function_definition_minimal`
  - 近い既存: `tests/spec/functions/def_basic.srt`
- `core_language::function_call_locals_are_isolated`
- `core_language::function_zero_arg_call`
- `core_language::function_partial_application_composition`
- `core_language::closure_argument_type_infers_from_add_constraint`

### 3.4 `tests/spec/control` に寄せる

- `core_language::if_expression_with_else`
  - 近い既存: `tests/spec/control/if_expression_value.srt`
- `core_language::if_expression_without_else_returns_unit`
  - 近い既存: `tests/spec/control/if_two_arg_returns_unit.srt`
- `core_language::match_boolean_exhaustive`
- `core_language::match_result_exhaustive`
  - 近い既存: `tests/spec/control/if_match_basics.srt`
- `core_language::match_boolean_wildcard_arm`
- `core_language::match_int_literal_patterns`
- `core_language::match_string_literal_patterns`
- `core_language::match_list_patterns`
  - 近い既存: `tests/spec/control/match_extensions.srt`
- `core_language::cond_selects_first_true_branch_and_skips_later_branches`
- `core_language::cond_allows_block_bodies`
  - 近い既存: `tests/spec/control/cond_expression_value.srt`
- `core_language::kernel_and_or_short_circuit`
  - `control` か `stdmod` に寄せる候補

### 3.5 `tests/spec/strings` に寄せる

- `core_language::string_interpolation_basic`
  - 近い既存: `tests/spec/strings/interpolation_basic.srt`

### 3.6 `tests/spec/usecases` に寄せる

- `pipelines_and_usecases::result_pipeline_usecase_user_lookup_and_render`
- `pipelines_and_usecases::kernel_helper_usecase_works_with_funcliteral_and_flow_ops`
- `pipelines_and_usecases::safebind_usecase_result_and_list_pipeline`
  - 近い既存: `tests/spec/usecases/safebind_list_processing.srt`
- `pipelines_and_usecases::list_pipeline_usecase_expand_and_present_keywords`
  - 近い既存: `tests/spec/usecases/list_keyword_expansion.srt`
- `pipelines_and_usecases::language_goal_combined`
  - `tests/spec/smoke/basic_roundtrip_program.srt` と統合整理候補

## 4. 優先度B: 新規 spec を追加してから Rust 側を削るもの

下記は純粋成功系だが、現状の `tests/spec/**` に対応 fixture が薄い。`spec` を足してから Rust 側を削る。

### 4.1 `tests/spec/data` を新設または拡充

- `core_language::struct_definition_and_field_access`
- `core_language::record_constructor_positional`
- `core_language::record_constructor_named_args`
- `core_language::struct_record_forward_references_and_type_annotation_succeed`
- `core_language::struct_property_update_via_associated_functions`
- `core_language::enum_state_transition_via_associated_functions`

### 4.2 `tests/spec/operators` を拡充

- `pipelines_and_usecases::pipe_accepts_capture_and_injected_call`
- `pipelines_and_usecases::pipe_accepts_qualified_capture_and_injected_call`
- `pipelines_and_usecases::result_pipeline_map_and_bind_work`
- `pipelines_and_usecases::result_pipeline_injects_left_value_into_call_rhs`
- `pipelines_and_usecases::list_pipeline_helpers_and_compose_work`
- `pipelines_and_usecases::compose_builds_callable_from_capture_only`

### 4.3 `tests/spec/errors` を拡充

- `safebind_and_errors::deferror_no_args_basic`
- `safebind_and_errors::deferror_forward_reference_in_result_signature_succeeds`
- `safebind_and_errors::result_ok_case_prints_value`
- `safebind_and_errors::result_helpers_render_multiline_cause_trees`

### 4.4 `tests/spec/control` / `tests/spec/usecases` を拡充

- `safebind_and_errors::safebind_top_level_ok`
- `safebind_and_errors::safebind_list_pattern_ok`
- `safebind_and_errors::safebind_list_pattern_plain_list_ok`
- `safebind_and_errors::safebind_uncons_string_ok`
- `safebind_and_errors::safebind_string_pattern_plain_string_ok`
- `safebind_and_errors::safebind_string_pattern_handles_multibyte_chars`
- `safebind_and_errors::match_string_empty_and_uncons_is_exhaustive`
- `safebind_and_errors::safebind_list_pattern_with_nested_constructor_literals_ok`
- `safebind_and_errors::safebind_list_pattern_with_nested_constructor_and_tail_ok`
- `safebind_and_errors::safebind_top_ok_pattern_allows_nested_result`
- `safebind_and_errors::safebind_allows_total_plain_rhs`

## 5. Rust integration に残すもの

### 5.1 runtime 観測なので残す

- `runtime_observation::tail_recursive_function_reuses_single_non_top_level_frame`
- `runtime_observation::match_arm_tail_calls_are_optimized`
- `runtime_observation::mutual_tail_recursion_is_optimized`
- `runtime_observation::non_tail_recursion_keeps_growing_frames`

### 5.2 `stderr` 契約を見ているので残す

- `safebind_and_errors::safebind_list_pattern_plain_list_empty_propagates_empty_list`
- `safebind_and_errors::safebind_string_pattern_empty_propagates_pattern_mismatch`
- `safebind_and_errors::safebind_fixed_list_pattern_reports_index_out_of_bounds_for_longer_rhs`
- `safebind_and_errors::safebind_fixed_list_pattern_reports_index_out_of_bounds_for_shorter_rhs`
- `safebind_and_errors::safebind_nested_result_err_propagates`
- `safebind_and_errors::safebind_list_pattern_empty_propagates_empty_list`
- `safebind_and_errors::safebind_function_early_return_on_err`
- `safebind_and_errors::safebind_script_error_eprints`
- `safebind_and_errors::builtin_prelude_provides_none_error`
- `safebind_and_errors::builtin_safe_xxx_zero_error_can_be_matched_and_eprinted`
- `safebind_and_errors::deferror_interpolated_message_display`
- `safebind_and_errors::match_err_eprint_with_wildcard_arm`
- `safebind_and_errors::eprint_renders_linear_cause_chain_lines`

### 5.3 `tests/integration` の別ファイルで残すべきもの

- `build_roundtrip.rs`
  - `build` / `dump` / `.eldr` / JSON 契約
- `run_eldr.rs`
  - `.srt` と `.eldr` の roundtrip、および runtime diagnostics の位置情報
- `repl.rs`
  - 対話契約
- `test_command.rs`
  - `surtr test` CLI 契約
- `module_import_fixtures.rs`
  - multi-source module / staged loader 契約

## 6. 実行順序

1. 優先度Aの重複ケースを `tests/spec/**` に統合する
2. Rust 側の同等成功系を削除する
3. 優先度Bの新規 fixture を `tests/spec/**` に追加する
4. `language_features` には runtime 観測と `stderr` 契約だけを残す
5. 成功系の削減後に `cargo nextest run -p rune --test run_srt` を主系に据える

## 7. 削減後の理想像

- `tests/spec/**`
  - 純粋 Surtr 成功系の正本
- `tests/compile_errors/**`
  - 純粋 Surtr 失敗系の正本
- `tests/integration/language_features/**`
  - runtime 観測
  - `stderr` 契約
- `tests/integration/*.rs`
  - CLI / artifact / REPL / module loader 契約

## 8. 備考

- `tests/integration/language_features` に残っている `assert_compile_error(...)` も、本来は `tests/compile_errors/**` に寄せる余地がある
- ただし今回の優先順位は「Rust 側 E2E 成功系を減らすこと」なので、本リストでは成功系を先に切り出す
