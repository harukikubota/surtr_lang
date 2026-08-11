use scar::typed::{
    OperatorTraitOp, TraitCallOrigin, TypedFacetPathKind, TypedFacetSegment, TypedInner, TypedNode,
    TypedPattern, TypedProgram, TypedWhereConstraintRhs,
};
use scar::types::Ty;
use sigil::resolved::{
    Resolved, ResolvedFacetBracketExpr, ResolvedFacetPathSegment, ResolvedHashMapLiteralEntry,
    ResolvedId, ResolvedPattern,
};
use sindr::policy::{EntryPoint, ExitCodePolicy, RuntimeSourcePolicy};
use sindr::primitives::int;
use spire::ast::{Lit, Span};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

mod support;

use support::*;

const PROCESS_MODULE_SOURCE: &str = include_str!("../../../lib/process.srt");

const SURFACE_WORKER_COUNT: usize = 1;
const SURFACE_BUCKET_COUNT: usize = 32;

const SURFACE_CASES: &[(&str, fn())] = &[
    (
        "process_stdlib_no_longer_declares_task_hidden_lower_helpers",
        process_stdlib_no_longer_declares_task_hidden_lower_helpers as fn(),
    ),
    (
        "process_stdlib_declares_common_process_family_modules",
        process_stdlib_declares_common_process_family_modules as fn(),
    ),
    (
        "process_module_only_declares_public_runtime_helpers",
        process_module_only_declares_public_runtime_helpers as fn(),
    ),
    (
        "process_stdlib_declares_agent_lower_surface_with_regular_surface_docs",
        process_stdlib_declares_agent_lower_surface_with_regular_surface_docs as fn(),
    ),
    (
        "field_access_is_resolved_to_numeric_index",
        field_access_is_resolved_to_numeric_index as fn(),
    ),
    (
        "match_bool_requires_exhaustive_arms",
        match_bool_requires_exhaustive_arms as fn(),
    ),
    (
        "match_bool_accepts_qualified_boolean_constructor_patterns",
        match_bool_accepts_qualified_boolean_constructor_patterns as fn(),
    ),
    (
        "match_bool_qualified_constructor_patterns_require_exhaustive_arms",
        match_bool_qualified_constructor_patterns_require_exhaustive_arms as fn(),
    ),
    (
        "safebind_total_pattern_accepts_plain_rhs",
        safebind_total_pattern_accepts_plain_rhs as fn(),
    ),
    (
        "dbg_special_form_typechecks_to_unit",
        dbg_special_form_typechecks_to_unit as fn(),
    ),
    (
        "safebind_function_requires_result_return_type",
        safebind_function_requires_result_return_type as fn(),
    ),
    (
        "safebind_result_closure_uses_nearest_callable_return_type",
        safebind_result_closure_uses_nearest_callable_return_type as fn(),
    ),
    (
        "safebind_non_result_closure_is_rejected",
        safebind_non_result_closure_is_rejected as fn(),
    ),
    (
        "safebind_result_returning_annotated_closure_allows_safebind",
        safebind_result_returning_annotated_closure_allows_safebind as fn(),
    ),
    (
        "safebind_non_result_closure_rejects_safebind",
        safebind_non_result_closure_rejects_safebind as fn(),
    ),
    (
        "safebind_top_ok_pattern_requires_nested_result_rhs",
        safebind_top_ok_pattern_requires_nested_result_rhs as fn(),
    ),
    (
        "safebind_top_ok_pattern_accepts_nested_result_rhs",
        safebind_top_ok_pattern_accepts_nested_result_rhs as fn(),
    ),
    (
        "safebind_list_pattern_accepts_plain_list_rhs",
        safebind_list_pattern_accepts_plain_list_rhs as fn(),
    ),
    (
        "safebind_string_pattern_accepts_plain_string_rhs",
        safebind_string_pattern_accepts_plain_string_rhs as fn(),
    ),
    (
        "int_range_literal_typechecks_to_list_int",
        int_range_literal_typechecks_to_list_int as fn(),
    ),
    (
        "string_range_literal_typechecks_to_result_list_string",
        string_range_literal_typechecks_to_result_list_string as fn(),
    ),
    (
        "hash_map_literal_typechecks_string_key_expressions",
        hash_map_literal_typechecks_string_key_expressions as fn(),
    ),
    (
        "hash_map_literal_rejects_non_string_keys",
        hash_map_literal_rejects_non_string_keys as fn(),
    ),
    (
        "hash_map_literal_rejects_mixed_value_types",
        hash_map_literal_rejects_mixed_value_types as fn(),
    ),
    (
        "match_string_requires_empty_and_uncons_arms_for_exhaustiveness",
        match_string_requires_empty_and_uncons_arms_for_exhaustiveness as fn(),
    ),
    (
        "match_string_accepts_empty_and_uncons_arms",
        match_string_accepts_empty_and_uncons_arms as fn(),
    ),
    (
        "safebind_list_pattern_accepts_nested_constructor_literals",
        safebind_list_pattern_accepts_nested_constructor_literals as fn(),
    ),
    (
        "tuple_literal_and_field_access_typecheck",
        tuple_literal_and_field_access_typecheck as fn(),
    ),
    (
        "tuple_bind_pattern_typechecks",
        tuple_bind_pattern_typechecks as fn(),
    ),
    (
        "facet_view_on_plain_value_returns_plain_focus",
        facet_view_on_plain_value_returns_plain_focus as fn(),
    ),
    (
        "facet_view_on_result_value_returns_result_focus",
        facet_view_on_result_value_returns_result_focus as fn(),
    ),
    (
        "facet_variant_selector_returns_result_and_requires_pascal_case",
        facet_variant_selector_returns_result_and_requires_pascal_case as fn(),
    ),
    (
        "facet_preview_requires_variant_path_and_records_path_kind",
        facet_preview_requires_variant_path_and_records_path_kind as fn(),
    ),
    (
        "facet_preview_accepts_option_variant",
        facet_preview_accepts_option_variant as fn(),
    ),
    (
        "facet_boolean_selector_uses_regular_enum_diagnostic",
        facet_boolean_selector_uses_regular_enum_diagnostic as fn(),
    ),
    (
        "facet_list_and_map_segments_are_fallible_structural_paths",
        facet_list_and_map_segments_are_fallible_structural_paths as fn(),
    ),
    (
        "facet_explicit_container_root_captures_use_expected_function_context",
        facet_explicit_container_root_captures_use_expected_function_context as fn(),
    ),
    (
        "facet_dynamic_container_segments_accept_runtime_expressions",
        facet_dynamic_container_segments_accept_runtime_expressions as fn(),
    ),
    (
        "facet_dynamic_container_segments_reject_result_and_wrong_key_types",
        facet_dynamic_container_segments_reject_result_and_wrong_key_types as fn(),
    ),
    (
        "facet_negative_list_index_and_range_segments_typecheck",
        facet_negative_list_index_and_range_segments_typecheck as fn(),
    ),
    (
        "facet_range_segments_require_plain_int_endpoints_and_list_values",
        facet_range_segments_require_plain_int_endpoints_and_list_values as fn(),
    ),
    (
        "facet_const_dynamic_container_segments_require_literals",
        facet_const_dynamic_container_segments_require_literals as fn(),
    ),
    (
        "facet_optional_marker_rejected_on_non_enum_segment",
        facet_optional_marker_rejected_on_non_enum_segment as fn(),
    ),
    (
        "facet_case_api_requires_enum_path_and_records_modes",
        facet_case_api_requires_enum_path_and_records_modes as fn(),
    ),
    (
        "facet_surface_resolves_after_facet_rename",
        facet_surface_resolves_after_facet_rename as fn(),
    ),
    (
        "facet_chain_typecheck_success_and_mismatch",
        facet_chain_typecheck_success_and_mismatch as fn(),
    ),
    (
        "facet_slash_compose_typecheck_success_and_mismatch",
        facet_slash_compose_typecheck_success_and_mismatch as fn(),
    ),
    (
        "facet_set_returns_result_source",
        facet_set_returns_result_source as fn(),
    ),
    (
        "facet_put_returns_plain_source",
        facet_put_returns_plain_source as fn(),
    ),
    (
        "facet_put_rejects_result_source_and_variant_path",
        facet_put_rejects_result_source_and_variant_path as fn(),
    ),
    (
        "facet_put_supports_same_type_tuple_update_inside_annotated_closure",
        facet_put_supports_same_type_tuple_update_inside_annotated_closure as fn(),
    ),
    (
        "facet_put_unannotated_closure_still_lacks_tuple_context_from_expected_return",
        facet_put_unannotated_closure_still_lacks_tuple_context_from_expected_return as fn(),
    ),
    (
        "facet_put_supports_type_changing_tuple_update",
        facet_put_supports_type_changing_tuple_update as fn(),
    ),
    (
        "facet_put_rebuilds_unique_generic_named_type",
        facet_put_rebuilds_unique_generic_named_type as fn(),
    ),
    (
        "facet_put_rejects_repeated_generic_named_type",
        facet_put_rejects_repeated_generic_named_type as fn(),
    ),
    (
        "facet_case_set_rebuilds_unique_generic_enum",
        facet_case_set_rebuilds_unique_generic_enum as fn(),
    ),
    (
        "facet_put_rejects_result_annotation_context",
        facet_put_rejects_result_annotation_context as fn(),
    ),
    (
        "facet_put_rejects_result_return_context",
        facet_put_rejects_result_return_context as fn(),
    ),
    (
        "facet_over_requires_unary_result_callable",
        facet_over_requires_unary_result_callable as fn(),
    ),
    (
        "optional_type_annotation_matches_option",
        optional_type_annotation_matches_option as fn(),
    ),
    (
        "optional_type_annotation_rejects_result_value",
        optional_type_annotation_rejects_result_value as fn(),
    ),
    (
        "facet_set_rejects_plain_value_for_result_focus",
        facet_set_rejects_plain_value_for_result_focus as fn(),
    ),
    (
        "facet_shorthand_view_and_mutation_forms_typecheck",
        facet_shorthand_view_and_mutation_forms_typecheck as fn(),
    ),
    (
        "facet_shorthand_reuses_existing_facet_api_errors",
        facet_shorthand_reuses_existing_facet_api_errors as fn(),
    ),
    (
        "facet_shorthand_misuse_is_rejected_outside_facet_api",
        facet_shorthand_misuse_is_rejected_outside_facet_api as fn(),
    ),
    (
        "facet_over_accepts_success_updater_for_result_focus",
        facet_over_accepts_success_updater_for_result_focus as fn(),
    ),
    (
        "facet_over_allows_result_typed_payload_replacement",
        facet_over_allows_result_typed_payload_replacement as fn(),
    ),
    (
        "facet_over_result_requires_result_container_updater",
        facet_over_result_requires_result_container_updater as fn(),
    ),
    (
        "readonly_facet_view_succeeds_and_preserves_path_metadata",
        readonly_facet_view_succeeds_and_preserves_path_metadata as fn(),
    ),
    (
        "readonly_field_blocks_deep_mutation_but_owner_can_replace_property",
        readonly_field_blocks_deep_mutation_but_owner_can_replace_property as fn(),
    ),
    (
        "readonly_struct_root_blocks_mutating_facet_even_for_owner",
        readonly_struct_root_blocks_mutating_facet_even_for_owner as fn(),
    ),
    (
        "facet_standalone_tuple_root_is_rejected",
        facet_standalone_tuple_root_is_rejected as fn(),
    ),
    (
        "facet_bindings_can_be_reused_by_facet_intrinsics",
        facet_bindings_can_be_reused_by_facet_intrinsics as fn(),
    ),
    (
        "facet_tuple_type_root_view_works_with_expected_context",
        facet_tuple_type_root_view_works_with_expected_context as fn(),
    ),
    (
        "deferred_tuple_facet_binding_can_be_reused_by_facet_intrinsics",
        deferred_tuple_facet_binding_can_be_reused_by_facet_intrinsics as fn(),
    ),
    (
        "deferred_tuple_facet_binding_can_compose_before_consumption",
        deferred_tuple_facet_binding_can_compose_before_consumption as fn(),
    ),
    (
        "facet_tuple_type_root_compose_works_as_inner_path",
        facet_tuple_type_root_compose_works_as_inner_path as fn(),
    ),
    (
        "facet_tuple_type_root_slash_compose_works_as_inner_path",
        facet_tuple_type_root_slash_compose_works_as_inner_path as fn(),
    ),
    (
        "facet_const_slash_compose_allows_facet_consts",
        facet_const_slash_compose_allows_facet_consts as fn(),
    ),
    (
        "facet_const_slash_compose_rejects_non_facet_const_refs",
        facet_const_slash_compose_rejects_non_facet_const_refs as fn(),
    ),
    (
        "slash_operator_rejects_numeric_division_and_points_to_safe_div",
        slash_operator_rejects_numeric_division_and_points_to_safe_div as fn(),
    ),
    (
        "facet_tuple_type_root_without_context_can_bind_as_deferred_path",
        facet_tuple_type_root_without_context_can_bind_as_deferred_path as fn(),
    ),
    (
        "facet_view_inside_closure_is_allowed_for_same_scope_consumption",
        facet_view_inside_closure_is_allowed_for_same_scope_consumption as fn(),
    ),
    (
        "facet_capture_shorthand_builds_read_closure",
        facet_capture_shorthand_builds_read_closure as fn(),
    ),
    (
        "facet_values_cannot_be_embedded_in_runtime_containers",
        facet_values_cannot_be_embedded_in_runtime_containers as fn(),
    ),
    (
        "nested_facet_types_are_rejected_in_function_signatures",
        nested_facet_types_are_rejected_in_function_signatures as fn(),
    ),
    (
        "private_field_access_is_allowed_inside_owner_impl_only",
        private_field_access_is_allowed_inside_owner_impl_only as fn(),
    ),
    (
        "private_field_access_outside_owner_impl_is_rejected_for_value_and_capability_roots",
        private_field_access_outside_owner_impl_is_rejected_for_value_and_capability_roots
            as fn(),
    ),
    (
        "private_field_access_inside_closure_is_rejected_outside_owner_impl",
        private_field_access_inside_closure_is_rejected_outside_owner_impl as fn(),
    ),
    (
        "private_field_access_inside_param_closure_is_rejected_outside_owner_impl",
        private_field_access_inside_param_closure_is_rejected_outside_owner_impl as fn(),
    ),
    (
        "private_capability_root_is_rejected_in_facet_view_call",
        private_capability_root_is_rejected_in_facet_view_call as fn(),
    ),
    (
        "facet_scope_local_value_can_flow_to_closure_after_view",
        facet_scope_local_value_can_flow_to_closure_after_view as fn(),
    ),
    (
        "facet_runtime_transport_restrictions_remain",
        facet_runtime_transport_restrictions_remain as fn(),
    ),
    (
        "extractor_single_value_match_result_contract_typechecks",
        extractor_single_value_match_result_contract_typechecks as fn(),
    ),
    (
        "struct_matchblock_head_uses_attached_deconstruct_method",
        struct_matchblock_head_uses_attached_deconstruct_method as fn(),
    ),
    (
        "struct_matchblock_head_requires_attached_deconstruct_method",
        struct_matchblock_head_requires_attached_deconstruct_method as fn(),
    ),
    (
        "enum_impl_extractor_can_be_used_in_matchblock",
        enum_impl_extractor_can_be_used_in_matchblock as fn(),
    ),
    (
        "forward_struct_type_annotation_and_literal_are_allowed",
        forward_struct_type_annotation_and_literal_are_allowed as fn(),
    ),
    (
        "generic_struct_single_type_param_typechecks",
        generic_struct_single_type_param_typechecks as fn(),
    ),
    (
        "generic_struct_two_type_params_typecheck",
        generic_struct_two_type_params_typecheck as fn(),
    ),
    (
        "forward_deferror_value_can_flow_into_err",
        forward_deferror_value_can_flow_into_err as fn(),
    ),
    (
        "zero_arg_deferror_value_can_flow_into_error_parameter",
        zero_arg_deferror_value_can_flow_into_error_parameter as fn(),
    ),
    (
        "recover_kind_constructor_marker_typechecks",
        recover_kind_constructor_marker_typechecks as fn(),
    ),
    (
        "forward_reference_type_tags_are_deterministic_across_runs",
        forward_reference_type_tags_are_deterministic_across_runs as fn(),
    ),
    (
        "user_function_calls_typecheck_inside_script_module_scope",
        user_function_calls_typecheck_inside_script_module_scope as fn(),
    ),
    (
        "namespaced_type_and_trait_impl_typecheck_inside_script_module_scope",
        namespaced_type_and_trait_impl_typecheck_inside_script_module_scope as fn(),
    ),
    (
        "tuple_trait_impl_typechecks_inside_script_module_scope",
        tuple_trait_impl_typechecks_inside_script_module_scope as fn(),
    ),
    (
        "concrete_tuple_trait_impl_typechecks_inside_script_module_scope",
        concrete_tuple_trait_impl_typechecks_inside_script_module_scope as fn(),
    ),
    (
        "generic_user_function_calls_typecheck_inside_script_module_scope",
        generic_user_function_calls_typecheck_inside_script_module_scope as fn(),
    ),
    (
        "where_constraint_kinds_survive_in_typed_metadata",
        where_constraint_kinds_survive_in_typed_metadata as fn(),
    ),
    (
        "rigid_generic_return_rejects_concrete_body",
        rigid_generic_return_rejects_concrete_body as fn(),
    ),
    (
        "signature_generics_are_rigid_while_definition_body_is_checked",
        signature_generics_are_rigid_while_definition_body_is_checked as fn(),
    ),
    (
        "named_args_user_function_calls_typecheck_inside_script_module_scope",
        named_args_user_function_calls_typecheck_inside_script_module_scope as fn(),
    ),
    (
        "canonical_builtin_type_name_hole_is_reserved_for_structs",
        canonical_builtin_type_name_hole_is_reserved_for_structs as fn(),
    ),
    (
        "canonical_builtin_type_name_hole_is_reserved_for_enums",
        canonical_builtin_type_name_hole_is_reserved_for_enums as fn(),
    ),
    (
        "canonical_builtin_type_name_hole_is_reserved_for_errors",
        canonical_builtin_type_name_hole_is_reserved_for_errors as fn(),
    ),
    (
        "canonical_builtin_type_name_closure_is_reserved_for_structs",
        canonical_builtin_type_name_closure_is_reserved_for_structs as fn(),
    ),
    (
        "canonical_builtin_type_name_match_arms_is_reserved_for_structs",
        canonical_builtin_type_name_match_arms_is_reserved_for_structs as fn(),
    ),
    (
        "canonical_builtin_type_name_cond_clauses_is_reserved_for_enums",
        canonical_builtin_type_name_cond_clauses_is_reserved_for_enums as fn(),
    ),
    (
        "match_arms_type_is_forbidden_in_ordinary_user_signatures",
        match_arms_type_is_forbidden_in_ordinary_user_signatures as fn(),
    ),
    (
        "match_arms_type_is_forbidden_in_return_types",
        match_arms_type_is_forbidden_in_return_types as fn(),
    ),
    (
        "cond_clauses_type_is_forbidden_in_ordinary_user_signatures",
        cond_clauses_type_is_forbidden_in_ordinary_user_signatures as fn(),
    ),
    (
        "cond_clauses_type_is_forbidden_in_return_types",
        cond_clauses_type_is_forbidden_in_return_types as fn(),
    ),
    (
        "trailing_block_calls_typecheck_inside_script_module_scope",
        trailing_block_calls_typecheck_inside_script_module_scope as fn(),
    ),
    (
        "set_exit_code_is_allowed_in_script_rules",
        set_exit_code_is_allowed_in_script_rules as fn(),
    ),
    (
        "set_exit_code_is_forbidden_in_repl_chunk_rules",
        set_exit_code_is_forbidden_in_repl_chunk_rules as fn(),
    ),
    (
        "set_exit_code_entry_only_policy_allows_only_entrypoint_function",
        set_exit_code_entry_only_policy_allows_only_entrypoint_function as fn(),
    ),
    (
        "assert_special_form_typechecks_to_result_unit",
        assert_special_form_typechecks_to_result_unit as fn(),
    ),
    (
        "bitwidth_zero_arg_variant_reference_reuses_std_enum_constructor_uid",
        bitwidth_zero_arg_variant_reference_reuses_std_enum_constructor_uid as fn(),
    ),
    (
        "bitwidth_zero_arg_variant_typechecks_with_builtin_prelude",
        bitwidth_zero_arg_variant_typechecks_with_builtin_prelude as fn(),
    ),
    (
        "ensure_special_form_typechecks_to_result_value",
        ensure_special_form_typechecks_to_result_value as fn(),
    ),
    (
        "and_special_form_typechecks_to_boolean_if",
        and_special_form_typechecks_to_boolean_if as fn(),
    ),
    (
        "eq_helper_typechecks_as_trait_call",
        eq_helper_typechecks_as_trait_call as fn(),
    ),
    (
        "eq_helper_mismatch_uses_operator_helper_message",
        eq_helper_mismatch_uses_operator_helper_message as fn(),
    ),
    (
        "shadowed_eq_keeps_generic_call_mismatch_message",
        shadowed_eq_keeps_generic_call_mismatch_message as fn(),
    ),
    (
        "concat_helper_typechecks_as_trait_call",
        concat_helper_typechecks_as_trait_call as fn(),
    ),
    (
        "to_string_helper_typechecks_as_trait_call",
        to_string_helper_typechecks_as_trait_call as fn(),
    ),
    (
        "ensure_rejects_call_expression_predicate",
        ensure_rejects_call_expression_predicate as fn(),
    ),
    (
        "assert_rejects_non_concrete_error_expression",
        assert_rejects_non_concrete_error_expression as fn(),
    ),
    (
        "kernel_and_contract_rejects_eager_signature",
        kernel_and_contract_rejects_eager_signature as fn(),
    ),
    (
        "special_form_builtin_decl_must_live_under_kernel",
        special_form_builtin_decl_must_live_under_kernel as fn(),
    ),
    (
        "kernel_does_not_allow_removed_concat_builtin",
        kernel_does_not_allow_removed_concat_builtin as fn(),
    ),
    (
        "if_auto_forces_zero_arg_closure_once_for_branch_type",
        if_auto_forces_zero_arg_closure_once_for_branch_type as fn(),
    ),
    (
        "if_nested_closure_is_not_deep_forced",
        if_nested_closure_is_not_deep_forced as fn(),
    ),
    (
        "user_lazy_annotation_is_rejected",
        user_lazy_annotation_is_rejected as fn(),
    ),
    (
        "assert_accepts_lazy_error_branch",
        assert_accepts_lazy_error_branch as fn(),
    ),
    (
        "ensure_accepts_lazy_error_branch",
        ensure_accepts_lazy_error_branch as fn(),
    ),
    (
        "assert_accepts_existing_error_value",
        assert_accepts_existing_error_value as fn(),
    ),
    (
        "ensure_accepts_existing_error_value",
        ensure_accepts_existing_error_value as fn(),
    ),
    (
        "generic_annotation_list_int_is_accepted",
        generic_annotation_list_int_is_accepted as fn(),
    ),
    (
        "generic_def_signature_instantiates_per_call_site",
        generic_def_signature_instantiates_per_call_site as fn(),
    ),
    (
        "generic_defenum_constructor_and_match_typecheck",
        generic_defenum_constructor_and_match_typecheck as fn(),
    ),
    (
        "closure_param_annotation_without_expected_type_constrains_calls",
        closure_param_annotation_without_expected_type_constrains_calls as fn(),
    ),
    (
        "closure_application_mismatch_reports_callable_type_signature",
        closure_application_mismatch_reports_callable_type_signature as fn(),
    ),
    (
        "builtin_function_arity_reports_call_target_signature",
        builtin_function_arity_reports_call_target_signature as fn(),
    ),
    (
        "builtin_function_mismatch_reports_call_target_signature",
        builtin_function_mismatch_reports_call_target_signature as fn(),
    ),
    (
        "capture_application_mismatch_reports_callable_type_signature",
        capture_application_mismatch_reports_callable_type_signature as fn(),
    ),
    (
        "script_callable_signature_omits_file_path_segments",
        script_callable_signature_omits_file_path_segments as fn(),
    ),
    (
        "compose_mismatch_reports_left_and_right_callable_types",
        compose_mismatch_reports_left_and_right_callable_types as fn(),
    ),
    (
        "compose_accepts_calls_returning_function_values",
        compose_accepts_calls_returning_function_values as fn(),
    ),
    (
        "compose_rejects_non_function_call_results_after_typechecking_call",
        compose_rejects_non_function_call_results_after_typechecking_call as fn(),
    ),
    (
        "closure_trait_helper_binding_requires_concrete_callable_boundary",
        closure_trait_helper_binding_requires_concrete_callable_boundary as fn(),
    ),
    (
        "closure_trait_helper_binding_accepts_binding_annotation",
        closure_trait_helper_binding_accepts_binding_annotation as fn(),
    ),
    (
        "closure_trait_helper_binding_accepts_parameter_annotations",
        closure_trait_helper_binding_accepts_parameter_annotations as fn(),
    ),
    (
        "on_call_concretizes_closure_trait_helper_from_key_function",
        on_call_concretizes_closure_trait_helper_from_key_function as fn(),
    ),
    (
        "on_call_concretizes_trait_helper_capture_from_key_function",
        on_call_concretizes_trait_helper_capture_from_key_function as fn(),
    ),
    (
        "pipe_plain_apply_over_result_reports_whole_lhs_mismatch",
        pipe_plain_apply_over_result_reports_whole_lhs_mismatch as fn(),
    ),
    (
        "context_bind_rejects_plain_rhs_return",
        context_bind_rejects_plain_rhs_return as fn(),
    ),
    (
        "context_bind_rhs_closure_receives_result_return_expectation",
        context_bind_rhs_closure_receives_result_return_expectation as fn(),
    ),
    (
        "safebind_pipe_bind_closure_receives_expected_result_return",
        safebind_pipe_bind_closure_receives_expected_result_return as fn(),
    ),
    (
        "apply_and_map_rhs_closures_receive_whole_expression_return_expectation",
        apply_and_map_rhs_closures_receive_whole_expression_return_expectation as fn(),
    ),
    (
        "safebind_pipe_apply_annotated_closure_receives_expected_result_return",
        safebind_pipe_apply_annotated_closure_receives_expected_result_return as fn(),
    ),
    (
        "safebind_pipe_map_annotated_closure_receives_expected_result_return",
        safebind_pipe_map_annotated_closure_receives_expected_result_return as fn(),
    ),
    (
        "kleisli_compose_closures_receive_result_return_expectation",
        kleisli_compose_closures_receive_result_return_expectation as fn(),
    ),
    (
        "safebind_kleisli_annotated_closure_receives_expected_result_return",
        safebind_kleisli_annotated_closure_receives_expected_result_return as fn(),
    ),
    (
        "lifted_compose_rhs_closure_allows_explicit_nested_result_expectation",
        lifted_compose_rhs_closure_allows_explicit_nested_result_expectation as fn(),
    ),
    (
        "context_map_keeps_result_for_later_bind",
        context_map_keeps_result_for_later_bind as fn(),
    ),
    (
        "context_map_and_bind_lower_to_operator_trait_calls",
        context_map_and_bind_lower_to_operator_trait_calls as fn(),
    ),
    (
        "explicit_functor_call_has_explicit_origin",
        explicit_functor_call_has_explicit_origin as fn(),
    ),
    (
        "flow_apply_and_compose_operators_lower_to_trait_calls",
        flow_apply_and_compose_operators_lower_to_trait_calls as fn(),
    ),
    (
        "user_defined_container_can_use_context_operators_via_traits",
        user_defined_container_can_use_context_operators_via_traits as fn(),
    ),
    (
        "result_match_wildcard_self_after_ok_can_change_ok_payload_type",
        result_match_wildcard_self_after_ok_can_change_ok_payload_type as fn(),
    ),
    (
        "result_match_wildcard_self_after_ok_can_keep_err_for_bind_shape",
        result_match_wildcard_self_after_ok_can_keep_err_for_bind_shape as fn(),
    ),
    (
        "result_match_wildcard_self_requires_err_proven_branch",
        result_match_wildcard_self_requires_err_proven_branch as fn(),
    ),
    (
        "closure_param_annotation_must_match_expected_signature",
        closure_param_annotation_must_match_expected_signature as fn(),
    ),
    (
        "local_binding_annotation_can_reference_outer_generic_type_param",
        local_binding_annotation_can_reference_outer_generic_type_param as fn(),
    ),
    (
        "closure_param_annotation_can_reference_outer_generic_type_param",
        closure_param_annotation_can_reference_outer_generic_type_param as fn(),
    ),
    (
        "generic_first_can_inline_tuple_rebuild_with_closure_param_annotation",
        generic_first_can_inline_tuple_rebuild_with_closure_param_annotation as fn(),
    ),
    (
        "sibling_closures_keep_substitution_state_local",
        sibling_closures_keep_substitution_state_local as fn(),
    ),
    (
        "cyclic_type_definition_is_rejected",
        cyclic_type_definition_is_rejected as fn(),
    ),
    (
        "enum_cycle_is_allowed_when_not_shared_by_all_variants",
        enum_cycle_is_allowed_when_not_shared_by_all_variants as fn(),
    ),
    (
        "enum_cycle_is_rejected_when_shared_by_all_variants",
        enum_cycle_is_rejected_when_shared_by_all_variants as fn(),
    ),
    (
        "enum_field_access_is_rejected",
        enum_field_access_is_rejected as fn(),
    ),
    (
        "match_binding_pattern_is_treated_as_exhaustive",
        match_binding_pattern_is_treated_as_exhaustive as fn(),
    ),
    (
        "match_tuple_binding_pattern_is_treated_as_exhaustive",
        match_tuple_binding_pattern_is_treated_as_exhaustive as fn(),
    ),
    (
        "match_guard_must_be_boolean",
        match_guard_must_be_boolean as fn(),
    ),
    (
        "guarded_match_arm_does_not_satisfy_exhaustiveness",
        guarded_match_arm_does_not_satisfy_exhaustiveness as fn(),
    ),
    (
        "struct_literal_rejects_extra_fields",
        struct_literal_rejects_extra_fields as fn(),
    ),
    (
        "constructor_named_args_reject_duplicate_fields",
        constructor_named_args_reject_duplicate_fields as fn(),
    ),
    (
        "struct_literal_field_shorthand_typechecks",
        struct_literal_field_shorthand_typechecks as fn(),
    ),
    (
        "struct_literal_field_shorthand_mixed_with_explicit_typechecks",
        struct_literal_field_shorthand_mixed_with_explicit_typechecks as fn(),
    ),
    (
        "struct_literal_field_shorthand_rejects_duplicate_fields",
        struct_literal_field_shorthand_rejects_duplicate_fields as fn(),
    ),
    ("struct_requires_impl_new", struct_requires_impl_new as fn()),
    (
        "generic_struct_bare_annotation_requires_type_args",
        generic_struct_bare_annotation_requires_type_args as fn(),
    ),
    (
        "generic_struct_arity_mismatch_is_rejected",
        generic_struct_arity_mismatch_is_rejected as fn(),
    ),
    (
        "struct_new_accepts_result_self_return_type",
        struct_new_accepts_result_self_return_type as fn(),
    ),
    (
        "struct_new_rejects_non_self_return_type",
        struct_new_rejects_non_self_return_type as fn(),
    ),
    (
        "struct_new_rejects_result_non_self_payload",
        struct_new_rejects_result_non_self_payload as fn(),
    ),
    (
        "struct_constructor_call_accepts_result_return_type",
        struct_constructor_call_accepts_result_return_type as fn(),
    ),
    (
        "struct_literal_is_rejected_outside_impl_body",
        struct_literal_is_rejected_outside_impl_body as fn(),
    ),
    (
        "user_function_call_rejects_mixed_named_and_positional_args",
        user_function_call_rejects_mixed_named_and_positional_args as fn(),
    ),
    (
        "user_function_call_rejects_duplicate_named_arg",
        user_function_call_rejects_duplicate_named_arg as fn(),
    ),
    (
        "impl_self_rebinding_allows_self_type",
        impl_self_rebinding_allows_self_type as fn(),
    ),
    (
        "impl_self_rebinding_rejects_non_self_type",
        impl_self_rebinding_rejects_non_self_type as fn(),
    ),
    (
        "deferror_show_type_mismatch_points_to_show_expression_span",
        deferror_show_type_mismatch_points_to_show_expression_span as fn(),
    ),
    (
        "operator_traits_and_concrete_numeric_helpers_typecheck",
        operator_traits_and_concrete_numeric_helpers_typecheck as fn(),
    ),
    (
        "duration_operator_traits_dispatch_to_surtr_impls",
        duration_operator_traits_dispatch_to_surtr_impls as fn(),
    ),
    (
        "bounded_add_generics_specialize_without_pending_trait_calls",
        bounded_add_generics_specialize_without_pending_trait_calls as fn(),
    ),
    (
        "range_duration_comparisons_specialize_without_pending_trait_calls",
        range_duration_comparisons_specialize_without_pending_trait_calls as fn(),
    ),
    (
        "generic_struct_constructor_calls_remain_polymorphic_within_closure_body",
        generic_struct_constructor_calls_remain_polymorphic_within_closure_body as fn(),
    ),
    (
        "generic_struct_constructor_calls_remain_polymorphic_within_one_source",
        generic_struct_constructor_calls_remain_polymorphic_within_one_source as fn(),
    ),
    (
        "scar_session_preserves_trait_registry_across_chunks",
        scar_session_preserves_trait_registry_across_chunks as fn(),
    ),
    (
        "add_trait_mismatch_lists_available_implementations",
        add_trait_mismatch_lists_available_implementations as fn(),
    ),
    (
        "trait_method_call_rejects_named_arguments_without_panic",
        trait_method_call_rejects_named_arguments_without_panic as fn(),
    ),
    (
        "add_trait_missing_receiver_lists_available_implementations",
        add_trait_missing_receiver_lists_available_implementations as fn(),
    ),
    (
        "add_operator_missing_impl_lists_available_implementations_in_hint",
        add_operator_missing_impl_lists_available_implementations_in_hint as fn(),
    ),
    (
        "bind_operator_missing_impl_lists_available_implementations_in_hint",
        bind_operator_missing_impl_lists_available_implementations_in_hint as fn(),
    ),
    (
        "from_helper_typechecks_as_generic_trait_call",
        from_helper_typechecks_as_generic_trait_call as fn(),
    ),
    (
        "try_from_helper_typechecks_as_generic_trait_call",
        try_from_helper_typechecks_as_generic_trait_call as fn(),
    ),
    (
        "encode_helper_typechecks_as_generic_trait_call",
        encode_helper_typechecks_as_generic_trait_call as fn(),
    ),
    (
        "json_value_encode_source_alias_typechecks",
        json_value_encode_source_alias_typechecks as fn(),
    ),
    (
        "decode_helper_typechecks_explicit_target",
        decode_helper_typechecks_explicit_target as fn(),
    ),
    (
        "decode_helper_inside_decode_impl_dispatches_by_receiver_and_target",
        decode_helper_inside_decode_impl_dispatches_by_receiver_and_target as fn(),
    ),
    (
        "decode_helper_allows_same_pattern_recursive_dispatch",
        decode_helper_allows_same_pattern_recursive_dispatch as fn(),
    ),
    (
        "encode_helper_dispatches_to_receiver_impl_with_json_value_target",
        encode_helper_dispatches_to_receiver_impl_with_json_value_target as fn(),
    ),
    (
        "encode_helper_allows_same_pattern_recursive_dispatch",
        encode_helper_allows_same_pattern_recursive_dispatch as fn(),
    ),
    (
        "from_helper_suggests_try_from_when_only_fallible_impl_exists",
        from_helper_suggests_try_from_when_only_fallible_impl_exists as fn(),
    ),
    (
        "try_from_helper_suggests_from_when_only_infallible_impl_exists",
        try_from_helper_suggests_from_when_only_infallible_impl_exists as fn(),
    ),
    (
        "from_and_try_from_impls_are_mutually_exclusive",
        from_and_try_from_impls_are_mutually_exclusive as fn(),
    ),
    (
        "process_sleep_accepts_duration_literal",
        process_sleep_accepts_duration_literal as fn(),
    ),
    (
        "process_self_is_rejected_outside_process_context",
        process_self_is_rejected_outside_process_context as fn(),
    ),
    (
        "process_self_typechecks_inside_process_handler",
        process_self_typechecks_inside_process_handler as fn(),
    ),
    (
        "singleton_agent_pid_surface_returns_concrete_pid",
        singleton_agent_pid_surface_returns_concrete_pid as fn(),
    ),
    (
        "singleton_genserver_pid_surface_returns_concrete_pid",
        singleton_genserver_pid_surface_returns_concrete_pid as fn(),
    ),
    (
        "singleton_agent_explicit_pid_call_typechecks",
        singleton_agent_explicit_pid_call_typechecks as fn(),
    ),
    (
        "genserver_additional_call_handler_typechecks_as_process_context",
        genserver_additional_call_handler_typechecks_as_process_context as fn(),
    ),
    (
        "genserver_call_handler_accepts_call_result_contract",
        genserver_call_handler_accepts_call_result_contract as fn(),
    ),
    (
        "process_meta_state_mismatch_is_rejected",
        process_meta_state_mismatch_is_rejected as fn(),
    ),
    (
        "user_defined_process_state_can_appear_in_public_signatures",
        user_defined_process_state_can_appear_in_public_signatures as fn(),
    ),
    (
        "typecheck_staged_program_keeps_process_specs",
        typecheck_staged_program_keeps_process_specs as fn(),
    ),
    (
        "dynsup_spawn_accepts_worker_init_route_reference",
        dynsup_spawn_accepts_worker_init_route_reference as fn(),
    ),
    (
        "custom_supervisor_spawn_accepts_worker_init_route_reference",
        custom_supervisor_spawn_accepts_worker_init_route_reference as fn(),
    ),
    (
        "supervisor_spawn_rejects_plain_closure_argument",
        supervisor_spawn_rejects_plain_closure_argument as fn(),
    ),
    (
        "supervisor_spawn_rejects_non_worker_callable",
        supervisor_spawn_rejects_non_worker_callable as fn(),
    ),
    (
        "supervisor_adopt_accepts_worker_pid",
        supervisor_adopt_accepts_worker_pid as fn(),
    ),
    (
        "supervisor_adopt_rejects_non_pid_argument",
        supervisor_adopt_rejects_non_pid_argument as fn(),
    ),
    (
        "supervisor_adopt_rejects_when_policy_disallows_it",
        supervisor_adopt_rejects_when_policy_disallows_it as fn(),
    ),
    (
        "supervisor_status_returns_supervisor_status",
        supervisor_status_returns_supervisor_status as fn(),
    ),
    (
        "supervisor_workers_returns_workers_handle",
        supervisor_workers_returns_workers_handle as fn(),
    ),
    (
        "workers_submit_accepts_worker_message_template",
        workers_submit_accepts_worker_message_template as fn(),
    ),
    (
        "workers_broadcast_accepts_worker_message_template",
        workers_broadcast_accepts_worker_message_template as fn(),
    ),
    (
        "task_await_accepts_task_handle",
        task_await_accepts_task_handle as fn(),
    ),
    (
        "workers_reserve_can_flow_into_worker_call",
        workers_reserve_can_flow_into_worker_call as fn(),
    ),
    (
        "tap_err_accepts_local_error_observer_binding",
        tap_err_accepts_local_error_observer_binding as fn(),
    ),
    (
        "tap_err_accepts_error_observer_captures_and_composition",
        tap_err_accepts_error_observer_captures_and_composition as fn(),
    ),
    (
        "error_observer_binding_cannot_escape_as_plain_value",
        error_observer_binding_cannot_escape_as_plain_value as fn(),
    ),
    (
        "error_observer_binding_cannot_be_called_directly",
        error_observer_binding_cannot_be_called_directly as fn(),
    ),
    (
        "error_observer_binding_cannot_use_error_annotation",
        error_observer_binding_cannot_use_error_annotation as fn(),
    ),
    (
        "error_observer_closure_param_cannot_use_error_annotation",
        error_observer_closure_param_cannot_use_error_annotation as fn(),
    ),
    (
        "error_observer_binding_cannot_flow_through_generic_identity",
        error_observer_binding_cannot_flow_through_generic_identity as fn(),
    ),
];

macro_rules! surface_bucket_test {
    ($name:ident, $bucket:expr) => {
        #[test]
        fn $name() {
            run_surface_case_bucket($bucket, SURFACE_BUCKET_COUNT);
        }
    };
}

surface_bucket_test!(typecheck_surface_bucket_0, 0);
surface_bucket_test!(typecheck_surface_bucket_1, 1);
surface_bucket_test!(typecheck_surface_bucket_2, 2);
surface_bucket_test!(typecheck_surface_bucket_3, 3);
surface_bucket_test!(typecheck_surface_bucket_4, 4);
surface_bucket_test!(typecheck_surface_bucket_5, 5);
surface_bucket_test!(typecheck_surface_bucket_6, 6);
surface_bucket_test!(typecheck_surface_bucket_7, 7);
surface_bucket_test!(typecheck_surface_bucket_8, 8);
surface_bucket_test!(typecheck_surface_bucket_9, 9);
surface_bucket_test!(typecheck_surface_bucket_10, 10);
surface_bucket_test!(typecheck_surface_bucket_11, 11);
surface_bucket_test!(typecheck_surface_bucket_12, 12);
surface_bucket_test!(typecheck_surface_bucket_13, 13);
surface_bucket_test!(typecheck_surface_bucket_14, 14);
surface_bucket_test!(typecheck_surface_bucket_15, 15);
surface_bucket_test!(typecheck_surface_bucket_16, 16);
surface_bucket_test!(typecheck_surface_bucket_17, 17);
surface_bucket_test!(typecheck_surface_bucket_18, 18);
surface_bucket_test!(typecheck_surface_bucket_19, 19);
surface_bucket_test!(typecheck_surface_bucket_20, 20);
surface_bucket_test!(typecheck_surface_bucket_21, 21);
surface_bucket_test!(typecheck_surface_bucket_22, 22);
surface_bucket_test!(typecheck_surface_bucket_23, 23);
surface_bucket_test!(typecheck_surface_bucket_24, 24);
surface_bucket_test!(typecheck_surface_bucket_25, 25);
surface_bucket_test!(typecheck_surface_bucket_26, 26);
surface_bucket_test!(typecheck_surface_bucket_27, 27);
surface_bucket_test!(typecheck_surface_bucket_28, 28);
surface_bucket_test!(typecheck_surface_bucket_29, 29);
surface_bucket_test!(typecheck_surface_bucket_30, 30);
surface_bucket_test!(typecheck_surface_bucket_31, 31);

fn run_surface_case_bucket(bucket: usize, bucket_count: usize) {
    assert!(bucket_count > 0, "bucket_count must be positive");
    assert!(
        bucket < bucket_count,
        "bucket {bucket} out of range {bucket_count}"
    );

    let case_indices = SURFACE_CASES
        .iter()
        .enumerate()
        .filter_map(|(idx, _)| (idx % bucket_count == bucket).then_some(idx))
        .collect::<Vec<_>>();
    assert!(
        !case_indices.is_empty(),
        "no scar surface cases assigned to bucket {bucket} of {bucket_count}"
    );

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let next_case = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let failures = Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        for _ in 0..SURFACE_WORKER_COUNT {
            scope.spawn(|| loop {
                if failed.load(Ordering::Relaxed) {
                    break;
                }
                let position = next_case.fetch_add(1, Ordering::Relaxed);
                let Some(idx) = case_indices.get(position).copied() else {
                    break;
                };
                let Some((name, case)) = SURFACE_CASES.get(idx) else {
                    break;
                };
                if let Err(payload) = std::panic::catch_unwind(*case) {
                    failed.store(true, Ordering::Relaxed);
                    failures.lock().expect("failure lock").push(format!(
                        "{name}: {}",
                        panic_payload_message(payload.as_ref())
                    ));
                    break;
                }
            });
        }
    });

    std::panic::set_hook(previous_hook);

    let failures = failures.into_inner().expect("failure lock");
    if !failures.is_empty() {
        panic!(
            "{} scar surface case(s) failed in bucket {}/{}:\n{}",
            failures.len(),
            bucket,
            bucket_count,
            failures.join("\n")
        );
    }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

fn typed_bind_rhs<'a>(typed: &'a [TypedNode], name: &str) -> &'a TypedNode {
    typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(TypedPattern::Var(_, id), rhs)
            | TypedInner::SafeBind(TypedPattern::Var(_, id), rhs)
                if id.name == name =>
            {
                Some(rhs.as_ref())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected binding `{name}`"))
}

fn resolved_test_id(name: &str, unique_id: u32, span: &Span) -> ResolvedId {
    ResolvedId {
        name: name.into(),
        qualified_name: None,
        symbol_info: None,
        unique_id,
        compiler_generated: false,
        span: span.clone(),
    }
}

fn resolved_bracket_segment(expr: Resolved, display: &str) -> ResolvedFacetPathSegment {
    ResolvedFacetPathSegment::Bracket(ResolvedFacetBracketExpr {
        expr: Box::new(expr),
        display: display.into(),
    })
}

fn process_stdlib_no_longer_declares_task_hidden_lower_helpers() {
    for hidden_name in [
        "__task_call",
        "__task_async",
        "__task_launch",
        "__task_cast",
        "__task_call_timeout",
        "__task_async_timeout",
        "__task_launch_timeout",
        "__task_cast_timeout",
        "__workers_submit",
        "__workers_broadcast",
        "__workers_reserve",
        "__workers_size",
    ] {
        assert!(
            !PROCESS_MODULE_SOURCE.contains(hidden_name),
            "process stdlib should not declare {hidden_name}"
        );
    }
}

fn process_stdlib_declares_common_process_family_modules() {
    for module_name in ["defmod Supervisor", "defmod GenServer", "defmod Agent"] {
        assert!(
            PROCESS_MODULE_SOURCE.contains(module_name),
            "process stdlib should declare {module_name}"
        );
    }
}

fn process_module_only_declares_public_runtime_helpers() {
    let process_start = PROCESS_MODULE_SOURCE
        .find("defmod Process")
        .expect("Process module should exist");
    let out_handler_start = PROCESS_MODULE_SOURCE
        .find("defmod OutHandler")
        .expect("OutHandler module should follow Process");
    let process_module = &PROCESS_MODULE_SOURCE[process_start..out_handler_start];

    assert!(
        !process_module.contains("@hidden"),
        "Process module should contain only directly callable public helpers"
    );
    for internal_name in [
        "__process_pid",
        "__process_spawn",
        "__process_state",
        "__process_store",
        "__process_self",
        "__process_context_handler",
        "__process_sleep",
    ] {
        assert!(
            !process_module.contains(internal_name),
            "Process module should not declare {internal_name}"
        );
    }
}

fn process_stdlib_declares_agent_lower_surface_with_regular_surface_docs() {
    let agent_start = PROCESS_MODULE_SOURCE
        .find("defmod Agent")
        .expect("Agent module should exist");
    let process_start = PROCESS_MODULE_SOURCE
        .find("defmod Process")
        .expect("Process module should follow Agent");
    let agent_module = &PROCESS_MODULE_SOURCE[agent_start..process_start];

    for surface in [
        "def pid(",
        "def spawn(",
        "def state(",
        "def store(",
        "def self()",
        "def context_handler(",
    ] {
        assert!(
            agent_module.contains(surface),
            "Agent module should declare hidden lower surface {surface}"
        );
    }
    assert!(
        agent_module.contains("Ordinary code cannot call this function directly."),
        "Agent lower docs should tell users they cannot call hidden functions directly"
    );
    assert!(
        agent_module.contains("## Regular Surface"),
        "Agent lower docs should show the regular surface"
    );
}

fn field_access_is_resolved_to_numeric_index() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age }
  }
}

user: User = User("alice", 30)
age = user.age"#,
    );

    let typed = typecheck(resolved).expect("typecheck should succeed");
    let facet_view = typed.iter().find_map(|node| {
        if let TypedInner::Bind(_, rhs) = &node.node {
            if let TypedInner::FacetView {
                path,
                source_is_result,
                ..
            } = &rhs.node
            {
                return Some((path.clone(), *source_is_result));
            }
        }
        None
    });

    let (path, source_is_result) = facet_view.expect("expected bind rhs to be FacetView");
    assert!(!source_is_result);
    assert!(!path.may_fail);
    assert_eq!(path.segments.len(), 1);
    match &path.segments[0] {
        TypedFacetSegment::Field { field_index, .. } => assert_eq!(*field_index, 1),
        other => panic!("expected field segment, got {other:?}"),
    }
}

fn match_bool_requires_exhaustive_arms() {
    let resolved = resolve_with_builtin_prelude(
        r#"flag = True
print(match flag {
  True => "yes",
})"#,
    );

    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err.message.contains("Non-exhaustive match. Missing: False"));
}

fn match_bool_accepts_qualified_boolean_constructor_patterns() {
    let resolved = resolve_with_builtin_prelude(
        r#"flag = True
print(match flag {
  Boolean::True => "yes",
  Boolean::False => "no",
})"#,
    );

    typecheck(resolved).expect("Boolean constructor patterns should typecheck like enum variants");
}

fn match_bool_qualified_constructor_patterns_require_exhaustive_arms() {
    let resolved = resolve_with_builtin_prelude(
        r#"flag = True
print(match flag {
  Boolean::True => "yes",
})"#,
    );
    let err = typecheck(resolved)
        .expect_err("qualified Boolean constructor patterns should use enum exhaustiveness");
    assert!(err.message.contains("Non-exhaustive match. Missing: False"));
}

fn safebind_total_pattern_accepts_plain_rhs() {
    let resolved = resolve_with_builtin_prelude("num =? 10");
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

fn dbg_special_form_typechecks_to_unit() {
    let resolved = resolve_with_builtin_prelude("x = dbg!(1, \"ok\")");
    let typed = typecheck(resolved).expect("typecheck should succeed");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");

    assert_eq!(rhs.ty, Ty::Unit);
    assert!(matches!(rhs.node, TypedInner::Dbg(_)));
}

fn safebind_function_requires_result_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"def bad() -> Int {
  num =? Ok(1)
  num
}"#,
    );

    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err
        .message
        .contains("can only be used in functions returning Result"));
}

fn safebind_result_closure_uses_nearest_callable_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"handler: (Int -> Result<Int>) = {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    typecheck(resolved).expect("Result-returning closure should allow SafeBind");
}

fn safebind_non_result_closure_is_rejected() {
    let resolved = resolve_with_builtin_prelude(
        r#"bad: (Int -> Int) = {|x|
  value =? Ok(x)
  value
}"#,
    );
    let err = typecheck(resolved).expect_err("non-Result closure should reject SafeBind");
    assert!(err
        .message
        .contains("can only be used in functions returning Result"));
}

fn safebind_result_returning_annotated_closure_allows_safebind() {
    let typed = typecheck_with_builtin_prelude(
        r#"handler: (Int -> Result<Int>) = {|x|
  value =? Ok(x + 1)
  Ok(value)
}

result: Result<Int> = handler(1)"#,
    );
    assert_eq!(typed.last().map(|node| &node.ty), Some(&Ty::Unit));
}

fn safebind_non_result_closure_rejects_safebind() {
    let resolved = resolve_with_builtin_prelude(
        r#"handler: (Int -> Int) = {|x|
  value =? Ok(x + 1)
  value
}"#,
    );

    let err = typecheck(resolved).expect_err("non-Result closure should fail");
    assert!(err
        .message
        .contains("can only be used in functions returning Result"));
}

fn safebind_top_ok_pattern_requires_nested_result_rhs() {
    let resolved = resolve_with_builtin_prelude(
        r#"value: Result<Int> = Ok(1)
Ok(num) =? value"#,
    );
    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err.message.contains("`Ok(...)` pattern requires Result"));
}

fn safebind_top_ok_pattern_accepts_nested_result_rhs() {
    let resolved = resolve_with_builtin_prelude(
        r#"value: Result<Result<Int>> = Ok(Ok(1))
Ok(num) =? value"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

fn safebind_list_pattern_accepts_plain_list_rhs() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = [1, 2, 3]
[head, ..tail] =? value"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

fn safebind_string_pattern_accepts_plain_string_rhs() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = "source"
[head, ..tail] =? value"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

fn int_range_literal_typechecks_to_list_int() {
    let resolved = resolve_with_builtin_prelude("nums = [1..3]");
    let typed = typecheck(resolved).expect("typecheck should succeed");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");
    assert_eq!(rhs.ty, Ty::List(Box::new(Ty::Int)));
}

fn string_range_literal_typechecks_to_result_list_string() {
    let resolved = resolve_with_builtin_prelude(r#"chars = ["a".."c"]"#);
    let typed = typecheck(resolved).expect("typecheck should succeed");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");
    assert_eq!(
        rhs.ty,
        Ty::Result(Box::new(Ty::List(Box::new(Ty::Str))), Box::new(Ty::Error))
    );
}

fn hash_map_literal_typechecks_string_key_expressions() {
    let resolved = resolve_with_builtin_prelude(
        r#"raw = " talk "
scores = hash!["talk" => 80, String::trim(raw) => 90]"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    let rhs = typed_bind_rhs(&typed, "scores");
    assert_eq!(rhs.ty, Ty::Enum("HashMap".into(), vec![Ty::Int]));
    assert!(matches!(rhs.node, TypedInner::HashMapLiteral(_)));
}

fn hash_map_literal_rejects_non_string_keys() {
    let resolved = resolve_with_builtin_prelude(r#"scores = hash![1 => "bad"]"#);
    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err.message.contains("HashMap literal key must be String"));
}

fn hash_map_literal_rejects_mixed_value_types() {
    let resolved = resolve_with_builtin_prelude(r#"scores = hash!["ok" => 1, "bad" => "two"]"#);
    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err.message.contains("expected Int, got String"));
}

fn match_string_requires_empty_and_uncons_arms_for_exhaustiveness() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = "x"
print(match value {
  [head, ..tail] => head,
})"#,
    );

    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err.message.contains("Non-exhaustive match. Missing: []"));
}

fn match_string_accepts_empty_and_uncons_arms() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = "x"
print(match value {
  [] => "empty",
  [head, ..tail] => tail,
})"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(!typed.is_empty());
}

fn safebind_list_pattern_accepts_nested_constructor_literals() {
    let resolved = resolve_with_builtin_prelude(
        r#"lr = [Ok(1), Ok(2), Ok(3)]
[Ok(1), Ok(2), _] =? lr"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

fn tuple_literal_and_field_access_typecheck() {
    let resolved = resolve_with_builtin_prelude(
        r#"pair = (1, "two")
first = pair._0
second = pair._1"#,
    );
    let typed = typecheck(resolved).expect("tuple access should typecheck");
    assert!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::Bind(_, _)))
            .count()
            >= 3
    );
}

fn tuple_bind_pattern_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"pair = (1, "two")
(left, right) = pair"#,
    );
    let typed = typecheck(resolved).expect("tuple bind should typecheck");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

fn facet_view_on_plain_value_returns_plain_focus() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
user.name"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::FacetView { .. }));
}

fn facet_view_on_result_value_returns_result_focus() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
result_user: Result<User> = Ok(User("alice"))
result_user.name"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Str)
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
    assert!(matches!(last.node, TypedInner::FacetView { .. }));
}

fn facet_variant_selector_returns_result_and_requires_pascal_case() {
    let typed = typecheck_with_builtin_prelude(
        r#"defenum Expr {
  Add(Int, Int),
  Halt,
}
expr = Expr::Add(1, 2)
expr.Add"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Tuple(items) if items.len() == 2)
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));

    let err = typecheck_with_rules(
        r#"defenum Expr {
  Add(Int, Int),
  Halt,
}
expr = Expr::Add(1, 2)
expr.add"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("lowercase variant selector should fail");
    assert!(err.message.contains("No variant selector 'add'"));
}

fn facet_preview_requires_variant_path_and_records_path_kind() {
    let typed = typecheck_with_builtin_prelude(
        r#"defenum Expr {
  Add(Int, Int),
  Halt,
}
expr = Expr::Add(1, 2)
Facet::preview(Expr.Add / Tuple._0, expr)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    let TypedInner::FacetView { path, .. } = &last.node else {
        panic!("expected Facet::preview to lower as a view");
    };
    assert_eq!(path.path_kind, TypedFacetPathKind::VariantPath);
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Int)
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));

    let err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = User("alice")
Facet::preview(User.name, user)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("preview should reject structural paths");
    assert!(err
        .message
        .contains("Facet::preview requires a variant Facet"));
}

fn facet_preview_accepts_option_variant() {
    let typed = typecheck_with_builtin_prelude(
        r#"value: Option<Int> = Option::Some(1)
Facet::preview(Option.Some, value)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    let TypedInner::FacetView { path, .. } = &last.node else {
        panic!("expected Facet::preview to lower as a view");
    };
    assert_eq!(path.path_kind, TypedFacetPathKind::VariantPath);
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Int)
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
}

#[test]
fn facet_preview_accepts_boolean_variant_root() {
    let typed = typecheck_with_builtin_prelude(
        r#"flag = True
Facet::preview(Boolean.True, flag)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    let TypedInner::FacetView { path, .. } = &last.node else {
        panic!("expected Boolean.True to lower as a variant Facet view");
    };
    assert_eq!(path.path_kind, TypedFacetPathKind::VariantPath);
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Unit)
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
}

fn facet_boolean_selector_uses_regular_enum_diagnostic() {
    let err = typecheck_with_rules(
        r#"flag = True
Facet::preview(Boolean.Maybe, flag)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("unknown Boolean selector should fail through regular enum selector handling");
    assert!(err
        .message
        .contains("No variant selector 'Maybe' on Boolean (use PascalCase constructor names)"));
}

fn facet_list_and_map_segments_are_fallible_structural_paths() {
    let typed = typecheck_with_builtin_prelude(
        r#"scores = [10, 20, 30]
score_map = HashMap::from_entries([("talk", 80)])
list_root = Facet::view(List.[1], scores)
map_root = Facet::view(HashMap.["talk"], score_map)
list_value = scores.[1]
map_value = score_map.["talk"]"#,
    );

    for name in ["list_root", "map_root", "list_value", "map_value"] {
        let rhs = typed_bind_rhs(&typed, name);
        assert!(
            matches!(
                &rhs.ty,
                Ty::Result(ok, err)
                    if matches!(ok.as_ref(), Ty::Int) && matches!(err.as_ref(), Ty::Error)
            ),
            "{name} should be Result<Int>, got {:?}",
            rhs.ty
        );
        let TypedInner::FacetView { path, .. } = &rhs.node else {
            panic!("{name} should lower to FacetView, got {:?}", rhs.node);
        };
        assert_eq!(path.path_kind, TypedFacetPathKind::FallibleStructural);
        assert!(path.may_fail, "{name} should be fallible");
    }

    assert!(matches!(
        &typed_bind_rhs(&typed, "list_root").node,
        TypedInner::FacetView { path, .. }
            if matches!(path.segments.as_slice(), [TypedFacetSegment::ListIndex { .. }])
    ));
    assert!(matches!(
        &typed_bind_rhs(&typed, "map_root").node,
        TypedInner::FacetView { path, .. }
            if matches!(path.segments.as_slice(), [TypedFacetSegment::MapKey { literal_key, .. }] if literal_key.as_deref() == Some("talk"))
    ));
}

fn facet_explicit_container_root_captures_use_expected_function_context() {
    let typed = typecheck_with_builtin_prelude(
        r#"scores = [10, 20]
score_map = HashMap::from_entries([("talk", 80)])
get_first: (List<Int> -> Result<Int>) = &List.[0]
get_talk: (HashMap<Int> -> Result<Int>) = &HashMap.["talk"]
first = get_first(scores)
talk = get_talk(score_map)"#,
    );

    for name in ["first", "talk"] {
        let rhs = typed_bind_rhs(&typed, name);
        assert!(
            matches!(
                &rhs.ty,
                Ty::Result(ok, err)
                    if matches!(ok.as_ref(), Ty::Int) && matches!(err.as_ref(), Ty::Error)
            ),
            "{name} should be Result<Int>, got {:?}",
            rhs.ty
        );
    }
}

#[test]
fn facet_root_dispatch_requires_symbol_capability_metadata_not_matching_names() {
    let span = Span { start: 0, end: 0 };
    let tuple_id = resolved_test_id("Tuple", 900_001, &span);
    let tuple_value_id = resolved_test_id("tuple_value", 900_002, &span);
    let list_id = resolved_test_id("List", 900_003, &span);
    let list_value_id = resolved_test_id("list_value", 900_004, &span);
    let map_id = resolved_test_id("HashMap", 900_005, &span);
    let map_value_id = resolved_test_id("map_value", 900_006, &span);

    let typed = typecheck(vec![
        Resolved::Bind(
            span.clone(),
            ResolvedPattern::Var(tuple_id.clone()),
            Box::new(Resolved::TupleLiteral(
                span.clone(),
                vec![
                    Resolved::Lit(span.clone(), Lit::Str("alice".into())),
                    Resolved::Lit(span.clone(), Lit::Int(int(42))),
                ],
            )),
        ),
        Resolved::Bind(
            span.clone(),
            ResolvedPattern::Var(tuple_value_id),
            Box::new(Resolved::FieldAccess(
                span.clone(),
                Box::new(Resolved::Var(span.clone(), tuple_id)),
                "_0".into(),
            )),
        ),
        Resolved::Bind(
            span.clone(),
            ResolvedPattern::Var(list_id.clone()),
            Box::new(Resolved::ListLiteral(
                span.clone(),
                vec![
                    Resolved::Lit(span.clone(), Lit::Int(int(10))),
                    Resolved::Lit(span.clone(), Lit::Int(int(20))),
                ],
            )),
        ),
        Resolved::Bind(
            span.clone(),
            ResolvedPattern::Var(list_value_id),
            Box::new(Resolved::FacetSegmentAccess(
                span.clone(),
                Box::new(Resolved::Var(span.clone(), list_id)),
                resolved_bracket_segment(Resolved::Lit(span.clone(), Lit::Int(int(0))), "0"),
            )),
        ),
        Resolved::Bind(
            span.clone(),
            ResolvedPattern::Var(map_id.clone()),
            Box::new(Resolved::HashMapLiteral(
                span.clone(),
                vec![ResolvedHashMapLiteralEntry {
                    key: Resolved::Lit(span.clone(), Lit::Str("talk".into())),
                    value: Resolved::Lit(span.clone(), Lit::Int(int(80))),
                }],
            )),
        ),
        Resolved::Bind(
            span.clone(),
            ResolvedPattern::Var(map_value_id),
            Box::new(Resolved::FacetSegmentAccess(
                span.clone(),
                Box::new(Resolved::Var(span.clone(), map_id)),
                resolved_bracket_segment(
                    Resolved::Lit(span.clone(), Lit::Str("talk".into())),
                    "\"talk\"",
                ),
            )),
        ),
    ])
    .expect("value bindings with root-like names should typecheck as ordinary values");

    let tuple_rhs = typed_bind_rhs(&typed, "tuple_value");
    assert!(matches!(tuple_rhs.ty, Ty::Str));

    for name in ["list_value", "map_value"] {
        let rhs = typed_bind_rhs(&typed, name);
        assert!(
            matches!(
                &rhs.ty,
                Ty::Result(ok, err)
                    if matches!(ok.as_ref(), Ty::Int) && matches!(err.as_ref(), Ty::Error)
            ),
            "{name} should be Result<Int>, got {:?}",
            rhs.ty
        );
        assert!(matches!(rhs.node, TypedInner::FacetView { .. }));
    }
}

#[test]
fn facet_root_capability_dispatch_preserves_standard_roots_and_string_diagnostic() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
pair = ("bob", 7)
scores = [10, 20]
score_map = HashMap::from_entries([("talk", 80)])
user_name = Facet::view(User.name, user)
tuple_name = Facet::view(Tuple._0, pair)
list_score = Facet::view(List.[0], scores)
map_score = Facet::view(HashMap.["talk"], score_map)"#,
    );

    for name in ["user_name", "tuple_name"] {
        let rhs = typed_bind_rhs(&typed, name);
        assert!(matches!(rhs.ty, Ty::Str), "{name} should be String");
        assert!(matches!(rhs.node, TypedInner::FacetView { .. }));
    }
    for name in ["list_score", "map_score"] {
        let rhs = typed_bind_rhs(&typed, name);
        assert!(
            matches!(
                &rhs.ty,
                Ty::Result(ok, err)
                    if matches!(ok.as_ref(), Ty::Int) && matches!(err.as_ref(), Ty::Error)
            ),
            "{name} should be Result<Int>, got {:?}",
            rhs.ty
        );
        assert!(matches!(rhs.node, TypedInner::FacetView { .. }));
    }

    let err = typecheck_with_rules("bad = String.len", RuntimeSourcePolicy::script())
        .expect_err("String.len should stay a known-symbol non-Facet-root diagnostic");
    assert!(err.message.contains("String is not a Facet path root"));
}

#[test]
fn deferred_list_and_hashmap_facet_bindings_can_be_reused_by_facet_intrinsics() {
    let typed = typecheck_with_builtin_prelude(
        r#"scores = [10, 20]
score_map = HashMap::from_entries([("talk", 80)])
list_path = List.[0]
map_path = HashMap.["talk"]
list_score = Facet::view(list_path, scores)
map_score = Facet::view(map_path, score_map)"#,
    );

    for name in ["list_score", "map_score"] {
        let rhs = typed_bind_rhs(&typed, name);
        assert!(
            matches!(
                &rhs.ty,
                Ty::Result(ok, err)
                    if matches!(ok.as_ref(), Ty::Int) && matches!(err.as_ref(), Ty::Error)
            ),
            "{name} should be Result<Int>, got {:?}",
            rhs.ty
        );
        assert!(matches!(rhs.node, TypedInner::FacetView { .. }));
    }
}

fn facet_dynamic_container_segments_accept_runtime_expressions() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord ScoreBook(scores: List<Int>, by_kind: HashMap<Int>)
def find_index(values: List<Int>) -> Int { 1 }
def normalize_key(raw: String) -> String { String::trim(raw) }
scores = [10, 20, 30]
score_map = HashMap::from_entries([("talk", 80)])
book = ScoreBook(scores, score_map)
index = 0
raw_name = " talk "
list_root = Facet::view(List.[index + 1], scores)
map_root = Facet::view(HashMap.[normalize_key(raw_name)], score_map)
list_value = scores.[find_index(scores)]
map_value = score_map.[String::trim(raw_name)]
path = ScoreBook.scores.[index + 1]
bulk = Facet::bulk_update(book) {
  scores.[index + 1] <- set(99)
  by_kind.[String::trim(raw_name)] <- over({|value| Ok(value + 1)})
}"#,
    );

    for name in ["list_root", "map_root", "list_value", "map_value", "bulk"] {
        let rhs = typed_bind_rhs(&typed, name);
        assert!(
            matches!(&rhs.ty, Ty::Result(_, err) if matches!(err.as_ref(), Ty::Error)),
            "{name} should be Result<...>, got {:?}",
            rhs.ty
        );
    }

    let path_rhs = typed_bind_rhs(&typed, "path");
    assert!(matches!(
        path_rhs.node,
        TypedInner::FacetPath(_) | TypedInner::PendingFacetPath(_)
    ));
}

fn facet_dynamic_container_segments_reject_result_and_wrong_key_types() {
    let list_err = typecheck_with_rules(
        r#"def find_index(values: List<Int>) -> Result<Int> { Ok(0) }
values = [1, 2, 3]
bad = Facet::view(List.[find_index(values)], values)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("Result<Int> bracket expression should fail");
    assert!(list_err.message.contains(
        "Facet bracket expression must be plain Int; unwrap Result<Int> before using it"
    ));

    let map_err = typecheck_with_rules(
        r#"map: HashMap<Int> = HashMap::from_entries([("taro", 18)])
bad = Facet::view(HashMap.[1], map)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("non-String HashMap key should fail");
    assert!(map_err
        .message
        .contains("HashMap Facet key expression must be String"));
}

fn facet_negative_list_index_and_range_segments_typecheck() {
    let typed = typecheck_with_builtin_prelude(
        r#"scores = [10, 20, 30, 40]
last = Facet::view(List.[-1], scores)
window = Facet::view(List.[1..-2], scores)
updated = Facet::set(List.[1..2], scores, [99, 100, 101])
bumped = Facet::over(List.[0..1], scores, {|slice| Ok(List::append(slice, [77]))})"#,
    );

    let last_rhs = typed_bind_rhs(&typed, "last");
    assert!(matches!(
        &last_rhs.ty,
        Ty::Result(ok, err)
            if matches!(ok.as_ref(), Ty::Int) && matches!(err.as_ref(), Ty::Error)
    ));

    for name in ["window", "updated", "bumped"] {
        let rhs = typed_bind_rhs(&typed, name);
        assert!(
            matches!(
                &rhs.ty,
                Ty::Result(ok, err)
                    if matches!(ok.as_ref(), Ty::List(_)) && matches!(err.as_ref(), Ty::Error)
            ),
            "{name} should be Result<List<_>>, got {:?}",
            rhs.ty
        );
    }
}

fn facet_range_segments_require_plain_int_endpoints_and_list_values() {
    let endpoint_err = typecheck_with_rules(
        r#"def find_index(values: List<Int>) -> Result<Int> { Ok(0) }
values = [1, 2, 3]
bad = Facet::view(List.[find_index(values)..1], values)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("Result<Int> range endpoint should fail");
    assert!(endpoint_err.message.contains(
        "Facet bracket expression must be plain Int; unwrap Result<Int> before using it"
    ));

    let set_err = typecheck_with_rules(
        r#"values = [1, 2, 3]
bad = Facet::set(List.[0..1], values, 9)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("slice set should require List<A>");
    assert!(
        set_err.message.contains("Facet::set value type mismatch")
            || set_err
                .message
                .contains("Facet updates through List segments cannot change the element type"),
        "{set_err:?}"
    );

    let over_err = typecheck_with_rules(
        r#"values = [1, 2, 3]
bad = Facet::over(List.[0..1], values, {|n| Ok(n + 1)})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("slice over should require List<A> updater");
    assert!(
        over_err.message.contains("Facet::over update function")
            || over_err.message.contains("Argument type mismatch"),
        "{over_err:?}"
    );
}

fn facet_const_dynamic_container_segments_require_literals() {
    let err = typecheck_with_rules(
        r#"index = 0
const PATH: Facet<List<Int>, Int> = List.[index]"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("dynamic container bracket in const facet should fail");
    assert!(err
        .message
        .contains("const Facet path bracket segments must use literal Int or String values"));
}

fn facet_optional_marker_rejected_on_non_enum_segment() {
    let err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = User("alice")
Facet::set(User.name?, user, "bob")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("optional marker on a field should fail");
    assert!(err
        .message
        .contains("optional Facet selectors are no longer supported"));
}

fn facet_case_api_requires_enum_path_and_records_modes() {
    let typed = typecheck_with_builtin_prelude(
        r#"defenum Slot {
  Some(Result<String>),
  None,
}
slot = Slot::Some(Ok("alice"))
updated =? Facet::case_set(Slot.Some, slot, Ok("bob"))
overed =? Facet::case_over(Slot.Some, updated, {|name| Ok(name ++ "!")})
Facet::case_over(Slot.Some, overed, {|value: Result<String>| Ok(value)})"#,
    );
    let rendered = format!("{typed:?}");
    assert!(rendered.contains("CaseSet"), "{rendered}");
    assert!(rendered.contains("CaseFocusValue"), "{rendered}");
    assert!(rendered.contains("CaseFocusResult"), "{rendered}");

    let err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = User("alice")
Facet::case_over(User.name, user, {|name| Ok(name ++ "!")})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("case_over should reject structural-only paths");
    assert!(err.message.contains("requires an enum Facet path"));
}

fn facet_surface_resolves_after_facet_rename() {
    let resolved = resolve_with_builtin_prelude_result(
        r#"defrecord User(name: String)
user = User("alice")
Facet::view(User.name, user)"#,
    )
    .expect("Facet surface should remain available");
    assert!(!resolved.is_empty());
}

fn facet_chain_typecheck_success_and_mismatch() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
user = User(Profile("alice"))
Facet::view(chain(User.profile, Profile.name), user)"#,
    );
    assert!(matches!(
        typed.last().map(|node| &node.ty),
        Some(scar::types::Ty::Str)
    ));

    let err = typecheck_with_rules(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
chain(Profile.name, User.profile)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("mismatched chain should fail");
    assert!(!err.message.is_empty());
}

fn facet_slash_compose_typecheck_success_and_mismatch() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
user = User(Profile("alice"))
Facet::view(User.profile / Profile.name, user)"#,
    );
    assert!(matches!(
        typed.last().map(|node| &node.ty),
        Some(scar::types::Ty::Str)
    ));

    let err = typecheck_with_rules(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
Profile.name / User.profile"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("mismatched slash compose should fail");
    assert!(!err.message.is_empty());
}

fn facet_set_returns_result_source() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
Facet::set(User.name, user, "bob")"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Record(name, _) if name == "User")
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
    assert!(matches!(last.node, TypedInner::FacetSet { .. }));
}

fn facet_put_returns_plain_source() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
put(User.name, user, "bob")"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Record(name, _) if name == "User"
    ));
    assert!(matches!(last.node, TypedInner::FacetSet { .. }));
}

fn facet_put_rejects_result_source_and_variant_path() {
    let result_source_err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = Ok(User("alice"))
put(User.name, user, "bob")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("Result source should fail for Facet::put");
    assert!(result_source_err
        .message
        .contains("Facet::put requires a plain source value"));

    let variant_path_err = typecheck_with_rules(
        r#"defenum Expr {
  Add(Int, Int),
  Halt,
}
expr = Expr::Add(1, 2)
put(Expr.Add / Tuple._0, expr, 7)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("variant path should fail for Facet::put");
    assert!(variant_path_err
        .message
        .contains("Facet::put requires an infallible structural Facet path"));
}

fn facet_put_supports_same_type_tuple_update_inside_annotated_closure() {
    let typed = typecheck_with_builtin_prelude(
        r#"def first(f: (Int -> Int)) -> ((Int, Boolean) -> (Int, Boolean)) {
  {|pair: (Int, Boolean)| Facet::put(Tuple._0, pair, f(pair._0))}
}"#,
    );
    assert!(!typed.is_empty());
}

fn facet_put_unannotated_closure_still_lacks_tuple_context_from_expected_return() {
    let err = typecheck_with_rules(
        r#"def first(f: (Int -> Int)) -> ((Int, Boolean) -> (Int, Boolean)) {
  {|pair| Facet::put(Tuple._0, pair, f(pair._0))}
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("unannotated closure should still expose tuple context gap");
    assert!(err
        .message
        .contains("Tuple._0 requires tuple source context"));
}

fn facet_put_supports_type_changing_tuple_update() {
    typecheck_with_rules(
        r#"def first(f: (Int -> String)) -> ((Int, Boolean) -> (String, Boolean)) {
  {|pair: (Int, Boolean)| Facet::put(Tuple._0, pair, f(pair._0))}
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("Facet::put should rebuild a tuple with the replacement type");
}

fn facet_put_rebuilds_unique_generic_named_type() {
    typecheck_with_rules(
        r#"defstruct Box<$A> {
  value: $A,
}
impl Box {
  def new<$A>(value: $A) -> Box<$A> { Box { value: value } }
}
updated = Facet::put(Box.value, Box(1), "one")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("Facet::put should rebuild a uniquely parameterized named type");
}

fn facet_put_rejects_repeated_generic_named_type() {
    let err = typecheck_with_rules(
        r#"defstruct Pair<$A> {
  left: $A,
  right: $A,
}
impl Pair {
  def new<$A>(left: $A, right: $A) -> Pair<$A> { Pair { left: left, right: right } }
}
Facet::put(Pair.left, Pair(1, 2), "one")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("a repeated generic parameter must not be rebuilt through one field");
    assert!(err
        .message
        .contains("generic parameter occurs outside the updated field"));
}

fn facet_case_set_rebuilds_unique_generic_enum() {
    typecheck_with_rules(
        r#"defenum Slot<$A> {
  Some($A),
  None,
}
updated = Facet::case_set(Slot.Some, Slot::Some(1), "one")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("Facet::case_set should rebuild a uniquely parameterized enum");
}

fn facet_put_rejects_result_annotation_context() {
    let err = typecheck_with_rules(
        r#"defrecord User(name: String)
updated: Result<User> = Facet::put(User.name, User("alice"), "bob")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("Facet::put should explain Result annotation mismatch");
    assert!(err.message.contains("expected Result<User>, got User"));
}

fn facet_put_rejects_result_return_context() {
    let err = typecheck_with_rules(
        r#"defrecord User(name: String)
def rename() -> Result<User> {
  Facet::put(User.name, User("alice"), "bob")
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("Facet::put should explain Result return mismatch");
    assert!(err.message.contains("expected Result<User>, got User"));
}

fn facet_over_requires_unary_result_callable() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
Facet::over(User.name, user, {|name| Ok(name)})"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Record(name, _) if name == "User")
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
    assert!(matches!(last.node, TypedInner::FacetOver { .. }));

    let err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = User("alice")
Facet::over(User.name, user, {|name| name})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("non-Result update function should fail");
    assert!(err
        .message
        .contains("Facet::over update function must return Result"));
}

fn optional_type_annotation_matches_option() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Boxed(
  value: Int?,
)
boxed = Boxed(Option::Some(1))
same: Option<Int> = boxed.value"#,
    );
    assert!(!typed.is_empty());
}

#[test]
fn optional_type_annotation_rejects_result_value() {
    let resolved = resolve_with_builtin_prelude(
        r#"defrecord Boxed(
  value: Int?,
)
boxed = Boxed(Ok(1))"#,
    );
    let err = typecheck(resolved).expect_err("Result value should not typecheck for Int?");
    assert!(
        err.message
            .contains("expected Option<Int>, got Result<Int>")
            || err
                .message
                .contains("Record field value expected Option<Int>, got Result<Int>"),
        "{}",
        err.message
    );
}

fn facet_set_rejects_plain_value_for_result_focus() {
    let resolved = resolve_with_builtin_prelude(
        r#"defrecord User(score: Result<Int>)
user = User(Err(NoneError))
Facet::set(User.score, user, 3)"#,
    );
    let err = typecheck(resolved).expect_err("set must not implicitly wrap a Result focus");
    assert!(
        err.message.contains("expected Result<Int>, got Int"),
        "{}",
        err.message
    );
}

fn facet_shorthand_view_and_mutation_forms_typecheck() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String, score: Result<Int>)
user = User("alice", Ok(1))
name = Facet::view(~user.name)
updated =? Facet::set(~user.name, "bob")
replaced = put(~updated.name, "carol")
bumped =? Facet::over(~replaced.score, {|score| Ok(score + 1)})
Facet::over_result(~bumped.score, {|score| Ok(score)})"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.node, TypedInner::FacetOver { .. }));
}

fn facet_shorthand_reuses_existing_facet_api_errors() {
    let preview_err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = User("alice")
Facet::preview(~user.name)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("structural shorthand should fail for preview");
    assert!(preview_err
        .message
        .contains("Facet::preview requires a variant Facet"));

    let put_err = typecheck_with_rules(
        r#"defrecord User(name: String)
result_user = Ok(User("alice"))
put(~result_user.name, "bob")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("result source shorthand should fail for put");
    assert!(put_err
        .message
        .contains("Facet::put requires a plain source value"));
}

fn facet_shorthand_misuse_is_rejected_outside_facet_api() {
    let bind_err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = User("alice")
path = ~user.name"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("shorthand binding should fail");
    assert!(bind_err.message.contains(
        "must be consumed as the first argument of Facet::view/preview/put/set/over/over_result"
    ));

    let missing_path_err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = User("alice")
Facet::view(~user)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("missing path should fail");
    assert!(missing_path_err
        .message
        .contains("requires a field or tuple path"));
}

fn facet_over_accepts_success_updater_for_result_focus() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(score: Result<Int>)
user = User(Ok(1))
Facet::over(User.score, user, {|score| Ok(score + 1)})"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.node, TypedInner::FacetOver { .. }));
}

fn facet_over_allows_result_typed_payload_replacement() {
    typecheck_with_rules(
        r#"defrecord User(score: Result<Int>)
user = User(Ok(1))
Facet::over(User.score, user, {|score| Ok(Ok(score))})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("Facet::over should permit changing a Result payload to a Result value");
}

fn facet_over_result_requires_result_container_updater() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(score: Result<Int>)
user = User(Ok(1))
Facet::over_result(User.score, user, {|score| Ok(score)})"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.node, TypedInner::FacetOver { .. }));

    let err = typecheck_with_rules(
        r#"defrecord User(score: Result<Int>)
user = User(Ok(1))
Facet::over_result(User.score, user, {|score| Ok(1)})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("plain success updater should fail for Facet::over_result");
    assert!(err
        .message
        .contains("Facet::over_result update function output mismatch"));
}

fn readonly_facet_view_succeeds_and_preserves_path_metadata() {
    let typed = typecheck_with_builtin_prelude(
        r#"defstruct Profile {
  name: String,
}

defstruct User {
  readonly profile: Profile,
}

impl Profile {
  def new(name: String) -> Self {
    Profile { name: name }
  }
}

impl User {
  def new(profile: Profile) -> Self {
    User { profile: profile }
  }
}

user = User(Profile("alice"))
Facet::view(User.profile.name, user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    let TypedInner::FacetView { path, .. } = &last.node else {
        panic!("expected FacetView");
    };
    assert!(matches!(last.ty, scar::types::Ty::Str));
    match &path.segments[0] {
        TypedFacetSegment::Field {
            readonly,
            container_type_name,
            ..
        } => {
            assert!(*readonly);
            assert_eq!(container_type_name, "User");
        }
        other => panic!("expected field segment, got {other:?}"),
    }
}

fn readonly_field_blocks_deep_mutation_but_owner_can_replace_property() {
    let err = typecheck_with_rules(
        r#"defstruct Profile {
  name: String,
}

defstruct User {
  readonly profile: Profile,
}

impl Profile {
  def new(name: String) -> Self {
    Profile { name: name }
  }
}

impl User {
  def new(profile: Profile) -> Self {
    User { profile: profile }
  }
}

user = User(Profile("alice"))
Facet::set(User.profile.name, user, "bob")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("deep mutation through readonly field should fail");
    assert!(err.message.contains("readonly field User.profile"));

    let typed = typecheck_with_builtin_prelude(
        r#"defstruct Profile {
  name: String,
}

defstruct User {
  readonly profile: Profile,
}

impl Profile {
  def new(name: String) -> Self {
    Profile { name: name }
  }
}

impl User {
  def new(profile: Profile) -> Self {
    User { profile: profile }
  }

  def replace_profile(self: Self, next_profile: Profile) -> Result<User> {
    Facet::set(User.profile, self, next_profile)
  }
}"#,
    );
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Def(..))
    ));
}

fn readonly_struct_root_blocks_mutating_facet_even_for_owner() {
    let err = typecheck_with_rules(
        r#"@readonly
defstruct Profile {
  name: String,
}

impl Profile {
  def new(name: String) -> Self {
    Profile { name: name }
  }
}

profile = Profile("alice")
Facet::over(Profile.name, profile, {|name| Ok(name)})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("readonly root should reject mutating facet");
    assert!(err.message.contains("readonly type Profile"));

    let err = typecheck_with_rules(
        r#"@readonly
defstruct Profile {
  name: String,
}

impl Profile {
  def new(name: String) -> Self {
    Profile { name: name }
  }

  def rename(self: Self, next_name: String) -> Result<Profile> {
    Facet::set(Profile.name, self, next_name)
  }
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("readonly root should also reject owner mutation");
    assert!(err.message.contains("readonly type Profile"));
}

fn facet_standalone_tuple_root_is_rejected() {
    let err = resolve_with_builtin_prelude_result(
        r#"pair = (1, "one")
Facet::view(_0, pair)"#,
    )
    .expect_err("standalone tuple root should fail during resolve");
    assert!(err.message.contains("Undefined variable: _0"));
}

fn facet_bindings_can_be_reused_by_facet_intrinsics() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
facet = User.name
Facet::view(facet, user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::FacetView { .. }));
}

fn facet_tuple_type_root_view_works_with_expected_context() {
    let typed = typecheck_with_builtin_prelude(
        r#"pair = ("alice", 42)
Facet::view(Tuple._0, pair)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::FacetView { .. }));
}

fn deferred_tuple_facet_binding_can_be_reused_by_facet_intrinsics() {
    let typed = typecheck_with_builtin_prelude(
        r#"pair = ("alice", 42)
facet = Tuple._1
Facet::view(facet, pair)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Int));
    assert!(matches!(last.node, TypedInner::FacetView { .. }));
}

fn deferred_tuple_facet_binding_can_compose_before_consumption() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Profile(name: String)
pair = (Profile("alice"), 42)
outer = Tuple._0
path = outer / Profile.name
Facet::view(path, pair)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::FacetView { .. }));
}

fn facet_tuple_type_root_compose_works_as_inner_path() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(pair: (String, Int))
user = User(("alice", 42))
Facet::view(Facet::chain(User.pair, Tuple._0), user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

fn facet_tuple_type_root_slash_compose_works_as_inner_path() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(pair: (String, Int))
user = User(("alice", 42))
Facet::view(User.pair / Tuple._0, user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

fn facet_const_slash_compose_allows_facet_consts() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
const USER_PROFILE: Facet<InfallibleStructural, User, Profile, _, _> = User.profile
const PROFILE_NAME: Facet<InfallibleStructural, Profile, String, _, _> = Profile.name
const FULL_NAME: Facet<InfallibleStructural, User, String, _, _> = USER_PROFILE / PROFILE_NAME
user = User(Profile("alice"))
Facet::view(FULL_NAME, user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

fn facet_const_slash_compose_rejects_non_facet_const_refs() {
    let err = typecheck_with_rules(
        r#"const VALUE = 1
const BAD = VALUE / VALUE"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("non-facet const refs should fail");
    assert!(err
        .message
        .contains("const value must be a primitive literal or a facet path"));
}

fn slash_operator_rejects_numeric_division_and_points_to_safe_div() {
    let err = typecheck_with_rules(r#"print(to_string(10 / 3))"#, RuntimeSourcePolicy::script())
        .expect_err("numeric infix slash should fail");
    assert!(err.message.contains("`/` requires Compose implementation"));
    assert!(err
        .hint
        .as_deref()
        .is_some_and(|hint| hint.contains("Int::safe_div")));
}

fn facet_tuple_type_root_without_context_can_bind_as_deferred_path() {
    let typed = typecheck_with_builtin_prelude("facet = Tuple._0");
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Unit));
}

fn facet_view_inside_closure_is_allowed_for_same_scope_consumption() {
    let typed = typecheck_with_rules(
        r#"defrecord User(name: String)
facet = User.name
getter = {|user| Facet::view(facet, user)}
getter(User("alice"))"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("closure-local Facet::view should typecheck");
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

fn facet_capture_shorthand_builds_read_closure() {
    let typed = typecheck_with_rules(
        r#"defrecord User(name: String)
facet = User.name
getter = &facet
getter(User("alice"))"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("&facet shorthand should typecheck");
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

fn facet_values_cannot_be_embedded_in_runtime_containers() {
    let tuple_err = typecheck_with_rules(
        r#"defrecord User(name: String)
(User.name, 1)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("tuple literal should reject facet");
    assert!(tuple_err
        .message
        .contains("Tuple literal cannot contain Facet values"));

    let list_err = typecheck_with_rules(
        r#"defrecord User(name: String)
[User.name, User.name]"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("list literal should reject facet");
    assert!(list_err
        .message
        .contains("List literal cannot contain Facet values"));

    let ok_err = typecheck_with_rules(
        r#"defrecord User(name: String)
Ok(User.name)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("result constructors should reject facet");
    assert!(ok_err
        .message
        .contains("Result constructors cannot contain Facet values"));
}

fn nested_facet_types_are_rejected_in_function_signatures() {
    let param_err = typecheck_with_rules(
        r#"defrecord User(name: String)
def bad(values: List<Facet<InfallibleStructural, User, String, _, _>>) -> Unit { () }"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("nested facet in parameter type should fail");
    assert!(param_err
        .message
        .contains("cannot appear in function parameter types"));

    let ret_err = typecheck_with_rules(
        r#"defrecord User(name: String)
def bad() -> List<Facet<InfallibleStructural, User, String, _, _>> { [] }"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("nested facet in return type should fail");
    assert!(ret_err
        .message
        .contains("cannot appear in function return types"));
}

fn private_field_access_is_allowed_inside_owner_impl_only() {
    let typed = typecheck_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }

  def read_password(self) -> String {
    self.password
  }
}
user = User("alice", "s3cr3t")
User::read_password(user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

fn private_field_access_outside_owner_impl_is_rejected_for_value_and_capability_roots() {
    let value_err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
user = User("alice", "s3cr3t")
user.password"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private value access should fail outside impl");
    assert!(value_err
        .message
        .contains("Field 'User.password' is private"));

    let capability_err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
User.password"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private capability root should fail");
    assert!(capability_err
        .message
        .contains("Field 'User.password' is private"));
}

fn private_field_access_inside_closure_is_rejected_outside_owner_impl() {
    let err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
user = User("alice", "s3cr3t")
{|| user.password}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private value access inside closure should fail outside impl");
    assert!(err.message.contains("Field 'User.password' is private"));
}

fn private_field_access_inside_param_closure_is_rejected_outside_owner_impl() {
    let err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
reader = {|user: User| user.password}
user = User("alice", "s3cr3t")
reader(user)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private value access inside parameter closure should fail outside impl");
    assert!(err.message.contains("Field 'User.password' is private"));
}

fn private_capability_root_is_rejected_in_facet_view_call() {
    let err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
user = User("alice", "s3cr3t")
Facet::view(User.password, user)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private capability root in Facet::view should fail");
    assert!(err.message.contains("Field 'User.password' is private"));
}

fn facet_scope_local_value_can_flow_to_closure_after_view() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
facet = User.name
name = Facet::view(facet, user)
reader = {|| name}
reader()"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

fn facet_runtime_transport_restrictions_remain() {
    let arg_err = typecheck_with_rules(
        r#"defrecord User(name: String)
print(to_string(User.name))"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("passing Facet value as argument should fail");
    assert!(arg_err.message.contains("cannot accept Facet values"));

    let return_err = typecheck_with_rules(
        r#"defrecord User(name: String)
def bad() -> Facet<InfallibleStructural, User, String, _, _> {
  User.name
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("returning Facet value should fail");
    assert!(return_err
        .message
        .contains("cannot appear in function return types"));

    let arg_var_err = typecheck_with_rules(
        r#"defrecord User(name: String)
def consume(value: String) -> String {
  value
}
facet = User.name
consume(facet)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("passing Facet binding as runtime function argument should fail");
    assert!(
        arg_var_err.message.contains("cannot accept Facet values")
            || arg_var_err.message.contains("Argument type mismatch")
            || arg_var_err.message.contains("compile-time only")
    );
}

fn extractor_single_value_match_result_contract_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Single {
  value: Int,
}
impl Single {
  def new(value: Int) -> Self {
Single { value: value }
  }

  defextractor deconstruct(self: Self) -> Option<Int> {
Option::Some(self.value)
  }
}

value = Single(1)
print(match value {
  Single(inner) => to_string(inner),
  _ => "bad",
})"#,
    );
    let typed = typecheck(resolved).expect("single-value extractor should typecheck");
    assert!(!typed.is_empty());
}

fn struct_matchblock_head_uses_attached_deconstruct_method() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age }
  }
  defextractor deconstruct(self: Self) -> Option<(String, Int)> {
Option::None
  }
}
user = User("alice", 30)
print(match user {
  User(name, age) => "bad",
  _ => "fallback",
})"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(!typed.is_empty());
}

fn struct_matchblock_head_requires_attached_deconstruct_method() {
    let err = resolve_with_builtin_prelude_result(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
User { name: name }
  }
}
user = User("alice")
print(match user {
  User(name) => name,
  _ => "fallback",
})"#,
    )
    .expect_err("resolve should fail");
    assert!(err.message.contains(
        "MatchBlock head `User` requires attached extractor `User::deconstruct`, but it is not defined"
    ));
}

fn enum_impl_extractor_can_be_used_in_matchblock() {
    let resolved = resolve_with_builtin_prelude(
        r#"defenum Light {
  Red,
  Green,
}
impl Light {
  defextractor stop_code(self: Self) -> Option<Int> {
match self {
  Light::Red => Option::Some(1),
  _ => Option::None,
}
  }
}
light = Light::Red
print(match light {
  Light::stop_code(code) => to_string(code),
  _ => "fallback",
})"#,
    );
    let typed = typecheck(resolved).expect("enum impl extractor should typecheck");
    assert!(!typed.is_empty());
}

fn forward_struct_type_annotation_and_literal_are_allowed() {
    let resolved = resolve_with_builtin_prelude(
        r#"user: User = User("alice", 30)
defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age }
  }
}"#,
    );
    let typed = typecheck(resolved).expect("forward struct reference should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::StructDef(_, _, _, _, _))));
}

fn generic_struct_single_type_param_typechecks() {
    let typed = typecheck_with_builtin_prelude(
        r#"defstruct Box<$A> {
  value: $A,
}
impl Box {
  def new<$A>(value: $A) -> Box<$A> {
    Box { value: value }
  }
}
boxed: Box<Int> = Box(41)
printable: Int = boxed.value"#,
    );
    let boxed_bind = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(TypedPattern::Var(ty, id), rhs) if id.name == "boxed" => {
                Some((ty, rhs.as_ref()))
            }
            _ => None,
        })
        .expect("expected boxed binding");
    assert!(
        matches!(boxed_bind.0, Ty::Struct(name, fields) if name == "Global::Box"
        && matches!(fields.as_slice(), [(field, Ty::Int)] if field == "value"))
    );
    assert!(
        matches!(boxed_bind.1.ty, Ty::Struct(ref name, ref fields) if name == "Global::Box"
        && matches!(fields.as_slice(), [(field, Ty::Int)] if field == "value"))
    );
}

fn generic_struct_two_type_params_typecheck() {
    let typed = typecheck_with_builtin_prelude(
        r#"defstruct Pair<$A, $B> {
  left: $A,
  right: $B,
}
impl Pair {
  def new<$A, $B>(left: $A, right: $B) -> Pair<$A, $B> {
    Pair { left: left, right: right }
  }
}
pair: Pair<Int, String> = Pair(1, "two")
text: String = pair.right"#,
    );
    let pair_bind = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(TypedPattern::Var(ty, id), rhs) if id.name == "pair" => {
                Some((ty, rhs.as_ref()))
            }
            _ => None,
        })
        .expect("expected pair binding");
    assert!(
        matches!(pair_bind.0, Ty::Struct(name, fields) if name == "Global::Pair"
        && matches!(fields.as_slice(),
            [(left, Ty::Int), (right, Ty::Str)] if left == "left" && right == "right"))
    );
    assert!(
        matches!(pair_bind.1.ty, Ty::Struct(ref name, ref fields) if name == "Global::Pair"
        && matches!(fields.as_slice(),
            [(left, Ty::Int), (right, Ty::Str)] if left == "left" && right == "right"))
    );
}

fn forward_deferror_value_can_flow_into_err() {
    let resolved = resolve_with_builtin_prelude(
        r#"ret: Result<Int> = Err(NotFound)
deferror NotFound {
  "not found"
}"#,
    );
    let typed = typecheck(resolved).expect("forward deferror constructor should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::DeferrorDef(_, _, _, _, _))));
}

fn zero_arg_deferror_value_can_flow_into_error_parameter() {
    let resolved = resolve_with_builtin_prelude(
        r#"wrapped = Result::cause(Err(NoneError), NotFound)
deferror NotFound {
  "not found"
}"#,
    );
    let typed = typecheck(resolved).expect("zero-arg deferror should satisfy Error parameters");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Bind(_, _))));
}

fn recover_kind_constructor_marker_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = Result::recover_kind(Err(NotFound("runtime")), NotFound("marker"), {|err| Ok(1)})
deferror NotFound(detail: String) {
  detail
}"#,
    );
    let typed = typecheck(resolved).expect("recover_kind constructor marker should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Bind(_, _))));
}

fn forward_reference_type_tags_are_deterministic_across_runs() {
    let source = r#"user: User = User("alice", 30)
pair = Pair(first: 1, second: "two")
ret: Result<Int> = Err(NotFound("404"))

defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age }
  }
}

defrecord Pair(first: Int, second: String)

deferror NotFound(code: String) {
  "missing #{code}"
}"#;

    let first = typecheck_with_builtin_prelude(source);
    let second = typecheck_with_builtin_prelude(source);

    fn collect_type_tags(nodes: &[TypedNode]) -> Vec<(String, u32)> {
        nodes
            .iter()
            .filter_map(|node| match &node.node {
                TypedInner::StructDef(tag, name, _, _, _)
                | TypedInner::RecordDef(tag, name, _, _, _) => Some((name.clone(), *tag)),
                TypedInner::DeferrorDef(tag, _, id, _, _) => Some((id.name.clone(), *tag)),
                _ => None,
            })
            .collect()
    }

    assert_eq!(collect_type_tags(&first), collect_type_tags(&second));
}

fn user_function_calls_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        "def add1(x: Int) -> Int { x + 1 }\nprint(to_string(add1(41)))",
    );
    assert!(
        typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::Def(..))),
        "expected user function definition to survive typechecking"
    );
}

fn namespaced_type_and_trait_impl_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"namespace Auth {
  defrecord User(name: String)
}

impl Show for Auth::User {
  def to_string(self: Self) -> String { "user" }
}

value: Auth::User = Auth::User("alice")
print(to_string(value))"#,
    );
    assert!(
        typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::RecordDef(_, _, _, _, _))),
        "expected namespaced record definition to survive typechecking"
    );
    assert!(
        typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::TraitImplDef(..))),
        "expected namespaced trait impl to survive typechecking"
    );
}

fn tuple_trait_impl_typechecks_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"deftrait PairTrait {
  def keep(self: Self) -> Self
}

impl PairTrait for ($A, $B) {
  def keep(self: Self) -> Self {
    self
  }
}"#,
    );
    assert!(
        typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::TraitImplDef(..))),
        "expected tuple trait impl to survive typechecking"
    );
}

fn concrete_tuple_trait_impl_typechecks_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"deftrait PairTrait {
  def keep(self: Self) -> Self
}

impl PairTrait for (Int, String) {
  def keep(self: Self) -> Self {
    self
  }
}"#,
    );
    assert!(
        typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::TraitImplDef(..))),
        "expected concrete tuple trait impl to survive typechecking"
    );
}

fn generic_user_function_calls_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"def id(x: $A) -> $A { x }

left: Int = id(1)
right: String = id("ok")
print(to_string(left))
print(right)"#,
    );
    assert!(matches!(typed_bind_rhs(&typed, "left").ty, Ty::Int));
    assert!(matches!(typed_bind_rhs(&typed, "right").ty, Ty::Str));
    assert!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::App(_, _)))
            .count()
            >= 2,
        "expected both generic function call sites to typecheck"
    );
}

#[test]
fn where_constraint_kinds_survive_in_typed_metadata() {
    let typed = typecheck_with_rules(
        r#"deftrait Marker
where
  Self: Type<$Slot>
{
  def mark(self: Self) -> Self
  where
    Self: Marker
}

defenum Boxed<$T> {
  Box($T),
}

impl Marker for Boxed<$T>
where
  $T: Marker.$Slot
{
  def mark(self: Self) -> Self
  where
    $T: Marker
  {
    self
  }
}

def keep(value: $A) -> $A
where
  $A: Marker + Type<$B> + Marker.$Slot
{
  value
}

kept: Boxed<Int> = keep(Boxed::Box(1))
marked: Boxed<Int> = Marker::mark(Boxed::Box(1))"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("Step 4 records constraints without applying their later semantics");

    let where_clause = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Def(_, id, _, _, _, where_clause, _, _) if id.name == "keep" => {
                where_clause.as_ref()
            }
            _ => None,
        })
        .expect("typed def should retain its where clause");
    let bounds = &where_clause.constraints[0].bounds;
    assert!(matches!(bounds[0], TypedWhereConstraintRhs::Trait(_)));
    assert!(matches!(
        bounds[1],
        TypedWhereConstraintRhs::TypeConstructor { .. }
    ));
    assert!(matches!(
        bounds[2],
        TypedWhereConstraintRhs::TraitSlot { .. }
    ));

    let (trait_where, trait_methods) = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::TraitDef(name, where_clause, methods) if name.ends_with("Marker") => {
                Some((where_clause.as_ref(), methods))
            }
            _ => None,
        })
        .expect("typed trait should retain metadata");
    assert!(trait_where.is_some());
    assert!(trait_methods[0].where_clause.is_some());
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::TraitImplDef(_, _, Some(_)))));
    assert!(typed.iter().any(|node| match &node.node {
        TypedInner::Def(_, id, _, _, _, Some(_), _, _) => id.name.contains("mark"),
        _ => false,
    }));
}

#[test]
fn function_where_bounds_propagate_to_generic_call_sites() {
    let source = r#"deftrait Default {
  def default() -> Self
}

impl Default for String {
  def default() -> String {
    ""
  }
}

def make() -> $A
where
  $A: Default
{
  Default::default()
}
"#;

    typecheck_with_rules(
        &format!("{source}\nvalue: String = make()"),
        RuntimeSourcePolicy::script(),
    )
    .expect("a call target with the declared where-bound implementation should typecheck");

    let err = typecheck_with_rules(
        &format!("{source}\nvalue: Int = make()"),
        RuntimeSourcePolicy::script(),
    )
    .expect_err("the generic call must retain and enforce its where bound");
    assert!(
        err.message.contains("expected Int") || err.message.contains("Default"),
        "unexpected diagnostic: {err:?}"
    );
}

#[test]
fn where_clause_does_not_declare_a_new_type_variable() {
    let err = typecheck_with_rules(
        r#"deftrait Default {
  def default() -> Self
}

def id(value: $A) -> $A
where
  $B: Default
{
  value
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("a where-only type variable must not be introduced implicitly");

    assert!(err
        .message
        .contains("does not appear in the declaration signature"));
    assert!(err
        .hint
        .as_deref()
        .is_some_and(|hint| hint.contains("do not declare type variables")));
}

#[test]
fn functor_shaped_self_applications_survive_in_typed_metadata() {
    let typed = typecheck_with_rules(
        r#"deftrait FunctorShape
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("Step 4 should preserve higher-kinded Self applications");

    let method = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::TraitDef(name, _, methods) if name.ends_with("FunctorShape") => {
                methods.iter().find(|method| method.name == "fmap")
            }
            _ => None,
        })
        .expect("typed trait metadata should retain fmap");
    assert!(matches!(method.params.first(), Some(Ty::SelfApp(args)) if args.len() == 1));
    assert!(matches!(method.ret_ty, Ty::SelfApp(ref args) if args.len() == 1));
}

#[test]
fn unary_type_constructor_slot_is_inferred_for_trait_impl() {
    typecheck_with_rules(
        r#"deftrait FunctorShape
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

defenum Identity<$T> {
  Identity($T),
}

impl FunctorShape for Identity<$T> {
  def fmap(self: Identity<$A>, mapper: ($A -> $B)) -> Identity<$B> {
    match self {
      Identity::Identity(value) => Identity::Identity(mapper(value)),
    }
  }
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("a unary impl target should map its only parameter to the trait slot");
}

#[test]
fn multi_parameter_type_constructor_requires_explicit_slot_mapping() {
    let source = r#"deftrait FunctorShape
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

defenum Pair<$L, $R> {
  Pair($L, $R),
}

impl FunctorShape for Pair<$L, $R> {
  def fmap(self: Pair<$L, $A>, mapper: ($A -> $B)) -> Pair<$L, $B> {
    match self {
      Pair::Pair(left, right) => Pair::Pair(left, mapper(right)),
    }
  }
}"#;
    let err = typecheck_with_rules(source, RuntimeSourcePolicy::script())
        .expect_err("a multi-parameter target must name the public constructor slot");
    assert!(err.message.contains("does not satisfy Type<$A>"), "{err:?}");

    typecheck_with_rules(
        &source.replace(
            "impl FunctorShape for Pair<$L, $R> {",
            "impl FunctorShape for Pair<$L, $R>\nwhere\n  $R: FunctorShape.$A\n{",
        ),
        RuntimeSourcePolicy::script(),
    )
    .expect("an explicit slot mapping should preserve the left capture parameter");
}

#[test]
fn type_constructor_constraint_rejects_concrete_and_duplicate_slot_targets() {
    let trait_source = r#"deftrait FunctorShape
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}
"#;
    let concrete = format!(
        "{trait_source}\nimpl FunctorShape for Int {{\n  def fmap(self: Int, mapper: (Int -> Int)) -> Int {{ self }}\n}}"
    );
    let err = typecheck_with_rules(&concrete, RuntimeSourcePolicy::script())
        .expect_err("a concrete type is not a unary type constructor");
    assert!(err.message.contains("does not satisfy Type<$A>"), "{err:?}");

    let duplicate = format!(
        r#"{trait_source}
defenum Pair<$L, $R> {{
  Pair($L, $R),
}}
impl FunctorShape for Pair<$L, $R>
where
  $L: FunctorShape.$A
  $R: FunctorShape.$A
{{
  def fmap(self: Pair<$L, $A>, mapper: ($A -> $B)) -> Pair<$L, $B> {{ self }}
}}"#
    );
    let err = typecheck_with_rules(&duplicate, RuntimeSourcePolicy::script())
        .expect_err("one constructor slot cannot map to two target parameters");
    assert!(err.message.contains("mapped more than once"), "{err:?}");
}

#[test]
fn parent_trait_constraints_require_matching_impls_and_inherit_slots() {
    let declarations = r#"deftrait Parent
where
  Self: Type<$A>
{
  def keep(self: Self<$A>) -> Self<$A>
}

deftrait Child
where
  Self: Parent
{
  def child_keep(self: Self<$A>) -> Self<$A>
}

defenum Identity<$T> {
  Identity($T),
}
"#;
    let child_impl = r#"impl Child for Identity<$T> {
  def child_keep(self: Identity<$A>) -> Identity<$A> { self }
}"#;
    let err = typecheck_with_rules(
        &format!("{declarations}\n{child_impl}"),
        RuntimeSourcePolicy::script(),
    )
    .expect_err("a child impl cannot omit its parent impl");
    assert!(
        err.message.contains("requires parent impl Parent"),
        "{err:?}"
    );

    typecheck_with_rules(
        &format!(
            r#"{declarations}
impl Parent for Identity<$T> {{
  def keep(self: Identity<$A>) -> Identity<$A> {{ self }}
}}
{child_impl}"#
        ),
        RuntimeSourcePolicy::script(),
    )
    .expect("the child should inherit the parent's unary constructor slot");
}

#[test]
fn parent_trait_cycles_and_slot_mapping_mismatches_are_rejected() {
    let cycle = r#"deftrait Left
where
  Self: Right
{
  def left(self: Self) -> Self
}

deftrait Right
where
  Self: Left
{
  def right(self: Self) -> Self
}"#;
    let err = typecheck_with_rules(cycle, RuntimeSourcePolicy::script())
        .expect_err("parent trait cycles must be rejected");
    assert!(err.message.contains("constraint cycle"), "{err:?}");

    let mismatch = r#"deftrait Parent
where
  Self: Type<$A>
{
  def keep(self: Self<$A>) -> Self<$A>
}

deftrait Child
where
  Self: Parent
{
  def child_keep(self: Self<$A>) -> Self<$A>
}

defenum Pair<$L, $R> {
  Pair($L, $R),
}

impl Parent for Pair<$L, $R>
where
  $R: Parent.$A
{
  def keep(self: Pair<$L, $A>) -> Pair<$L, $A> { self }
}

impl Child for Pair<$L, $R>
where
  $L: Child.$A
{
  def child_keep(self: Pair<$A, $R>) -> Pair<$A, $R> { self }
}"#;
    let err = typecheck_with_rules(mismatch, RuntimeSourcePolicy::script())
        .expect_err("a child must preserve its parent's slot mapping");
    assert!(
        err.message.contains("same constructor slot mapping"),
        "{err:?}"
    );
}

#[test]
fn trait_impl_signature_validation_preserves_generic_relationships() {
    let err = typecheck_with_rules(
        r#"deftrait Pick {
  def pick(left: $A, right: $B) -> $A
}

impl Pick for Int {
  def pick(left: $A, right: $B) -> $B { right }
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("an impl cannot permute independent method generics");
    assert!(err.message.contains("incompatible signature"), "{err:?}");
}

#[test]
fn trait_impl_method_generics_are_rigid_while_checking_the_body() {
    let err = typecheck_with_rules(
        r#"deftrait Keep {
  def keep(value: $A) -> $A
}

impl Keep for Int {
  def keep(value: $A) -> $A { "wrong" }
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("an impl body cannot specialize its method generic to String");
    assert!(
        err.message.contains("expected $") && err.message.contains("got String"),
        "{err:?}"
    );
}

fn rigid_generic_return_rejects_concrete_body() {
    let err = typecheck_with_rules(
        r#"def nil() -> $A {
  ""
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("a concrete body must not satisfy a rigid generic return type");
    assert!(
        err.message.contains("expected $") && err.message.contains("got String"),
        "unexpected error: {}",
        err.message
    );

    typecheck_with_builtin_prelude(r#"def id(value: $A) -> $A { value }"#);
}

fn signature_generics_are_rigid_while_definition_body_is_checked() {
    let err = typecheck_with_rules(
        r#"def wrong(value: $A) -> $B {
  value
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("independent signature generics must not unify in the definition body");
    assert!(
        err.message.contains("expected $") && err.message.contains(", got $"),
        "unexpected error: {}",
        err.message
    );

    typecheck_with_rules(
        r#"def identity(value: $A) -> $A { value }

def gen_nil() -> List<$A> { [] }"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("matching rigid generics and generic container slots remain valid");
}

fn named_args_user_function_calls_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"def add(x: Int, y: Int) -> Int { x + y }
def add3(x: Int, y: Int, z: Int) -> Int { x + y + z }

print(to_string(add(y: 2, x: 1)))
print(to_string(add3(z: 3, y: 2, x: 1)))"#,
    );
    assert!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::Def(..)))
            .count()
            >= 2,
        "expected named-argument user functions to typecheck"
    );
}

fn canonical_builtin_type_name_hole_is_reserved_for_structs() {
    let err = typecheck_module_source_result(
        r#"defstruct Hole {
  value: Int,
}"#,
    )
    .expect_err("Hole should be reserved");
    assert!(
        err.contains("Type name `Hole` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

fn canonical_builtin_type_name_hole_is_reserved_for_enums() {
    let err = typecheck_module_source_result(
        r#"defenum Hole {
  Filled,
}"#,
    )
    .expect_err("Hole should be reserved");
    assert!(
        err.contains("Type name `Hole` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

fn canonical_builtin_type_name_hole_is_reserved_for_errors() {
    let err = typecheck_module_source_result(
        r#"deferror Hole {
  "reserved"
}"#,
    )
    .expect_err("Hole should be reserved");
    assert!(
        err.contains("Type name `Hole` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

fn canonical_builtin_type_name_closure_is_reserved_for_structs() {
    let err = typecheck_module_source_result(
        r#"defstruct Closure {
  value: Int,
}"#,
    )
    .expect_err("Closure should be reserved");
    assert!(
        err.contains("Type name `Closure` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

fn canonical_builtin_type_name_match_arms_is_reserved_for_structs() {
    let err = typecheck_module_source_result(
        r#"defstruct MatchArms {
  value: Int,
}"#,
    )
    .expect_err("MatchArms should be reserved");
    assert!(
        err.contains("Type name `MatchArms` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

fn canonical_builtin_type_name_cond_clauses_is_reserved_for_enums() {
    let err = typecheck_module_source_result(
        r#"defenum CondClauses {
  Clause,
}"#,
    )
    .expect_err("CondClauses should be reserved");
    assert!(
        err.contains("Type name `CondClauses` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

fn match_arms_type_is_forbidden_in_ordinary_user_signatures() {
    let err = typecheck_with_rules(
        r#"def bad(arms: MatchArms<Int, String>) -> String {
  "nope"
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("MatchArms should be restricted to special-form signatures");
    assert!(
        err.message
            .contains("MatchArms<$Scrutinee, $Result> is reserved for the `match` special form"),
        "unexpected error: {err}"
    );
}

fn match_arms_type_is_forbidden_in_return_types() {
    let err = typecheck_with_rules(
        r#"def bad() -> MatchArms<Int, String> {
  "nope"
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("MatchArms should be restricted to special-form return types");
    assert!(
        err.message
            .contains("MatchArms<$Scrutinee, $Result> is reserved for the `match` special form"),
        "unexpected error: {err}"
    );
}

fn cond_clauses_type_is_forbidden_in_ordinary_user_signatures() {
    let err = typecheck_with_rules(
        r#"def bad(clauses: CondClauses<String>) -> String {
  "nope"
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("CondClauses should be restricted to special-form signatures");
    assert!(
        err.message
            .contains("CondClauses<$Result> is reserved for the `cond` special form"),
        "unexpected error: {err}"
    );
}

fn cond_clauses_type_is_forbidden_in_return_types() {
    let err = typecheck_with_rules(
        r#"def bad() -> CondClauses<String> {
  "nope"
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("CondClauses should be restricted to special-form return types");
    assert!(
        err.message
            .contains("CondClauses<$Result> is reserved for the `cond` special form"),
        "unexpected error: {err}"
    );
}

fn trailing_block_calls_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"def take(flag: Boolean, value: (-> Int)) -> Int {
  if(flag, value(), 0)
}

print(to_string(take(True) { num = 10; num }))

v = if_then(True) { print("x") }
print(to_string(v))"#,
    );
    assert!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::App(_, _)))
            .count()
            >= 2,
        "expected trailing-block call sites to typecheck"
    );
}

fn set_exit_code_is_allowed_in_script_rules() {
    let typed =
        typecheck_with_rules("set_exit_code(9)", RuntimeSourcePolicy::script()).expect("must pass");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::App(_, _))
    ));
}

fn set_exit_code_is_forbidden_in_repl_chunk_rules() {
    let err = typecheck_with_rules("set_exit_code(9)", RuntimeSourcePolicy::repl_chunk())
        .expect_err("must fail");
    assert!(err.message.contains("forbidden by source policy"));
}

fn set_exit_code_entry_only_policy_allows_only_entrypoint_function() {
    let entrypoint = EntryPoint::qualified("main");
    let rules = RuntimeSourcePolicy::module()
        .with_exit_code_policy(ExitCodePolicy::EntryOnly, Some(&entrypoint));

    let ok = typecheck_with_rules(
        r#"def main() -> Result<()> {
  set_exit_code(7)
  Ok(())
}"#,
        rules.clone(),
    )
    .expect("entrypoint body should allow set_exit_code");
    assert!(ok
        .iter()
        .find(|node| matches!(node.node, TypedInner::Def(..)))
        .is_some());

    let err = typecheck_with_rules(
        r#"def helper() -> Result<()> {
  set_exit_code(7)
  Ok(())
}"#,
        rules,
    )
    .expect_err("non-entrypoint function must fail");
    assert!(err.message.contains("only allowed inside entrypoint"));
}

fn assert_special_form_typechecks_to_result_unit() {
    let typed = typecheck_with_builtin_prelude("guard = assert(True, NoneError())");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(rhs.node, TypedInner::Assert(_, _)));
            assert!(matches!(
                rhs.ty,
                scar::types::Ty::Result(ref ok, ref err)
                    if matches!(ok.as_ref(), scar::types::Ty::Unit)
                        && matches!(err.as_ref(), scar::types::Ty::Error)
            ));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

fn bitwidth_zero_arg_variant_reference_reuses_std_enum_constructor_uid() {
    let resolved = resolve_program_with_builtin_prelude("width = BitWidth::W8");

    let use_uid = match resolved
        .last()
        .expect("user bind should be present after std modules")
    {
        sigil::resolved::Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            sigil::resolved::Resolved::ConstructorCall(_, id, args) => {
                assert!(args.is_empty(), "W8 should be zero-arg");
                id.unique_id
            }
            other => panic!("expected zero-arg constructor call, got {other:?}"),
        },
        other => panic!("expected user bind, got {other:?}"),
    };

    let variant_uid = resolved
        .iter()
        .find_map(|node| match node {
            sigil::resolved::Resolved::EnumDef(_, id, _, variants, _)
                if id.name == "BitWidth" || id.name == "Global::BitWidth" =>
            {
                variants
                    .iter()
                    .find(|variant| {
                        variant.id.name == "BitWidth::W8"
                            || variant.id.name == "Global::BitWidth::W8"
                    })
                    .map(|variant| variant.id.unique_id)
            }
            _ => None,
        })
        .expect("BitWidth::W8 variant should exist");

    assert_eq!(use_uid, variant_uid);

    let colliding_defs = resolved
        .iter()
        .filter_map(|node| match node {
            sigil::resolved::Resolved::BuiltinDecl(_, id, _, _, _) if id.unique_id == use_uid => {
                Some(format!("builtin {}", id.name))
            }
            sigil::resolved::Resolved::Def(_, id, _, _, _, _, _, _) if id.unique_id == use_uid => {
                Some(format!("def {}", id.name))
            }
            sigil::resolved::Resolved::ExtractorDef(_, id, _, _, _, _, _)
                if id.unique_id == use_uid =>
            {
                Some(format!("extractor {}", id.name))
            }
            sigil::resolved::Resolved::StructDef(_, id, ..) if id.unique_id == use_uid => {
                Some(format!("struct {}", id.name))
            }
            sigil::resolved::Resolved::RecordDef(_, id, _) if id.unique_id == use_uid => {
                Some(format!("record {}", id.name))
            }
            sigil::resolved::Resolved::DeferrorDef(_, id, _, _) if id.unique_id == use_uid => {
                Some(format!("deferror {}", id.name))
            }
            sigil::resolved::Resolved::EnumDef(_, _, _, variants, _) => variants
                .iter()
                .find(|variant| variant.id.unique_id == use_uid)
                .map(|variant| format!("enum variant {}", variant.id.name)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        colliding_defs == vec!["enum variant BitWidth::W8".to_string()]
            || colliding_defs == vec!["enum variant Global::BitWidth::W8".to_string()],
        "unexpected declarations sharing uid {use_uid}: {colliding_defs:?}"
    );
}

fn bitwidth_zero_arg_variant_typechecks_with_builtin_prelude() {
    let typed = typecheck_with_builtin_prelude("width = BitWidth::W8");
    assert!(matches!(
        typed.last().expect("user bind should be present").node,
        TypedInner::Bind(_, _)
    ));
}

fn ensure_special_form_typechecks_to_result_value() {
    let typed = typecheck_with_builtin_prelude(
        r#"def is_even(n: Int) -> Boolean { Int::is_even(n) }
guard = ensure(4, &is_even, NoneError())"#,
    );
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(rhs.node, TypedInner::Ensure(_, _, _)));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

fn and_special_form_typechecks_to_boolean_if() {
    let typed = typecheck_with_builtin_prelude("flag = and(True, False)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(rhs.node, TypedInner::If(_, _, Some(_))));
            assert!(matches!(rhs.ty, scar::types::Ty::Bool));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

fn eq_helper_typechecks_as_trait_call() {
    let typed = typecheck_with_builtin_prelude("flag = eq(1, 1)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(
                rhs.node,
                TypedInner::TraitCall { ref method_name, .. } if method_name == "eq"
            ));
            assert!(matches!(rhs.ty, scar::types::Ty::Bool));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

fn eq_helper_mismatch_uses_operator_helper_message() {
    let resolved = resolve_with_builtin_prelude("print(to_string(eq(1, True)))");
    let err = typecheck(resolved).expect_err("eq helper mismatch must fail");
    assert!(err
        .message
        .contains("Eq::eq helper cannot compare Int and Boolean"));
}

fn shadowed_eq_keeps_generic_call_mismatch_message() {
    let resolved = resolve_with_builtin_prelude(
        r#"def eq(left: String, right: String) -> Boolean {
  True
}

print(to_string(eq(1, True)))"#,
    );
    let err = typecheck(resolved).expect_err("shadowed eq should use generic call checking");
    assert!(err
        .message
        .contains("Argument type mismatch: expected String, got Int"));
    assert!(!err.message.contains("Eq::eq helper"));
}

fn concat_helper_typechecks_as_trait_call() {
    let typed = typecheck_with_builtin_prelude(r#"value = concat("a", "b")"#);
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(
                rhs.node,
                TypedInner::TraitCall { ref method_name, .. } if method_name == "concat"
            ));
            assert!(matches!(rhs.ty, scar::types::Ty::Str));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

fn to_string_helper_typechecks_as_trait_call() {
    let typed = typecheck_with_builtin_prelude("text = to_string(42)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(
                rhs.node,
                TypedInner::TraitCall { ref method_name, .. } if method_name == "to_string"
            ));
            assert!(matches!(rhs.ty, scar::types::Ty::Str));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

fn ensure_rejects_call_expression_predicate() {
    let err = typecheck_with_rules(
        r#"def is_even() -> (Int -> Boolean) { {|n| Int::is_even(n) } }
guard = ensure(4, is_even(), NoneError)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("call expression predicate must fail");
    assert!(err.message.contains("ensure requires a closure or capture"));
}

fn assert_rejects_non_concrete_error_expression() {
    let err = typecheck_with_rules(
        r#"def bad_code() -> Int { 1 }
guard = assert(False, bad_code())"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("non-Error expression must fail");
    assert!(err
        .message
        .contains("assert error branch must evaluate to Error, got Int"));
}

fn kernel_and_contract_rejects_eager_signature() {
    let err = typecheck_std_modules_with_overrides(&[(
        "Kernel",
        r#"defmod Kernel {
  @builtin def and(left: Boolean, right: Boolean) -> Boolean
}"#,
    )])
    .expect_err("eager signature should violate canonical contract");
    assert!(err
        .message
        .contains("@builtin def and(left: Boolean, right: Lazy<Boolean>) -> Boolean"));
}

fn special_form_builtin_decl_must_live_under_kernel() {
    let err = typecheck_std_modules_with_overrides(&[(
        "Boolean",
        r#"@builtin type Boolean

impl Boolean {
  def not(value: Boolean) -> Boolean {
if(value, False, True)
  }

  @builtin def and(left: Boolean, right: Boolean) -> Boolean
}"#,
    )])
    .expect_err("special-form declaration outside Kernel must fail");
    assert!(err
        .message
        .contains("Special-form declaration `and` is only allowed in std module `Kernel`."));
}

fn kernel_does_not_allow_removed_concat_builtin() {
    let module_stages = std_module_stages_with_overrides(&[(
        "Kernel",
        r#"defmod Kernel {
  @builtin def concat(left: $A, right: $A) -> String
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&module_stages).expect("std modules should precollect");
    let err = sigil::resolve_staged_program(&module_stages, Vec::new(), &declaration_index, None)
        .expect_err("concat is no longer a declared runtime builtin");
    assert!(err.message.contains("Unknown builtin declaration: concat"));
}

fn if_auto_forces_zero_arg_closure_once_for_branch_type() {
    let typed = typecheck_with_builtin_prelude("value = if(True, {|| 1}, 2)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.ty, scar::types::Ty::Int)),
        other => panic!("expected bind, got {:?}", other),
    }
}

fn if_nested_closure_is_not_deep_forced() {
    let err = typecheck_with_rules(
        "value = if(True, {|| {|| 1}}, 2)",
        RuntimeSourcePolicy::script(),
    )
    .expect_err("nested lazy branch should not be deep forced");
    assert!(err
        .message
        .contains("if branches have different types: (-> Int) and Int"));
}

fn user_lazy_annotation_is_rejected() {
    let err = typecheck_with_rules("x: Lazy<Int> = 1", RuntimeSourcePolicy::script())
        .expect_err("user lazy annotations must fail");
    assert!(err
        .message
        .contains("Lazy<T> is reserved for std-module special-form declarations"));
}

fn assert_accepts_lazy_error_branch() {
    let typed = typecheck_with_rules(
        r#"deferror SomeError(detail: String) { detail }
guard = assert(False, {|| SomeError("boom") })"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("lazy error branch should typecheck");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.node, TypedInner::Assert(_, _))),
        other => panic!("expected bind, got {:?}", other),
    }
}

fn ensure_accepts_lazy_error_branch() {
    let typed = typecheck_with_rules(
        r#"deferror SomeError(detail: String) { detail }
def is_positive(value: Int) -> Boolean { value > 0 }
guard = ensure(-1, &is_positive, {|| SomeError("boom") })"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("lazy ensure error branch should typecheck");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.node, TypedInner::Ensure(_, _, _))),
        other => panic!("expected bind, got {:?}", other),
    }
}

fn assert_accepts_existing_error_value() {
    let typed = typecheck_with_rules(
        r#"guard = match Err(NoneError) {
  Ok(_) => assert(False, NoneError),
  Err(e) => assert(False, e),
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("existing Error value should typecheck");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.node, TypedInner::Match(_, _))),
        other => panic!("expected bind, got {:?}", other),
    }
}

fn ensure_accepts_existing_error_value() {
    let typed = typecheck_with_rules(
        r#"def is_positive(value: Int) -> Boolean { value > 0 }
guard = match Err(NoneError) {
  Ok(_) => ensure(-1, &is_positive, NoneError),
  Err(e) => ensure(-1, &is_positive, e),
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("existing Error value should typecheck");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.node, TypedInner::Match(_, _))),
        other => panic!("expected bind, got {:?}", other),
    }
}

fn generic_annotation_list_int_is_accepted() {
    let typed = typecheck_with_builtin_prelude("nums: List<Int> = [1, 2, 3]");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

fn generic_def_signature_instantiates_per_call_site() {
    let typed = typecheck_with_builtin_prelude(
        r#"def id(x: $A) -> $A { x }
left: Int = id(1)
right: String = id("ok")"#,
    );
    assert!(typed.len() >= 3);
    assert!(typed
        .iter()
        .rev()
        .take(3)
        .all(|node| matches!(node.node, TypedInner::Bind(_, _) | TypedInner::Def(..))));
}

fn generic_defenum_constructor_and_match_typecheck() {
    let typed = typecheck_with_builtin_prelude(
        r#"defenum StepSignal<$A> {
  Resume($A),
  Stop($A),
}

step: StepSignal<Int> = StepSignal::Resume(1)
value = match step {
  StepSignal::Resume(v) => v,
  StepSignal::Stop(v) => v,
}"#,
    );
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::EnumDef(_, _))));
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

fn closure_param_annotation_without_expected_type_constrains_calls() {
    let resolved = resolve_with_builtin_prelude(
        r#"id = {|value: Int| value}
answer = id("oops")"#,
    );
    let err = typecheck(resolved).expect_err("annotation should reject String call");
    assert!(err.message.contains("expected Int, got String"));
}

fn closure_application_mismatch_reports_callable_type_signature() {
    let resolved = resolve_with_builtin_prelude(
        r#"inc = {|n: Int| n + 1}
answer = inc("oops")"#,
    );
    let err = typecheck(resolved).expect_err("closure application should fail");
    assert!(err.message.contains("expected Int, got String"));
    let hint = err.hint.as_deref().expect("callable signature hint");
    assert!(hint.contains("Callable type signature: (Int -> Int)"));
}

fn builtin_function_arity_reports_call_target_signature() {
    let resolved = resolve_with_builtin_prelude("value = print()");
    let err = typecheck(resolved).expect_err("builtin arity mismatch should fail");
    let hint = err.hint.as_deref().expect("builtin signature hint");
    assert_eq!(
        hint,
        "Call target signature: Kernel::print(arg1: String) -> Unit"
    );
}

fn builtin_function_mismatch_reports_call_target_signature() {
    let resolved = resolve_with_builtin_prelude("value = print(1)");
    let err = typecheck(resolved).expect_err("builtin type mismatch should fail");
    let hint = err.hint.as_deref().expect("builtin signature hint");
    assert_eq!(
        hint,
        "Call target signature: Kernel::print(arg1: String) -> Unit"
    );
}

fn capture_application_mismatch_reports_callable_type_signature() {
    let resolved = resolve_with_builtin_prelude_in_script_module(
        r#"def add(x: Int, y: Int) -> Int {
  x + y
}
bad = &add(&1, "oops")"#,
    )
    .expect("source should resolve");
    let err = typecheck(resolved).expect_err("capture application should fail");
    assert!(err.message.contains("expected Int, got String"));
    let hint = err
        .hint
        .as_deref()
        .expect("callable definition signature hint");
    assert!(hint.contains("Callable definition signature: add(x: Int, y: Int) -> Int"));
}

fn script_callable_signature_omits_file_path_segments() {
    let resolved = resolve_with_builtin_prelude_in_module(
        r#"def add_one(x: Int) -> Int {
  x + 1
}
result = add_one("oops")"#,
        "__Script::Users::haruca::work::rust::surtr::surtr_compile_error_cases::type_call_arg_mismatch",
    )
    .expect("source should resolve");
    let err = typecheck(resolved).expect_err("function call should fail");
    let hint = err
        .hint
        .as_deref()
        .expect("callable definition signature hint");
    assert!(hint.contains("Callable definition signature: add_one(x: Int) -> Int"));
    assert!(!hint.contains("__Script::Users::haruca"));
    assert!(hint.contains("Callable definition span: 0.."));
}

fn compose_mismatch_reports_left_and_right_callable_types() {
    let resolved = resolve_with_builtin_prelude(
        r#"def text(x: Int) -> String {
  to_string(x)
}

def inc(x: Int) -> Int {
  x + 1
}

bad = &text >> &inc"#,
    );
    let err = typecheck(resolved).expect_err("compose mismatch should fail");
    assert!(err.message.contains("left output type"));
    let hint = err.hint.as_deref().expect("compose mismatch hint");
    assert!(hint.contains("Left output is String; right input is Int"));
    assert!(hint.contains("LHS: (Int -> String)"));
    assert!(hint.contains("RHS: (Int -> Int)"));
}

fn compose_accepts_calls_returning_function_values() {
    let resolved = resolve_with_builtin_prelude(
        r#"def make_inc() -> (Int -> Int) {
  {|x| x + 1}
}

def make_double() -> (Int -> Int) {
  {|x| x * 2}
}

plain = make_inc() >> make_double()"#,
    );
    typecheck(resolved).expect("compose should accept function-returning calls");
}

fn compose_rejects_non_function_call_results_after_typechecking_call() {
    let resolved = resolve_with_builtin_prelude(
        r#"def inc(x: Int) -> Int {
  x + 1
}

plain = inc(1) >> inc(1)"#,
    );
    let err = typecheck(resolved).expect_err("compose should reject Int call results");
    assert_eq!(err.message, "`>>` requires a function value");
    let hint = err.hint.as_deref().expect("compose function-value hint");
    assert!(hint.contains("Call target signature:"));
    assert!(hint.contains("result type Int is not a function value"));
}

fn closure_trait_helper_binding_requires_concrete_callable_boundary() {
    let resolved = resolve_with_builtin_prelude(r#"cmp = {|left, right| compare(left, right)}"#);
    let err = typecheck(resolved).expect_err("unresolved closure helper binding must fail");
    assert!(err
        .message
        .contains("Trait helper `compare` could not be concretized for this callable binding"));
}

fn closure_trait_helper_binding_accepts_binding_annotation() {
    let resolved = resolve_with_builtin_prelude(
        r#"cmp: (Int, Int -> Ordering) = {|left, right| compare(left, right)}"#,
    );
    typecheck(resolved).expect("binding annotation should concretize compare helper");
}

fn closure_trait_helper_binding_accepts_parameter_annotations() {
    let resolved =
        resolve_with_builtin_prelude(r#"cmp = {|left: Int, right: Int| compare(left, right)}"#);
    typecheck(resolved).expect("parameter annotations should concretize compare helper");
}

fn on_call_concretizes_closure_trait_helper_from_key_function() {
    let resolved = resolve_with_builtin_prelude(
        r#"sorted = List::sort_by(["a", "abcd", "xy"], {|left, right| compare(left, right)} `on` &String::len)"#,
    );
    typecheck(resolved).expect("on should concretize compare helper from the key callable");
}

fn on_call_concretizes_trait_helper_capture_from_key_function() {
    let resolved = resolve_with_builtin_prelude(
        r#"sorted = List::sort_by(["a", "abcd", "xy"], &compare `on` &String::len)"#,
    );
    typecheck(resolved)
        .expect("on should concretize captured compare helper from the key callable");
}

fn pipe_plain_apply_over_result_reports_whole_lhs_mismatch() {
    let resolved = resolve_with_builtin_prelude(
        r#"def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def inc(x: Int) -> Int {
  x + 1
}

bad = parse(1) |> &inc"#,
    );
    let err = typecheck(resolved).expect_err("plain pipe over Result should fail");
    assert!(err.message.contains("expected Int, got Result<Int>"));
    let hint = err.hint.as_deref().expect("operator rule hint");
    assert!(hint.contains("`|>` signature rule"));
    assert!(hint.contains("LHS: Result<Int>"));
    assert!(hint.contains("RHS: (Int -> Int)"));
    assert!(!hint.contains("`|*>`"));
}

fn context_bind_rejects_plain_rhs_return() {
    let resolved = resolve_with_builtin_prelude(
        r#"def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def inc(x: Int) -> Int {
  x + 1
}

bad = parse(1) |>= &inc"#,
    );
    let err = typecheck(resolved).expect_err("bind with plain RHS should fail");
    assert!(err
        .message
        .contains("requires the right-hand side to return Result, got Int"));
    let hint = err.hint.as_deref().expect("operator rule hint");
    assert!(hint.contains("`|>=` signature rule"));
    assert!(hint.contains("RHS: (Int -> Int)"));
    assert!(hint.contains("Use `|*>`"));
}

fn context_bind_rhs_closure_receives_result_return_expectation() {
    let resolved = resolve_with_builtin_prelude(
        r#"bound: Result<Int> = Ok(1) |>= {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    typecheck(resolved).expect("bind RHS closure should receive Result return expectation");
}

fn safebind_pipe_bind_closure_receives_expected_result_return() {
    let typed = typecheck_with_builtin_prelude(
        r#"result: Result<Int> = Ok(1) |>= {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    assert_eq!(typed.last().map(|node| &node.ty), Some(&Ty::Unit));
}

fn apply_and_map_rhs_closures_receive_whole_expression_return_expectation() {
    let resolved = resolve_with_builtin_prelude(
        r#"applied: Result<Int> = 1 |> {|x|
  value =? Ok(x + 1)
  Ok(value)
}

mapped: Result<Result<Int>> = Ok(1) |*> {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    typecheck(resolved)
        .expect("annotated apply/map operators should pass Result expectation to RHS closures");
}

fn safebind_pipe_apply_annotated_closure_receives_expected_result_return() {
    let typed = typecheck_with_builtin_prelude(
        r#"result: Result<Int> = 1 |> {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    assert_eq!(typed.last().map(|node| &node.ty), Some(&Ty::Unit));
}

fn safebind_pipe_map_annotated_closure_receives_expected_result_return() {
    let typed = typecheck_with_builtin_prelude(
        r#"result: Result<Result<Int>> = Ok(1) |*> {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    assert_eq!(typed.last().map(|node| &node.ty), Some(&Ty::Unit));
}

fn kleisli_compose_closures_receive_result_return_expectation() {
    let resolved = resolve_with_builtin_prelude(
        r#"pipeline: (Int -> Result<Int>) = {|x|
  value =? Ok(x + 1)
  Ok(value)
} >=> {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    typecheck(resolved).expect("Kleisli operands should receive Result return expectation");
}

fn safebind_kleisli_annotated_closure_receives_expected_result_return() {
    let typed = typecheck_with_builtin_prelude(
        r#"def parse(text: String) -> Result<Int> {
  Ok(String::len(text))
}

pipeline: (String -> Result<Int>) = &parse >=> {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    assert_eq!(typed.last().map(|node| &node.ty), Some(&Ty::Unit));
}

fn lifted_compose_rhs_closure_allows_explicit_nested_result_expectation() {
    let resolved = resolve_with_builtin_prelude(
        r#"deferror Oops {
  "oops"
}

def gen(x: Int) -> Result<Int, Oops> {
  if(x > 0, Ok(x), Err(Oops))
}

pipeline: (Int -> Result<Result<Int>>) = {|x|
  gen(x)
} >* {|x|
  value =? Ok(x + 1)
  Ok(value)
}"#,
    );
    typecheck(resolved)
        .expect("Lifted compose should allow explicit nested Result RHS expectation");
}

fn context_map_keeps_result_for_later_bind() {
    let typed = typecheck_with_builtin_prelude(
        r#"def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def inc(x: Int) -> Int {
  x + 1
}

def stringify(x: Int) -> Result<String> {
  Ok(to_string(x))
}

ok = parse(1) |*> &inc |>= &stringify"#,
    );
    assert_eq!(typed.last().map(|node| &node.ty), Some(&Ty::Unit));
}

fn context_map_and_bind_lower_to_operator_trait_calls() {
    let typed = typecheck_with_builtin_prelude(
        r#"def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def inc(x: Int) -> Int {
  x + 1
}

def stringify(x: Int) -> Result<String> {
  Ok(to_string(x))
}

mapped = parse(1) |*> &inc
bound = parse(1) |>= &stringify"#,
    );
    let trait_calls = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    dispatch,
                    origin,
                    args,
                    ..
                } => Some((trait_name, method_name, dispatch, origin, args, &rhs.ty)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(trait_calls.iter().any(
        |(trait_name, method_name, dispatch, origin, args, result_ty)| {
            trait_name.ends_with("Functor")
                && *method_name == "fmap"
                && matches!(
                    dispatch,
                    scar::typed::TraitDispatch::Static(
                        scar::typed::TraitDispatchTarget::UserFunction { name, .. }
                    ) if name.ends_with("::fmap") || name == "fmap"
                )
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::PipeMap,
                        lhs_ty: Ty::Result(_, _),
                        rhs_ty: Ty::Func(_, _) | Ty::UserFunc { .. } | Ty::BuiltinFunc { .. },
                    }
                )
                && args.len() == 2
                && matches!(result_ty, Ty::Result(ok, _) if matches!(ok.as_ref(), Ty::Int))
        }
    ));
    assert!(trait_calls.iter().any(
        |(_trait_name, _method_name, _dispatch, origin, args, result_ty)| {
            matches!(
                origin,
                TraitCallOrigin::Operator {
                    op: OperatorTraitOp::PipeBind,
                    lhs_ty: Ty::Result(_, _),
                    rhs_ty: Ty::Func(_, _) | Ty::UserFunc { .. } | Ty::BuiltinFunc { .. },
                }
            ) && args.len() == 2
                && matches!(result_ty, Ty::Result(ok, _) if matches!(ok.as_ref(), Ty::Str))
        }
    ));
}

fn explicit_functor_call_has_explicit_origin() {
    let typed = typecheck_with_builtin_prelude(
        r#"def inc(x: Int) -> Int {
  x + 1
}

mapped = Functor::fmap(Ok(1), &inc)"#,
    );
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            method_name,
            origin,
            ..
        } => {
            assert_eq!(method_name, "fmap");
            assert_eq!(origin, &TraitCallOrigin::Explicit);
            assert!(matches!(rhs.ty, Ty::Result(_, _)));
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

fn flow_apply_and_compose_operators_lower_to_trait_calls() {
    let typed = typecheck_with_builtin_prelude(
        r#"def inc(x: Int) -> Int {
  x + 1
}

def show_int(x: Int) -> String {
  to_string(x)
}

def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def parse_list(x: Int) -> List<Int> {
  [x]
}

def maybe_parse(x: Int) -> Option<Int> {
  Option::Some(x)
}

def maybe_show(x: Int) -> Option<String> {
  Option::Some(to_string(x))
}

applied = 1 |> &inc
plain = &inc >> &show_int
lifted = &parse >* &show_int
kleisli = &parse_list >=> {|x| [x, x + 1]}
lifted_option = &maybe_parse >* &show_int
kleisli_option = &maybe_parse >=> &maybe_show"#,
    );
    let calls = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    origin,
                    ..
                } => Some((trait_name.as_str(), method_name.as_str(), origin, &rhs.ty)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(calls
        .iter()
        .any(|(trait_name, method_name, origin, result_ty)| {
            trait_name.starts_with("PipeApply<")
                && *method_name == "pipe_apply"
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::PipeApply,
                        lhs_ty: Ty::Int,
                        rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    }
                )
                && matches!(result_ty, Ty::Int)
        }));
    assert!(calls
        .iter()
        .any(|(trait_name, method_name, origin, result_ty)| {
            trait_name.starts_with("Composable<")
                && *method_name == "compose"
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::Compose,
                        lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                        rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    }
                )
                && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Str))
        }));
    assert!(calls.iter().any(|(trait_name, method_name, origin, result_ty)| {
        trait_name.starts_with("LiftComposable<")
            && *method_name == "lift_compose"
            && matches!(
                origin,
                TraitCallOrigin::Operator {
                    op: OperatorTraitOp::LiftCompose,
                    lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                }
            )
            && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Result(ok, _) if matches!(ok.as_ref(), Ty::Str)))
    }));
    assert!(calls
        .iter()
        .any(|(trait_name, method_name, origin, result_ty)| {
            trait_name.starts_with("KleisliComposable<")
                && *method_name == "kleisli_compose"
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::KleisliCompose,
                        lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                        rhs_ty: Ty::Func(_, _),
                    }
                )
                && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::List(_)))
        }));
    assert!(calls.iter().any(|(trait_name, method_name, origin, result_ty)| {
        trait_name.starts_with("LiftComposable<")
            && *method_name == "lift_compose"
            && matches!(
                origin,
                TraitCallOrigin::Operator {
                    op: OperatorTraitOp::LiftCompose,
                    lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                }
            )
            && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Enum(name, args) if (name == "Option" || name == "Global::Option") && matches!(args.as_slice(), [Ty::Str])))
    }));
    assert!(calls.iter().any(|(trait_name, method_name, origin, result_ty)| {
        trait_name.starts_with("KleisliComposable<")
            && *method_name == "kleisli_compose"
            && matches!(
                origin,
                TraitCallOrigin::Operator {
                    op: OperatorTraitOp::KleisliCompose,
                    lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                }
            )
            && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Enum(name, args) if (name == "Option" || name == "Global::Option") && matches!(args.as_slice(), [Ty::Str])))
    }));
}

fn user_defined_container_can_use_context_operators_via_traits() {
    let typed = typecheck_with_builtin_prelude(
        r#"defenum Boxed<$T> {
  Box($T),
}

impl Functor for Boxed<$T> {
  def fmap(self: Boxed<$A>, mapper: ($A -> $B)) -> Boxed<$B> {
    match self {
      Boxed::Box(value) => Boxed::Box(mapper(value)),
    }
  }
}

impl Applicative for Boxed<$T> {
  def pure(value: $A) -> Boxed<$A> { Boxed::Box(value) }

  def apply(mapper: Boxed<($A -> $B)>, value: Boxed<$A>) -> Boxed<$B> {
    match mapper {
      Boxed::Box(f) => match value {
        Boxed::Box(inner) => Boxed::Box(f(inner)),
      },
    }
  }
}

impl Monad for Boxed<$T> {
  def bind(self: Boxed<$A>, mapper: ($A -> Boxed<$B>)) -> Boxed<$B> {
    match self {
      Boxed::Box(value) => mapper(value),
    }
  }
}

impl LiftComposable<$A, $B, $C, Boxed<$C>> for ($A -> Boxed<$B>) {
  def lift_compose(self: Self, rhs: ($B -> $C)) -> ($A -> Boxed<$C>) {
    {|value| Functor::fmap(self(value), rhs)}
  }
}

impl KleisliComposable<$A, $B, Boxed<$C>> for ($A -> Boxed<$B>) {
  def kleisli_compose(self: Self, rhs: ($B -> Boxed<$C>)) -> ($A -> Boxed<$C>) {
    {|value| Monad::bind(self(value), rhs)}
  }
}

def inc(x: Int) -> Int {
  x + 1
}

def box_inc(x: Int) -> Boxed<Int> {
  Boxed::Box(x + 1)
}

def render(x: Int) -> String {
  to_string(x)
}

def stringify(x: Int) -> Boxed<String> {
  Boxed::Box(to_string(x))
}

mapped = Boxed::Box(1) |*> &inc
bound = Boxed::Box(1) |>= &stringify
lifted = &box_inc >* &render
kleisli = &box_inc >=> &stringify"#,
    );

    let boxed_results = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(&rhs.ty),
            _ => None,
        })
        .filter(|ty| matches!(ty, Ty::Enum(name, _) if name == "Boxed" || name == "Global::Boxed"))
        .count();
    assert_eq!(boxed_results, 2);
    let boxed_function_results = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(&rhs.ty),
            _ => None,
        })
        .filter(|ty| {
            matches!(ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Enum(name, _) if name == "Boxed" || name == "Global::Boxed"))
        })
        .count();
    assert_eq!(boxed_function_results, 2);
}

fn result_match_wildcard_self_after_ok_can_change_ok_payload_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"def remap(value: Result<Int>) -> Result<String> {
  match value {
    Ok(inner) => Ok(to_string(inner)),
    _ => value,
  }
}"#,
    );

    let typed = typecheck(resolved).expect("Err-proven wildcard arm should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Def(..))));
}

fn result_match_wildcard_self_after_ok_can_keep_err_for_bind_shape() {
    let resolved = resolve_with_builtin_prelude(
        r#"def bind_like(value: Result<Int>) -> Result<String> {
  match value {
    Ok(inner) => Ok(to_string(inner)),
    _ => value,
  }
}"#,
    );

    let typed = typecheck(resolved).expect("Err-proven bind-style wildcard arm should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Def(..))));
}

fn result_match_wildcard_self_requires_err_proven_branch() {
    let resolved = resolve_with_builtin_prelude(
        r#"def bad(value: Result<Int>) -> Result<String> {
  match value {
    _ => value,
    Ok(inner) => Ok(to_string(inner)),
  }
}"#,
    );

    let err = typecheck(resolved).expect_err("wildcard arm without prior Ok coverage must fail");
    assert!(
        err.message.contains("Match arm type mismatch")
            || err
                .message
                .contains("expected Result<String>, got Result<Int>")
            || err
                .message
                .contains("expected Result<Int>, got Result<String>")
    );
}

fn closure_param_annotation_must_match_expected_signature() {
    let resolved = resolve_with_builtin_prelude(r#"id: (String -> String) = {|value: Int| value}"#);
    let err = typecheck(resolved).expect_err("mismatched expected signature must fail");
    assert!(err
        .message
        .contains("closure parameter `value` expected String, got Int"));
}

fn local_binding_annotation_can_reference_outer_generic_type_param() {
    let typed = typecheck_with_builtin_prelude(
        r#"def id(x: $A) -> $A {
  y: $A = x
  y
}"#,
    );
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Def(..))));
}

fn closure_param_annotation_can_reference_outer_generic_type_param() {
    let typed = typecheck_with_builtin_prelude(
        r#"def keep(x: $A) -> $A {
  same: ($A -> $A) = {|value: $A| value}
  same(x)
}"#,
    );
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Def(..))));
}

fn generic_first_can_inline_tuple_rebuild_with_closure_param_annotation() {
    let typed = typecheck_with_builtin_prelude(
        r#"def first(f: ($A -> $C)) -> (($A, $B) -> ($C, $B)) {
  {|pair: ($A, $B)|
    (left, right) = pair
    (f(left), right)
  }
}"#,
    );
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Def(..))));
}

fn sibling_closures_keep_substitution_state_local() {
    let typed = typecheck_with_builtin_prelude(
        r#"int_id: (Int -> Int) = {|value| value}
str_id: (String -> String) = {|value| value}
left: Int = int_id(1)
right: String = str_id("ok")"#,
    );
    assert!(typed.len() >= 4);
    assert!(typed
        .iter()
        .rev()
        .take(4)
        .all(|node| matches!(node.node, TypedInner::Bind(_, _))));
}

fn cyclic_type_definition_is_rejected() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Node {
  next: Node,
}"#,
    );
    let err = typecheck(resolved).expect_err("cyclic type must fail");
    assert!(err.message.contains("Cyclic type definition detected"));
}

fn enum_cycle_is_allowed_when_not_shared_by_all_variants() {
    let resolved = resolve_with_builtin_prelude(
        r#"defenum Loop {
  End,
  Next(Loop),
}
value: Loop = Loop::End"#,
    );
    let typed = typecheck(resolved).expect("enum should allow conditional recursion");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::EnumDef(_, _))));
}

fn enum_cycle_is_rejected_when_shared_by_all_variants() {
    let resolved = resolve_with_builtin_prelude(
        r#"defenum Loop {
  A(Loop),
  B(Loop),
}"#,
    );
    let err = typecheck(resolved).expect_err("enum cycle must fail");
    assert!(err.message.contains("Cyclic type definition detected"));
}

fn enum_field_access_is_rejected() {
    let resolved = resolve_with_builtin_prelude(
        r#"defenum Direction {
  Up,
  Down,
}
up: Direction = Direction::Up
x = up.idx"#,
    );
    let err = typecheck(resolved).expect_err("enum field access must fail");
    assert!(err
        .message
        .contains("No variant selector 'idx' on Direction"));
}

fn match_binding_pattern_is_treated_as_exhaustive() {
    let resolved = resolve_with_builtin_prelude(
        r#"flag = True
answer = match flag {
  value => value,
}"#,
    );
    let typed = typecheck(resolved).expect("binding arm should be exhaustive");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

fn match_tuple_binding_pattern_is_treated_as_exhaustive() {
    let resolved = resolve_with_builtin_prelude(
        r#"pair = (1, "two")
answer = match pair {
  (left, right) => right,
}"#,
    );
    let typed = typecheck(resolved).expect("tuple binding arm should be exhaustive");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

fn match_guard_must_be_boolean() {
    let resolved = resolve_with_builtin_prelude(
        r#"answer = match 1 {
  n when 1 => n,
  _ => 0,
}"#,
    );
    let err = typecheck(resolved).expect_err("non-boolean guard must fail");
    assert!(err.message.contains("match guard must be Boolean, got Int"));
}

fn guarded_match_arm_does_not_satisfy_exhaustiveness() {
    let resolved = resolve_with_builtin_prelude(
        r#"answer = match True {
  flag when flag => 1,
}"#,
    );
    let err = typecheck(resolved).expect_err("guarded-only arm must be non-exhaustive");
    assert!(err
        .message
        .contains("Non-exhaustive match. Missing: True, False"));
}

fn struct_literal_rejects_extra_fields() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age, extra: 1 }
  }
}
user = User("alice", 20)"#,
    );
    let err = typecheck(resolved).expect_err("extra fields must fail");
    assert!(err.message.contains("Unknown field 'extra' in User"));
}

fn constructor_named_args_reject_duplicate_fields() {
    let resolved = resolve_with_builtin_prelude(
        r#"defrecord Pair(first: Int, second: String)
pair = Pair(first: 1, first: 2)"#,
    );
    let err = typecheck(resolved).expect_err("duplicate named args must fail");
    assert!(err.message.contains("Duplicate field 'first' in Pair"));
}

fn struct_literal_field_shorthand_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
User { name, age }
  }
}
user = User("alice", 20)"#,
    );
    typecheck(resolved).expect("struct shorthand should typecheck");
}

fn struct_literal_field_shorthand_mixed_with_explicit_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def rename(self, name: String, next_age: Int) -> Self {
User { name, age: next_age }
  }

  def new(name: String, age: Int) -> Self {
    User::rename(User { name, age }, name, age)
  }
}"#,
    );
    typecheck(resolved).expect("mixed struct shorthand should typecheck");
}

fn struct_literal_field_shorthand_rejects_duplicate_fields() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
User { name, name: name }
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("duplicate shorthand field must fail");
    assert!(err.message.contains("Duplicate field 'name' in User"));
}

fn struct_requires_impl_new() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
user = User("alice")"#,
    );
    let err = typecheck(resolved).expect_err("struct without new should fail");
    assert!(err.message.contains("must define `new` in its impl block"));
}

fn generic_struct_bare_annotation_requires_type_args() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Box<$A> {
  value: $A,
}
boxed: Box = Box(1)
impl Box {
  def new<$A>(value: $A) -> Box<$A> {
    Box { value: value }
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("bare generic struct annotation should fail");
    assert!(err.message.contains("Type Box requires 1 type argument(s)"));
}

fn generic_struct_arity_mismatch_is_rejected() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Pair<$A, $B> {
  left: $A,
  right: $B,
}
pair: Pair<Int> = Pair(1, 2)
impl Pair {
  def new<$A, $B>(left: $A, right: $B) -> Pair<$A, $B> {
    Pair { left: left, right: right }
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("generic struct arity mismatch should fail");
    assert!(err
        .message
        .contains("Type Pair requires 2 type argument(s), got 1"));
}

fn struct_new_accepts_result_self_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Duration {
  private millis: Int,
}
impl Duration {
  def new(value: Int) -> Result<Self, Error> {
    Ok(Duration { millis: value })
  }
}
value: Result<Duration> = Duration(10)"#,
    );
    let typed = typecheck(resolved).expect("Result<Self, Error> constructor should pass");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");
    assert!(matches!(
        &rhs.ty,
        Ty::Result(ok, err)
            if matches!(ok.as_ref(), Ty::Struct(name, _) if name == "Global::Duration")
                && matches!(err.as_ref(), Ty::Error)
    ));
}

fn struct_new_rejects_non_self_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Int {
    1
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("non-Self constructor return must fail");
    assert!(err
        .message
        .contains("`new` must return Self or Result<Self, E>"));
}

fn struct_new_rejects_result_non_self_payload() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Result<List<Self>, Error> {
    Ok([User { name: name }])
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("Result payload must be Self");
    assert!(err
        .message
        .contains("`new` must return Self or Result<Self, E>"));
}

fn struct_constructor_call_accepts_result_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Duration {
  private millis: Int,
}
impl Duration {
  def new(value: Int) -> Result<Self, Error> {
    Ok(Duration { millis: value })
  }
}
dur = Duration(10)"#,
    );
    let typed = typecheck(resolved).expect("constructor call should accept Result<Self>");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");
    assert!(matches!(
        &rhs.ty,
        Ty::Result(ok, err)
            if matches!(ok.as_ref(), Ty::Struct(name, _) if name == "Global::Duration")
                && matches!(err.as_ref(), Ty::Error)
    ));
}

fn struct_literal_is_rejected_outside_impl_body() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
User { name: name }
  }
}
user = User { name: "alice" }"#,
    );
    let err = typecheck(resolved).expect_err("struct literal outside impl should fail");
    assert!(err
        .message
        .contains("Struct literal `User` is only allowed inside"));
}

fn user_function_call_rejects_mixed_named_and_positional_args() {
    let resolved = resolve_with_builtin_prelude(
        r#"def add3(x: Int, y: Int, z: Int) -> Int { x + y + z }
value = add3(1, y: 2, z: 3)"#,
    );
    let err = typecheck(resolved).expect_err("mixed args should fail");
    assert!(err
        .message
        .contains("Cannot mix positional and named arguments"));
}

fn user_function_call_rejects_duplicate_named_arg() {
    let resolved = resolve_with_builtin_prelude(
        r#"def add2(x: Int, y: Int) -> Int { x + y }
value = add2(x: 1, x: 2)"#,
    );
    let err = typecheck(resolved).expect_err("duplicate named arg should fail");
    assert!(err.message.contains("Duplicate argument 'x'"));
}

fn impl_self_rebinding_allows_self_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}

impl User {
  def new(name: String) -> Self {
User { name: name }
  }

  def keep(self) -> Self {
self = self
self
  }
}

user = User("alice")
print(to_string(User::keep(user).name))"#,
    );
    let _typed = typecheck(resolved).expect("self rebinding with Self should pass");
}

fn impl_self_rebinding_rejects_non_self_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}

impl User {
  def new(name: String) -> Self {
User { name: name }
  }

  def bad(self) -> Self {
self = 1
self
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("self rebinding with non-Self must fail");
    assert!(err.message.contains("`self` rebinding requires Self type"));
}

fn deferror_show_type_mismatch_points_to_show_expression_span() {
    let source = r#"deferror NotFound(code: String) {
  123
}"#;
    let resolved = resolve_with_builtin_prelude(source);
    let err = typecheck(resolved).expect_err("show block must return String");
    let literal_start = source.find("123").expect("literal should exist in source");
    assert!(err
        .message
        .contains("deferror show block must return String"));
    assert_eq!(err.span.start, literal_start);
}

fn operator_traits_and_concrete_numeric_helpers_typecheck() {
    let typed = typecheck_with_builtin_prelude(
        r#"sum = 1 + 2
quot = Float::safe_div(8.0, 2.0)
largest = Float::max(1.5, 2.5)"#,
    );

    let trait_calls = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    dispatch,
                    ..
                } => Some((trait_name.as_str(), method_name.as_str(), dispatch)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(trait_calls.len(), 1);
    assert!(trait_calls
        .iter()
        .any(|(trait_name, method_name, dispatch)| {
            *trait_name == "Add"
                && *method_name == "add"
                && matches!(
                    dispatch,
                    scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::BinOp(
                        spire::ast::BinOp::Add
                    ))
                )
        }));
}

fn duration_operator_traits_dispatch_to_surtr_impls() {
    let typed = typecheck_with_builtin_prelude(
        r#"sum = 10ms + 20ms
same = 10ms == 10ms
less = 10ms < 20ms"#,
    );

    let trait_calls = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    dispatch,
                    origin,
                    ..
                } => Some((trait_name.as_str(), method_name.as_str(), dispatch, origin)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    for (trait_name, method, expected_name) in [
        ("Add", "add", "Duration::add"),
        ("Eq", "eq", "Duration::eq"),
        ("Compare", "lt", "Compare::lt"),
    ] {
        assert!(
            trait_calls
                .iter()
                .any(|(actual_trait_name, method_name, dispatch, _)| {
                    *actual_trait_name == trait_name
                        && *method_name == method
                        && matches!(
                            dispatch,
                            scar::typed::TraitDispatch::Static(
                                scar::typed::TraitDispatchTarget::UserFunction { name, .. }
                            ) if name == expected_name
                        )
                }),
            "{trait_name}::{method} should dispatch to {expected_name}"
        );
    }

    assert!(
        trait_calls
            .iter()
            .any(|(trait_name, method_name, _, origin)| {
                *trait_name == "Compare"
                    && *method_name == "lt"
                    && matches!(
                        origin,
                        scar::typed::TraitCallOrigin::Comparison {
                            op: scar::typed::ComparisonOperator::Lt,
                            ..
                        }
                    )
            }),
        "< should lower through Compare::lt with a comparison origin"
    );
}

#[test]
fn compare_default_methods_dispatch_to_trait_source_when_impl_omits_override() {
    let typed = typecheck_with_builtin_prelude(
        r#"
defstruct BoxedInt { value: Int }

impl BoxedInt {
  def new(value: Int) -> Self {
    BoxedInt { value }
  }
}

impl Compare for BoxedInt {
  def compare(self: Self, rhs: Self) -> Ordering {
    Compare::compare(self.value, rhs.value)
  }
}

less = lt(BoxedInt(1), BoxedInt(2))
"#,
    );

    let trait_call = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    dispatch,
                    ..
                } => Some((trait_name.as_str(), method_name.as_str(), dispatch)),
                _ => None,
            },
            _ => None,
        })
        .expect("lt call should typecheck");

    assert_eq!(trait_call.0, "Compare");
    assert_eq!(trait_call.1, "lt");
    assert!(matches!(
        trait_call.2,
        scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
            name,
            ..
        }) if name == "Compare::lt"
    ));
}

#[test]
fn compare_trait_impl_still_requires_compare_method() {
    let resolved = resolve_with_builtin_prelude(
        r#"
defstruct BoxedInt { value: Int }

impl BoxedInt {
  def new(value: Int) -> Self {
    BoxedInt { value }
  }
}

impl Compare for BoxedInt {}
"#,
    );
    let err = typecheck(resolved)
        .expect_err("Compare::compare should remain required")
        .message;

    assert!(err.contains("missing method `compare`"), "{err}");
}

fn bounded_add_generics_specialize_without_pending_trait_calls() {
    fn has_pending_trait_call(node: &TypedNode) -> bool {
        match &node.node {
            TypedInner::TraitCall { dispatch, args, .. } => {
                matches!(dispatch, scar::typed::TraitDispatch::Pending)
                    || args.iter().any(has_pending_trait_call)
            }
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                has_pending_trait_call(func) || args.iter().any(has_pending_trait_call)
            }
            TypedInner::Block(stmts) => stmts.iter().any(has_pending_trait_call),
            TypedInner::Bind(_, rhs)
            | TypedInner::SafeBind(_, rhs)
            | TypedInner::Semi(rhs)
            | TypedInner::FieldAccess(rhs, _) => has_pending_trait_call(rhs),
            TypedInner::EagerBoundary(inner) => has_pending_trait_call(inner),
            TypedInner::ProcessContextHandler { .. } => false,
            TypedInner::SupervisorSpawn { init, .. } => has_pending_trait_call(init),
            TypedInner::SupervisorAdopt { pid, .. } => has_pending_trait_call(pid),
            TypedInner::SupervisorStatus { .. } => false,
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                has_pending_trait_call(init) || has_pending_trait_call(strategy)
            }
            TypedInner::FacetPath(_) | TypedInner::PendingFacetPath(_) => false,
            TypedInner::FacetView { source, .. } => has_pending_trait_call(source),
            TypedInner::FacetSet { source, value, .. } => {
                has_pending_trait_call(source) || has_pending_trait_call(value)
            }
            TypedInner::FacetOver {
                source, update_fun, ..
            } => has_pending_trait_call(source) || has_pending_trait_call(update_fun),
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => {
                has_pending_trait_call(left) || has_pending_trait_call(right)
            }
            TypedInner::TupleLiteral(items)
            | TypedInner::ListLiteral(items)
            | TypedInner::ConstructorCall(_, items)
            | TypedInner::StructLit(_, items) => items.iter().any(has_pending_trait_call),
            TypedInner::HashMapLiteral(entries) => entries
                .iter()
                .any(|(key, value)| has_pending_trait_call(key) || has_pending_trait_call(value)),
            TypedInner::If(cond, then_branch, else_branch) => {
                has_pending_trait_call(cond)
                    || has_pending_trait_call(then_branch)
                    || else_branch.as_deref().is_some_and(has_pending_trait_call)
            }
            TypedInner::Assert(cond, err) => {
                has_pending_trait_call(cond) || has_pending_trait_call(err)
            }
            TypedInner::Ensure(value, pred, err) => {
                has_pending_trait_call(value)
                    || has_pending_trait_call(pred)
                    || has_pending_trait_call(err)
            }
            TypedInner::MapErr(value, err) | TypedInner::Cause(value, err) => {
                has_pending_trait_call(value) || has_pending_trait_call(err)
            }
            TypedInner::RecoverKind(value, marker, handler) => {
                has_pending_trait_call(value)
                    || has_pending_trait_call(marker)
                    || has_pending_trait_call(handler)
            }
            TypedInner::Match(scrutinee, arms) => {
                has_pending_trait_call(scrutinee)
                    || arms.iter().any(|arm| {
                        arm.guard.as_ref().is_some_and(has_pending_trait_call)
                            || has_pending_trait_call(&arm.body)
                    })
            }
            TypedInner::InterpolatedStr(parts) => parts.iter().any(|part| match part {
                scar::typed::TypedInterpolatedPart::Text(_) => false,
                scar::typed::TypedInterpolatedPart::Expr(expr) => has_pending_trait_call(expr),
            }),
            TypedInner::Dbg(args) => args.iter().any(|arg| has_pending_trait_call(&arg.expr)),
            TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body) => has_pending_trait_call(body),
            TypedInner::Lit(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::DeferrorDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..) => false,
        }
    }

    let typed = typecheck_with_builtin_prelude(
        r#"def double<$N: Add>(x: $N) -> $N { x + x }
a = double(21)
b = double(1.5)"#,
    );

    let double_defs = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Def(fun_idx, id, ..) if id.name == "double" => Some(*fun_idx),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(double_defs.len(), 2);
    assert_ne!(double_defs[0], double_defs[1]);
    assert!(!typed.iter().any(has_pending_trait_call));
}

fn range_duration_comparisons_specialize_without_pending_trait_calls() {
    fn has_pending_trait_call(node: &TypedNode) -> bool {
        match &node.node {
            TypedInner::TraitCall { dispatch, args, .. } => {
                matches!(dispatch, scar::typed::TraitDispatch::Pending)
                    || args.iter().any(has_pending_trait_call)
            }
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                has_pending_trait_call(func) || args.iter().any(has_pending_trait_call)
            }
            TypedInner::Block(stmts) => stmts.iter().any(has_pending_trait_call),
            TypedInner::Bind(_, rhs)
            | TypedInner::SafeBind(_, rhs)
            | TypedInner::Semi(rhs)
            | TypedInner::FieldAccess(rhs, _) => has_pending_trait_call(rhs),
            TypedInner::EagerBoundary(inner) => has_pending_trait_call(inner),
            TypedInner::ProcessContextHandler { .. } => false,
            TypedInner::SupervisorSpawn { init, .. } => has_pending_trait_call(init),
            TypedInner::SupervisorAdopt { pid, .. } => has_pending_trait_call(pid),
            TypedInner::SupervisorStatus { .. } => false,
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                has_pending_trait_call(init) || has_pending_trait_call(strategy)
            }
            TypedInner::FacetPath(_) | TypedInner::PendingFacetPath(_) => false,
            TypedInner::FacetView { source, .. } => has_pending_trait_call(source),
            TypedInner::FacetSet { source, value, .. } => {
                has_pending_trait_call(source) || has_pending_trait_call(value)
            }
            TypedInner::FacetOver {
                source, update_fun, ..
            } => has_pending_trait_call(source) || has_pending_trait_call(update_fun),
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => {
                has_pending_trait_call(left) || has_pending_trait_call(right)
            }
            TypedInner::TupleLiteral(items)
            | TypedInner::ListLiteral(items)
            | TypedInner::ConstructorCall(_, items)
            | TypedInner::StructLit(_, items) => items.iter().any(has_pending_trait_call),
            TypedInner::HashMapLiteral(entries) => entries
                .iter()
                .any(|(key, value)| has_pending_trait_call(key) || has_pending_trait_call(value)),
            TypedInner::If(cond, then_branch, else_branch) => {
                has_pending_trait_call(cond)
                    || has_pending_trait_call(then_branch)
                    || else_branch.as_deref().is_some_and(has_pending_trait_call)
            }
            TypedInner::Assert(cond, err) => {
                has_pending_trait_call(cond) || has_pending_trait_call(err)
            }
            TypedInner::Ensure(value, pred, err) => {
                has_pending_trait_call(value)
                    || has_pending_trait_call(pred)
                    || has_pending_trait_call(err)
            }
            TypedInner::MapErr(value, err) | TypedInner::Cause(value, err) => {
                has_pending_trait_call(value) || has_pending_trait_call(err)
            }
            TypedInner::RecoverKind(value, marker, handler) => {
                has_pending_trait_call(value)
                    || has_pending_trait_call(marker)
                    || has_pending_trait_call(handler)
            }
            TypedInner::Match(scrutinee, arms) => {
                has_pending_trait_call(scrutinee)
                    || arms.iter().any(|arm| {
                        arm.guard.as_ref().is_some_and(has_pending_trait_call)
                            || has_pending_trait_call(&arm.body)
                    })
            }
            TypedInner::InterpolatedStr(parts) => parts.iter().any(|part| match part {
                scar::typed::TypedInterpolatedPart::Text(_) => false,
                scar::typed::TypedInterpolatedPart::Expr(expr) => has_pending_trait_call(expr),
            }),
            TypedInner::Dbg(args) => args.iter().any(|arg| has_pending_trait_call(&arg.expr)),
            TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body) => has_pending_trait_call(body),
            TypedInner::Lit(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::DeferrorDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..) => false,
        }
    }

    let typed = typecheck_with_builtin_prelude(
        r#"left = Range(10ms, 20ms)
right = Range(10ms, 30ms)
same = Range(10ms, 20ms)
ordering = compare(left, right)
eq = left == same
neq = left != right"#,
    );

    assert!(!typed.iter().any(has_pending_trait_call));
}

fn generic_struct_constructor_calls_remain_polymorphic_within_one_source() {
    let typed = typecheck_with_builtin_prelude(
        r#"defstruct Box<$A> {
  value: $A,
}
impl Box {
  def new<$A>(value: $A) -> Box<$A> {
    Box { value: value }
  }
}
a = Box(1)
b = Box(10ms)"#,
    );

    let mut bindings = typed.iter().filter_map(|node| match &node.node {
        TypedInner::Bind(TypedPattern::Var(ty, id), rhs) if id.name == "a" || id.name == "b" => {
            Some((id.name.as_str(), ty, rhs.as_ref()))
        }
        _ => None,
    });

    let a = bindings.next().expect("expected a binding");
    let b = bindings.next().expect("expected b binding");

    assert_eq!(a.0, "a");
    assert!(matches!(
        a.1,
        Ty::Struct(name, fields)
            if name == "Global::Box"
                && matches!(fields.as_slice(), [(field, Ty::Int)] if field == "value")
    ));
    assert!(matches!(
        a.2.ty,
        Ty::Struct(ref name, ref fields)
            if name == "Global::Box"
                && matches!(fields.as_slice(), [(field, Ty::Int)] if field == "value")
    ));

    assert_eq!(b.0, "b");
    assert!(matches!(
        b.1,
        Ty::Struct(name, fields)
            if name == "Global::Box"
                && matches!(fields.as_slice(), [(field, Ty::Struct(inner, _inner_fields))]
                    if field == "value"
                        && inner == "Duration")
    ));
    assert!(matches!(
        b.2.ty,
        Ty::Struct(ref name, ref fields)
            if name == "Global::Box"
                && matches!(fields.as_slice(), [(field, Ty::Struct(inner, _inner_fields))]
                    if field == "value"
                        && inner == "Duration")
    ));
}

fn generic_struct_constructor_calls_remain_polymorphic_within_closure_body() {
    let typed = typecheck_with_builtin_prelude(
        r#"factory = {||
  raw = Range(3, 1)
  dur = Range(10ms, 20ms)
  (raw, dur)
}"#,
    );

    let factory = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(TypedPattern::Var(_, id), rhs) if id.name == "factory" => Some(rhs),
            _ => None,
        })
        .expect("expected factory binding");

    let TypedInner::Closure(_, _, body) = &factory.node else {
        panic!("expected closure");
    };
    let TypedInner::Block(stmts) = &body.node else {
        panic!("expected closure body block");
    };

    let mut range_bindings = stmts.iter().filter_map(|node| match &node.node {
        TypedInner::Bind(TypedPattern::Var(ty, id), rhs)
            if id.name == "raw" || id.name == "dur" =>
        {
            Some((id.name.as_str(), ty, rhs.as_ref()))
        }
        _ => None,
    });

    let raw = range_bindings.next().expect("expected raw binding");
    let dur = range_bindings.next().expect("expected dur binding");

    assert_eq!(raw.0, "raw");
    assert!(matches!(
        raw.1,
        Ty::Struct(name, fields)
            if (name == "Range" || name == "Global::Range")
                && matches!(fields.as_slice(), [(min, Ty::Int), (max, Ty::Int)]
                    if min == "min" && max == "max")
    ));
    assert!(matches!(
        raw.2.ty,
        Ty::Struct(ref name, ref fields)
            if (name == "Range" || name == "Global::Range")
                && matches!(fields.as_slice(), [(min, Ty::Int), (max, Ty::Int)]
                    if min == "min" && max == "max")
    ));

    assert_eq!(dur.0, "dur");
    assert!(matches!(
        dur.1,
        Ty::Struct(name, fields)
            if (name == "Range" || name == "Global::Range")
                && matches!(
                    fields.as_slice(),
                    [(min, Ty::Struct(inner_min, _)), (max, Ty::Struct(inner_max, _))]
                        if min == "min"
                            && max == "max"
                            && inner_min == "Duration"
                            && inner_max == "Duration"
                )
    ));
    assert!(matches!(
        dur.2.ty,
        Ty::Struct(ref name, ref fields)
            if (name == "Range" || name == "Global::Range")
                && matches!(
                    fields.as_slice(),
                    [(min, Ty::Struct(inner_min, _)), (max, Ty::Struct(inner_max, _))]
                        if min == "min"
                            && max == "max"
                            && inner_min == "Duration"
                            && inner_max == "Duration"
                )
    ));
}

fn scar_session_preserves_trait_registry_across_chunks() {
    let mut session = session_from_cached_std_prelude();
    let user_resolved = resolve_with_builtin_prelude("value = 1 + 2");
    let typed = session
        .typecheck(user_resolved)
        .expect("trait registry should survive across chunks");

    assert!(typed.iter().any(|node| {
        matches!(
            &node.node,
            TypedInner::Bind(_, rhs)
                if matches!(
                    &rhs.node,
                    TypedInner::TraitCall {
                        method_name,
                        dispatch: scar::typed::TraitDispatch::Static(
                            scar::typed::TraitDispatchTarget::BinOp(spire::ast::BinOp::Add)
                        ),
                        ..
                    } if method_name == "add"
                )
        )
    }));
}

fn add_trait_mismatch_lists_available_implementations() {
    let resolved = resolve_with_builtin_prelude("value = Add::add(1, False)");
    let err = typecheck(resolved).expect_err("mismatched add trait call must fail");
    assert!(err.message.contains("Add::add expects argument 2"));
    assert!(err.message.contains("receiver type Int"));
    assert!(err.message.contains("got Boolean"));
    let hint = err.hint.as_deref().expect("trait summary hint");
    assert!(hint.contains("Call target signature: Add::add"));
    assert!(hint.contains("Add is implemented for: Duration, Float, Int"));
}

fn trait_method_call_rejects_named_arguments_without_panic() {
    let resolved = resolve_with_builtin_prelude("value = Add::add(self: 1, rhs: 2)");
    let err = typecheck(resolved).expect_err("named trait method args should fail");
    assert!(err
        .message
        .contains("Add::add does not accept named arguments"));
}

fn add_trait_missing_receiver_lists_available_implementations() {
    let resolved = resolve_with_builtin_prelude("value = Add::add(False, True)");
    let err = typecheck(resolved).expect_err("invalid add receiver must fail");
    assert!(err
        .message
        .contains("Add::add requires a receiver type implementing Add, got Boolean"));
    let hint = err.hint.as_deref().expect("trait summary hint");
    assert!(hint.contains("Call target signature: Add::add"));
    assert!(hint.contains("Add is implemented for: Duration, Float, Int"));
}

fn add_operator_missing_impl_lists_available_implementations_in_hint() {
    let resolved = resolve_with_builtin_prelude("value = False + True");
    let err = typecheck(resolved).expect_err("invalid add operator must fail");
    assert!(err.message.contains("`+` is not defined for Boolean"));
    let hint = err.hint.as_deref().expect("operator hint");
    assert!(hint.contains("Add is implemented for: Duration, Float, Int"));
}

fn bind_operator_missing_impl_lists_available_implementations_in_hint() {
    let resolved = resolve_with_builtin_prelude("value = 1 |>= {|x| Ok(x)}");
    let err = typecheck(resolved).expect_err("plain lhs bind must fail");
    assert!(err
        .message
        .contains("`|>=` requires Monad implementation on the left, got Int"));
    let hint = err.hint.as_deref().expect("bind hint");
    assert!(hint.contains("Monad is implemented for:"));
    assert!(hint.contains("List<$T>"));
    assert!(hint.contains("Option<$T>"));
    assert!(hint.contains("Result<$T>"));
}

fn from_helper_typechecks_as_generic_trait_call() {
    let typed = typecheck_with_builtin_prelude(r#"value = from::<String>(42)"#);
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            trait_name,
            method_name,
            receiver_ty,
            dispatch:
                scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
                    name,
                    ..
                }),
            args,
            ..
        } => {
            assert_eq!(trait_name, "From<String>");
            assert_eq!(method_name, "from");
            assert_eq!(name, "From<String>::Int::from");
            assert_eq!(receiver_ty, &scar::types::Ty::Int);
            assert_eq!(args.len(), 1);
            assert_eq!(rhs.ty, scar::types::Ty::Str);
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

fn try_from_helper_typechecks_as_generic_trait_call() {
    let typed = typecheck_with_builtin_prelude(r#"value = try_from::<Int>("42")"#);
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            trait_name,
            method_name,
            receiver_ty,
            dispatch:
                scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
                    name,
                    ..
                }),
            args,
            ..
        } => {
            assert_eq!(trait_name, "TryFrom<Int>");
            assert_eq!(method_name, "try_from");
            assert_eq!(name, "TryFrom<Int>::String::try_from");
            assert_eq!(receiver_ty, &scar::types::Ty::Str);
            assert_eq!(args.len(), 1);
            assert!(matches!(rhs.ty, scar::types::Ty::Result(_, _)));
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

fn encode_helper_typechecks_as_generic_trait_call() {
    let typed = typecheck_with_builtin_prelude(r#"value = Encode::encode::<JsonValue>("hello")"#);
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            trait_name,
            method_name,
            receiver_ty,
            dispatch:
                scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
                    name,
                    ..
                }),
            args,
            ..
        } => {
            assert_eq!(trait_name, "Encode<JsonValue>");
            assert_eq!(method_name, "encode");
            assert_eq!(name, "Encode<JsonValue>::String::encode");
            assert_eq!(receiver_ty, &scar::types::Ty::Str);
            assert_eq!(args.len(), 1);
            assert!(matches!(rhs.ty, scar::types::Ty::Result(_, _)));
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

fn json_value_encode_source_alias_typechecks() {
    let typed = typecheck_with_builtin_prelude(r#"value = JsonValue::encode("hello")"#);
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    assert!(matches!(rhs.ty, scar::types::Ty::Result(_, _)));
}

fn decode_helper_typechecks_explicit_target() {
    let typed = typecheck_with_builtin_prelude(
        r#"value = Decode::decode::<String>(JsonValue::String("ok"))"#,
    );
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            trait_name,
            method_name,
            receiver_ty,
            dispatch:
                scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
                    name,
                    ..
                }),
            args,
            ..
        } => {
            assert_eq!(trait_name, "Decode<String>");
            assert_eq!(method_name, "decode");
            assert_eq!(name, "Decode<String>::JsonValue::decode");
            assert!(
                matches!(receiver_ty, scar::types::Ty::Enum(name, _) if name.ends_with("JsonValue"))
            );
            assert_eq!(args.len(), 1);
            assert!(matches!(rhs.ty, scar::types::Ty::Result(_, _)));
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

fn decode_helper_inside_decode_impl_dispatches_by_receiver_and_target() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord JsonSpecConfig(name: String, entrypoint: String)

impl Decode<JsonSpecConfig> for JsonValue {
  def decode(self: Self) -> Result<JsonSpecConfig, Error> {
    name_json =? Json::get(self, "name")
    name =? Decode::decode::<String>(name_json)
    entry_json =? Json::get(self, "entrypoint")
    entry =? entry_json |> Decode::decode::<String>
    Ok(JsonSpecConfig(name, entry))
  }
}

cfg = Decode::decode::<JsonSpecConfig>(JsonValue::Null)"#,
    );
    let mut calls = Vec::new();
    for node in &typed {
        collect_decode_trait_calls(node, &mut calls);
    }
    let string_decode_calls = calls
        .iter()
        .filter(|(trait_name, dispatch_name)| {
            trait_name.as_str() == "Decode<String>"
                && dispatch_name
                    .as_deref()
                    .is_some_and(|name| name.ends_with("JsonValue::decode"))
        })
        .count();
    assert_eq!(
        string_decode_calls, 2,
        "nested direct and pipeline decode calls should dispatch to String decoder: {calls:?}"
    );
    assert!(
        calls.iter().any(|(trait_name, dispatch_name)| {
            trait_name.as_str() == "Decode<JsonSpecConfig>"
                && dispatch_name
                    .as_deref()
                    .is_some_and(|name| name.ends_with("JsonValue::decode"))
        }),
        "custom Config decoder should still be registered as its own dispatch target: {calls:?}"
    );
}

fn decode_helper_allows_same_pattern_recursive_dispatch() {
    typecheck_with_builtin_prelude(
        r#"defrecord JsonSpecRecursive(value: String)

impl Decode<JsonSpecRecursive> for JsonValue {
  def decode(self: Self) -> Result<JsonSpecRecursive, Error> {
    Decode::decode::<JsonSpecRecursive>(self)
  }
}"#,
    );
}

fn encode_helper_dispatches_to_receiver_impl_with_json_value_target() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord JsonSpecConfig(name: String, entrypoint: String)

impl Encode<JsonValue> for JsonSpecConfig {
  def encode(self: Self) -> Result<JsonValue, Error> {
    Ok(JsonValue::String(self.name))
  }
}

json = Encode::encode::<JsonValue>(JsonSpecConfig("surtr", "boot"))"#,
    );
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            trait_name,
            method_name,
            dispatch:
                scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
                    name,
                    ..
                }),
            args,
            ..
        } => {
            assert_eq!(trait_name, "Encode<JsonValue>");
            assert_eq!(method_name, "encode");
            assert_eq!(name, "Encode<JsonValue>::Global::JsonSpecConfig::encode");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

fn encode_helper_allows_same_pattern_recursive_dispatch() {
    typecheck_with_builtin_prelude(
        r#"defrecord JsonSpecRecursive(value: String)

impl Encode<JsonValue> for JsonSpecRecursive {
  def encode(self: Self) -> Result<JsonValue, Error> {
    Encode::encode::<JsonValue>(self)
  }
}"#,
    );
}

fn collect_decode_trait_calls(node: &TypedNode, calls: &mut Vec<(String, Option<String>)>) {
    match &node.node {
        TypedInner::TraitCall {
            trait_name,
            method_name,
            dispatch,
            args,
            ..
        } => {
            if method_name == "decode" {
                calls.push((trait_name.clone(), trait_dispatch_name(dispatch)));
            }
            for arg in args {
                collect_decode_trait_calls(arg, calls);
            }
        }
        TypedInner::App(func, args) | TypedInner::InjectCall(func, args) => {
            collect_decode_trait_calls(func, calls);
            for arg in args {
                collect_decode_trait_calls(arg, calls);
            }
        }
        TypedInner::Block(stmts) => {
            for stmt in stmts {
                collect_decode_trait_calls(stmt, calls);
            }
        }
        TypedInner::Bind(_, rhs) | TypedInner::SafeBind(_, rhs) => {
            collect_decode_trait_calls(rhs, calls);
        }
        TypedInner::Def(_, _, _, _, _, _, body, _)
        | TypedInner::ExtractorDef(_, _, _, _, _, body, _) => {
            collect_decode_trait_calls(body, calls);
        }
        TypedInner::Closure(_, _, body)
        | TypedInner::Semi(body)
        | TypedInner::FieldAccess(body, _) => {
            collect_decode_trait_calls(body, calls);
        }
        TypedInner::Pipe(left, right)
        | TypedInner::BinOp(_, left, right)
        | TypedInner::Compose(_, left, right)
        | TypedInner::ListCons(left, right) => {
            collect_decode_trait_calls(left, calls);
            collect_decode_trait_calls(right, calls);
        }
        TypedInner::ConstructorCall(_, args)
        | TypedInner::ListLiteral(args)
        | TypedInner::TupleLiteral(args) => {
            for arg in args {
                collect_decode_trait_calls(arg, calls);
            }
        }
        TypedInner::If(cond, then_branch, else_branch) => {
            collect_decode_trait_calls(cond, calls);
            collect_decode_trait_calls(then_branch, calls);
            if let Some(else_branch) = else_branch {
                collect_decode_trait_calls(else_branch, calls);
            }
        }
        _ => {}
    }
}

fn trait_dispatch_name(dispatch: &scar::typed::TraitDispatch) -> Option<String> {
    match dispatch {
        scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
            name,
            ..
        }) => Some(name.clone()),
        _ => None,
    }
}

fn from_helper_suggests_try_from_when_only_fallible_impl_exists() {
    let resolved = resolve_with_builtin_prelude(r#"value = from::<Int>("42")"#);
    let err = typecheck(resolved).expect_err("from on fallible conversion must fail");
    assert!(err
        .message
        .contains("String -> Int implements TryFrom, not From"));
    assert!(err.message.contains("Use try_from::<Int>(value)."));
}

fn try_from_helper_suggests_from_when_only_infallible_impl_exists() {
    let resolved = resolve_with_builtin_prelude(r#"value = try_from::<String>(42)"#);
    let err = typecheck(resolved).expect_err("try_from on infallible conversion must fail");
    assert!(err
        .message
        .contains("Int -> String implements From, not TryFrom"));
    assert!(err.message.contains("Use from::<String>(value)."));
}

fn from_and_try_from_impls_are_mutually_exclusive() {
    let overrides = [
        (
            "String",
            r#"@builtin type String

defenum StringEncoding {
  Utf8,
  Ascii,
}

deferror InvalidStringEncoding(detail: String) {
  detail
}

impl String {
  @builtin
  def codepoints(value: String, encoding: StringEncoding) -> Result<List<Int>, InvalidStringEncoding>

  @builtin
  def from_codepoints(values: List<Int>, encoding: StringEncoding) -> Result<String, InvalidStringEncoding>
}

impl Show for String {
  def to_string(self: Self) -> String {
inspect(self)
  }
}

impl From<String> for String {
  def from(self: Self) -> String {
self
  }
}

impl TryFrom<Int> for String {
  def try_from(self: Self) -> Result<Int, Error> {
Ok(0)
  }
}

impl From<Int> for String {
  def from(self: Self) -> Int {
0
  }
}

impl Eq for String {
  def eq(self: Self, rhs: Self) -> Boolean {
self == rhs
  }

  def neq(self: Self, rhs: Self) -> Boolean {
self != rhs
  }
}"#,
        ),
        ("StyledDoc", "defmod StyledDoc {}"),
        ("Test", "defmod Test {}"),
    ];

    let err = typecheck_std_modules_with_overrides(&overrides)
        .expect_err("conflicting From/TryFrom impls must fail");
    assert!(err
        .message
        .contains("From and TryFrom cannot both be implemented for String -> Int"));
}

fn process_sleep_accepts_duration_literal() {
    let typed = typecheck_with_builtin_prelude(r#"value = Process::sleep(100ms)"#);
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    assert!(matches!(rhs.ty, scar::types::Ty::Result(_, _)));
}

fn process_self_is_rejected_outside_process_context() {
    let resolved = resolve_with_builtin_prelude(r#"pid = Process::self()"#);
    let err = typecheck(resolved).expect_err("Process::self outside process must fail");
    assert!(err.message.contains("Process::self"));
}

fn process_self_typechecks_inside_process_handler() {
    let mut stages = std_module_stages();
    stages.push(vec![staged_process_module(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<PID<Counter>> {
    Ok(Process::self())
  }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let resolved =
        sigil::resolve_staged_program_with_state(&stages, Vec::new(), &declaration_index, None)
            .expect("resolve should succeed");
    scar::typecheck_staged_program(resolved)
        .expect("Process::self should typecheck inside process handler");
}

fn singleton_agent_pid_surface_returns_concrete_pid() {
    let mut stages = std_module_stages();
    stages.push(vec![staged_process_module(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let user_ast =
        spire::parse_with_context("pid = Counter::pid()", spire::ParserContext::project(0))
            .expect("script should parse");
    let resolved = sigil::resolve_staged_program_with_state(
        &stages,
        user_ast,
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
    .expect("resolve should succeed");
    let typed = scar::typecheck_staged_program(resolved).expect("typecheck should succeed");
    let rhs = typed
        .nodes
        .last()
        .and_then(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected pid binding");
    match &rhs.ty {
        Ty::Pid(symbol) => assert!(symbol == "Counter" || symbol == "Global::Counter"),
        other => panic!("expected PID<Counter>, got {other:?}"),
    }
}

fn singleton_genserver_pid_surface_returns_concrete_pid() {
    let mut stages = std_module_stages();
    stages.push(vec![staged_process_module(
        r#"defgenserver QueueServer {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @call
  def size(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let user_ast =
        spire::parse_with_context("pid = QueueServer::pid()", spire::ParserContext::project(0))
            .expect("script should parse");
    let resolved = sigil::resolve_staged_program_with_state(
        &stages,
        user_ast,
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
    .expect("resolve should succeed");
    let typed = scar::typecheck_staged_program(resolved).expect("typecheck should succeed");
    let rhs = typed
        .nodes
        .last()
        .and_then(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected pid binding");
    match &rhs.ty {
        Ty::Pid(symbol) => assert!(symbol == "QueueServer" || symbol == "Global::QueueServer"),
        other => panic!("expected PID<QueueServer>, got {other:?}"),
    }
}

fn singleton_agent_explicit_pid_call_typechecks() {
    let mut stages = std_module_stages();
    stages.push(vec![staged_process_module(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let user_ast = spire::parse_with_context(
        r#"pid = Counter::pid()
value =? Counter::get(pid, "count")
done =? Counter::set(pid, 1)"#,
        spire::ParserContext::project(0),
    )
    .expect("script should parse");
    let resolved = sigil::resolve_staged_program_with_state(
        &stages,
        user_ast,
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
    .expect("resolve should succeed");
    scar::typecheck_staged_program(resolved)
        .expect("singleton explicit pid-first agent surface should typecheck");
}

fn genserver_additional_call_handler_typechecks_as_process_context() {
    let mut stages = std_module_stages();
    stages.push(vec![staged_process_module(
        r#"defgenserver Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
    handlers {
      out: OutHandler = StdOut
    }
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @call
  def info(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }

  @call
  def log(state: Int, message: String) -> Result<CallResult<Unit, Int>> {
    _handler = ctx.out
    _message = message
    Ok(CallResult::Reply((), state))
  }
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let user_ast = spire::parse_with_context(
        r#"supervisor_init {
  Logger {}
}
info = Logger::info()
done = Logger::log("hello")"#,
        spire::ParserContext::project(0),
    )
    .expect("script should parse");
    let resolved = sigil::resolve_staged_program_with_state(
        &stages,
        user_ast,
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
    .expect("resolve should succeed");
    scar::typecheck_staged_program(resolved)
        .expect("additional @call handler should have process context access");
}

fn genserver_call_handler_accepts_call_result_contract() {
    let mut stages = std_module_stages();
    stages.push(vec![staged_process_module(
        r#"defgenserver Logger {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @call
  def info(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }

  @cast
  def reset(_state: Int, next: Int) -> Result<CastResult<Int>> {
    Ok(CastResult::Next(next))
  }
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let resolved =
        sigil::resolve_staged_program_with_state(&stages, Vec::new(), &declaration_index, None)
            .expect("resolve should succeed");
    scar::typecheck_staged_program(resolved)
        .expect("CallResult/CastResult handlers should typecheck");
}

fn process_meta_state_mismatch_is_rejected() {
    let mut stages = std_module_stages();
    stages.push(vec![staged_process_module(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: String) -> Result<Int> { Ok(0) }
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let resolved =
        sigil::resolve_staged_program_with_state(&stages, Vec::new(), &declaration_index, None)
            .expect("resolve should succeed");
    let err =
        scar::typecheck_staged_program(resolved).expect_err("meta.state mismatch should fail");

    assert!(err.message.contains(
        "@get handler `Counter::get` first parameter must match process state type `Int`"
    ));
}

fn user_defined_process_state_can_appear_in_public_signatures() {
    let user_ast = spire::parse_with_context(
        r#"defstruct CounterState {
  value: Int,
}

impl CounterState {
  def new(value: Int) -> Self {
    CounterState { value: value }
  }
}

defmod Helper {
  def expose(state: CounterState) -> CounterState {
    state
  }
}"#,
        spire::ParserContext::module(0, None),
    )
    .expect("source should parse");
    let mut stages = std_module_stages();
    let mut user_modules = Vec::new();
    let mut global_ast = Vec::new();
    for stmt in user_ast {
        match stmt {
            spire::ast::Ast::Defmod(_, module_path, ast, attrs) => {
                user_modules.push(sigil::StagedModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast,
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: None,
                });
            }
            other => global_ast.push(other),
        }
    }
    if !global_ast.is_empty() {
        user_modules.push(sigil::StagedModuleAst {
            module_path: String::new(),
            doc_module_path: None,
            ast: global_ast,
            module_doc: None,
            auto_import: false,
            process_spec: None,
        });
    }
    user_modules.push(staged_process_module(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: CounterState
  }

  @init
  def init() -> Result<CounterState> { Ok(CounterState::new(0)) }

  @get
  def get(state: CounterState) -> Result<CounterState> { Ok(state) }
}"#,
    ));
    stages.push(user_modules);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let resolved =
        sigil::resolve_staged_program_with_state(&stages, Vec::new(), &declaration_index, None)
            .expect("resolve should succeed");

    scar::typecheck_staged_program(resolved)
        .expect("user-defined process state should be allowed in public signatures");
}

fn typecheck_staged_program_keeps_process_specs() {
    let ast = spire::parse_with_context(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        spire::ParserContext::module(0, Some("Counter".to_string())),
    )
    .expect("defagent source should parse");

    let staged_module = match ast.into_iter().next().expect("lowered module should exist") {
        spire::ast::Ast::Defagent(_, module_path, ast, process_spec, attrs) => {
            sigil::StagedModuleAst {
                module_path,
                doc_module_path: None,
                ast,
                module_doc: attrs.doc,
                auto_import: attrs.auto_import,
                process_spec: Some(process_spec),
            }
        }
        other => panic!("expected defagent, got {other:?}"),
    };

    let mut stages = std_module_stages();
    stages.push(vec![staged_module]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let resolved =
        sigil::resolve_staged_program_with_state(&stages, Vec::new(), &declaration_index, None)
            .expect("resolve should succeed");
    let typed: TypedProgram =
        scar::typecheck_staged_program(resolved).expect("typecheck should succeed");

    assert_eq!(typed.process_specs.len(), 2);
    let spec = typed
        .process_specs
        .iter()
        .find(|spec| spec.process_name == "Counter" || spec.process_name == "Global::Counter")
        .expect("Counter process spec should exist");
    assert!(spec.module_path == "Counter" || spec.module_path == "Global::Counter");
    assert!(spec.process_name == "Counter" || spec.process_name == "Global::Counter");
    assert!(!spec.spec.boot);
}

fn staged_process_module(source: &str) -> sigil::StagedModuleAst {
    let ast = spire::parse_with_context(source, spire::ParserContext::module(0, None))
        .expect("process source should parse");
    match ast
        .into_iter()
        .next()
        .expect("lowered process should exist")
    {
        spire::ast::Ast::Defagent(_, module_path, ast, process_spec, attrs)
        | spire::ast::Ast::Defgenserver(_, module_path, ast, process_spec, attrs)
        | spire::ast::Ast::Defsupervisor(_, module_path, ast, process_spec, attrs) => {
            sigil::StagedModuleAst {
                module_path,
                doc_module_path: None,
                ast,
                module_doc: attrs.doc,
                auto_import: attrs.auto_import,
                process_spec: Some(process_spec),
            }
        }
        other => panic!("expected process module, got {other:?}"),
    }
}

fn typecheck_supervisor_spawn_fixture(
    script: &str,
) -> Result<TypedProgram, scar::error::TypeError> {
    let mut stages = std_module_stages();
    stages.push(vec![
        staged_process_module(
            r#"defagent MyWorker {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }

  @init
  def init(seed: Int) -> Result<Int> { Ok(seed) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        ),
        staged_process_module(
            r#"defsupervisor MySup {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}"#,
        ),
        staged_process_module(
            r#"defsupervisor LockedSup {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Temporary
    allow_adopt: False
  }
}"#,
        ),
    ]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let project_source = format!(
        r#"supervisor_init {{
  MySup {{}}
  LockedSup {{}}
  DynamicSupervisor {{}}
}}

{script}"#
    );
    let user_ast = spire::parse_with_context(&project_source, spire::ParserContext::project(0))
        .expect("script should parse");
    let resolved = sigil::resolve_staged_program_with_state(
        &stages,
        user_ast,
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
    .expect("resolve should succeed");
    scar::typecheck_staged_program(resolved)
}

fn typecheck_supervisor_pool_fixture(
    pool_source: &str,
) -> Result<TypedProgram, scar::error::TypeError> {
    let mut stages = std_module_stages();
    stages.push(vec![
        staged_process_module(
            r#"defagent MyWorker {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }

  @init
  def init(seed: Int) -> Result<Int> { Ok(seed) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        ),
        staged_process_module(
            r#"defsupervisor MySup {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}"#,
        ),
        staged_process_module(pool_source),
    ]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let user_ast = spire::parse_with_context(
        r#"supervisor_init {
  MySup {}
  MyPool {}
}"#,
        spire::ParserContext::project(0),
    )
    .expect("script should parse");
    let resolved = sigil::resolve_staged_program_with_state(
        &stages,
        user_ast,
        &declaration_index,
        Some("__Script::fixture".to_string()),
    )
    .expect("resolve should succeed");
    scar::typecheck_staged_program(resolved)
}

fn dynsup_spawn_accepts_worker_init_route_reference() {
    let typed =
        typecheck_supervisor_spawn_fixture(r#"pid = DynamicSupervisor::spawn(MyWorker::init(1))"#)
            .expect("DynSup spawn should typecheck");
    assert!(!typed.nodes.is_empty());
}

fn custom_supervisor_spawn_accepts_worker_init_route_reference() {
    let typed = typecheck_supervisor_spawn_fixture(r#"pid = MySup::spawn(MyWorker::init(1))"#)
        .expect("custom supervisor spawn should typecheck");
    assert!(!typed.nodes.is_empty());
}

fn supervisor_spawn_rejects_plain_closure_argument() {
    let err = typecheck_supervisor_spawn_fixture(r#"pid = MySup::spawn({|| Ok(1)})"#)
        .expect_err("plain closure should be rejected");
    assert!(err.message.contains("worker init"));
}

fn supervisor_spawn_rejects_non_worker_callable() {
    let err = typecheck_supervisor_spawn_fixture(r#"pid = MySup::spawn(MySup::status())"#)
        .expect_err("non-worker callable should be rejected");
    assert!(err.message.contains("worker init"));
}

fn supervisor_adopt_accepts_worker_pid() {
    let typed = typecheck_supervisor_spawn_fixture(
        r#"pid =? MySup::spawn(MyWorker::init(1))
_ =? MySup::adopt(pid)"#,
    )
    .expect("adopt should typecheck");
    assert!(!typed.nodes.is_empty());
}

fn supervisor_adopt_rejects_non_pid_argument() {
    let err = typecheck_supervisor_spawn_fixture(r#"_ =? MySup::adopt(1)"#)
        .expect_err("adopt should reject non pid");
    assert!(err.message.contains("PID"));
}

fn supervisor_adopt_rejects_when_policy_disallows_it() {
    let err = typecheck_supervisor_spawn_fixture(
        r#"pid =? MySup::spawn(MyWorker::init(1))
_ =? LockedSup::adopt(pid)"#,
    )
    .expect_err("adopt should respect allow_adopt");
    assert!(err.message.contains("allow_adopt"));
}

fn supervisor_status_returns_supervisor_status() {
    let typed = typecheck_supervisor_spawn_fixture(r#"status =? MySup::status()"#)
        .expect("status should typecheck");
    assert!(!typed.nodes.is_empty());
}

fn supervisor_workers_returns_workers_handle() {
    let typed = typecheck_supervisor_pool_fixture(
        r#"defgenserver MyPool {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Workers<MyWorker>
  }

  @init
  def init() -> Result<Workers<MyWorker>> {
    MySup::workers(MyWorker::init(1), WorkerStrategy::fixed(2))
  }

  @call
  def count(workers: Workers<MyWorker>) -> Result<CallResult<Int, Workers<MyWorker>>> {
    Ok(CallResult::Reply(Workers::size(workers), workers))
  }
}"#,
    )
    .expect("workers creation should typecheck");
    assert!(!typed.nodes.is_empty());
}

fn workers_submit_accepts_worker_message_template() {
    let typed = typecheck_supervisor_pool_fixture(
        r#"defgenserver MyPool {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Workers<MyWorker>
  }

  @init
  def init() -> Result<Workers<MyWorker>> {
    MySup::workers(MyWorker::init(1), WorkerStrategy::fixed(2))
  }

  @call
  def count(workers: Workers<MyWorker>) -> Result<CallResult<Int, Workers<MyWorker>>> {
    Ok(CallResult::Reply(Workers::size(workers), workers))
  }

  @cast
  def submit(workers: Workers<MyWorker>) -> Result<CastResult<Workers<MyWorker>>> {
    _ =? Workers::submit(workers, MyWorker::set(3))
    Ok(CastResult::Next(workers))
  }
}"#,
    )
    .expect("workers submit should accept worker message template");
    assert!(!typed.nodes.is_empty());
}

fn workers_broadcast_accepts_worker_message_template() {
    let typed = typecheck_supervisor_pool_fixture(
        r#"defgenserver MyPool {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Workers<MyWorker>
  }

  @init
  def init() -> Result<Workers<MyWorker>> {
    MySup::workers(MyWorker::init(1), WorkerStrategy::fixed(2))
  }

  @call
  def values(workers: Workers<MyWorker>) -> Result<CallResult<List<Result<Int>>, Workers<MyWorker>>> {
    Ok(CallResult::Reply(Workers::broadcast(workers, MyWorker::get("jobs")), workers))
  }
}"#,
    )
    .expect("workers broadcast should accept worker message template");
    assert!(!typed.nodes.is_empty());
}

fn task_await_accepts_task_handle() {
    let typed = typecheck_with_builtin_prelude(
        r#"task = Task::async({|| Ok("ready")})
value =? Task::await(task)"#,
    );
    assert!(!typed.is_empty());
}

fn workers_reserve_can_flow_into_worker_call() {
    let typed = typecheck_supervisor_pool_fixture(
        r#"defgenserver MyPool {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Workers<MyWorker>
  }

  @init
  def init() -> Result<Workers<MyWorker>> {
    MySup::workers(MyWorker::init(1), WorkerStrategy::fixed(2))
  }

  @call
  def reserve_set(workers: Workers<MyWorker>) -> Result<CallResult<Unit, Workers<MyWorker>>> {
    lease =? Workers::reserve(workers)
    _ =? MyWorker::set(lease, 9)
    Ok(CallResult::Reply((), workers))
  }
}"#,
    )
    .expect("workers reserve should typecheck as worker capability");
    assert!(!typed.nodes.is_empty());
}

fn tap_err_accepts_local_error_observer_binding() {
    let typed = typecheck_with_builtin_prelude(
        r#"handler = {|err| eprint(err)}
value = Result::tap_err(Err(NoneError), handler)"#,
    );
    assert!(!typed.is_empty());
}

fn tap_err_accepts_error_observer_captures_and_composition() {
    let typed = typecheck_with_builtin_prelude(
        r#"logged = Result::tap_err(Err(NoneError), &eprint)
named = Result::tap_err(Err(NoneError), &Error::kind >> &print)"#,
    );
    assert!(!typed.is_empty());
}

fn error_observer_binding_cannot_escape_as_plain_value() {
    let resolved = resolve_with_builtin_prelude(
        r#"handler = {|err| eprint(err)}
escaped = handler"#,
    );
    let err = typecheck(resolved).expect_err("Error observer binding must not escape");
    assert!(err.message.contains("Error observer closure cannot escape"));
}

fn error_observer_binding_cannot_be_called_directly() {
    let resolved = resolve_with_builtin_prelude(
        r#"handler = {|err| eprint(err)}
value = match Err(NoneError) {
  Ok(_) => (),
  Err(err) => handler(err),
}"#,
    );
    let err =
        typecheck(resolved).expect_err("Error observer binding must not be callable directly");
    assert!(err
        .message
        .contains("Error observer closure can only be passed"));
}

fn error_observer_binding_cannot_use_error_annotation() {
    let resolved = resolve_with_builtin_prelude(
        r#"handler: (Error -> Unit) = {|err| eprint(err)}
value = Result::tap_err(Err(NoneError), handler)"#,
    );
    let err = typecheck(resolved).expect_err("Error observer binding annotation must fail");
    assert!(err
        .message
        .contains("Error cannot be used as a user-defined function parameter type"));
}

fn error_observer_closure_param_cannot_use_error_annotation() {
    let resolved = resolve_with_builtin_prelude(
        r#"handler = {|err: Error| eprint(err)}
value = Result::tap_err(Err(NoneError), handler)"#,
    );
    let err = typecheck(resolved).expect_err("Error observer closure param annotation must fail");
    assert!(err
        .message
        .contains("Error cannot be used as a user-defined function parameter type"));
}

fn error_observer_binding_cannot_flow_through_generic_identity() {
    let resolved = resolve_with_builtin_prelude(
        r#"def id(value: $A) -> $A { value }
handler = {|err| eprint(err)}
value = Result::tap_err(Err(NoneError), id(handler))"#,
    );
    let err = typecheck(resolved).expect_err("Error observer binding must be a direct argument");
    assert!(err.message.contains("Error observer closure cannot escape"));
}

#[test]
fn explicit_type_arguments_specialize_functions_trait_calls_and_captures() {
    let typed = typecheck_with_builtin_prelude(
        r#"def identity(value: $A) -> $A { value }

deftrait Convert<$To> {
  def convert(self: Self) -> $To
}

impl Convert<Int> for String {
  def convert(self: String) -> Int { 1 }
}

number: Int = identity::<Int>(1)
identity_fn: (Int -> Int) = &identity::<Int>
converted: Int = Convert::convert::<Int>("")
convert_fn: (String -> Int) = &Convert::convert::<Int>
again: Int = convert_fn("")"#,
    );
    assert!(!typed.is_empty());

    let resolved = resolve_with_builtin_prelude(
        r#"def identity(value: $A) -> $A { value }
bad: String = identity::<Int>(1)"#,
    );
    let err = typecheck(resolved).expect_err("explicit Int must not satisfy String");
    assert!(err.message.contains("expected String, got Int"), "{err:?}");
}

#[test]
fn explicit_type_arguments_exclude_self_and_enforce_generic_arity() {
    let resolved =
        resolve_with_builtin_prelude(r#"value = Concat::concat::<String>("left", "right")"#);
    let err = typecheck(resolved).expect_err("Self must not be supplied as an explicit type input");
    assert!(
        err.message
            .contains("Concat::concat expects 0 explicit type argument(s), got 1"),
        "{err:?}"
    );

    let resolved = resolve_with_builtin_prelude(r#"value = TryFrom::try_from::<Int, String>("1")"#);
    let err = typecheck(resolved).expect_err("trait generics must use their declared arity");
    assert!(
        err.message
            .contains("TryFrom::try_from expects 1 explicit type argument(s), got 2"),
        "{err:?}"
    );
}

#[test]
fn explicit_function_type_arguments_follow_signature_order() {
    typecheck_with_rules(
        r#"def pair(left: $A, right: $B) -> ($A, $B) {
  (left, right)
}

value: (Int, String) = pair::<Int, String>(1, "ok")"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("implicit generic slots should follow their first signature appearance");

    typecheck_with_rules(
        r#"def reversed<$B, $A>(left: $A, right: $B) -> ($A, $B) {
  (left, right)
}

value: (String, Int) = reversed::<Int, String>("ok", 1)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("declared generic slots should follow their declaration order");
}
