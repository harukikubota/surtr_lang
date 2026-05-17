use crate::error::RuntimeError;
use crate::value::Value;
use crate::vm::{TaskMode, VmFileError, VmFileMode, VM};
use num_bigint::{BigInt, BigUint, Sign};
use regex::Regex;
use sindr::builtin::{builtin_meta_by_id, BUILTIN_METAS};
use sindr::names::surface_path_name;
use sindr::primitives::{int, SurtrInt, ToPrimitive, Zero};
use sindr::runtime::{
    quote_surtr_string_literal, Callable, FileHandleValue, HashMapHandle, ListHandle, Location,
    RandomGeneratorHandle, RegexCapturesHandle, RegexHandle, RegexMatchHandle, RichError,
    TypeEntry,
};
use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Read};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Function pointer type for built-in implementations.
pub type BuiltinFn = fn(&mut VM, Vec<Value>) -> Result<Value, RuntimeError>;

struct BuiltinImpl {
    name: &'static str,
    func: BuiltinFn,
}

// Eldr keeps implementation pointers only. Metadata lives in sindr::builtin.
const BUILTIN_IMPLS: &[BuiltinImpl] = &[
    BuiltinImpl {
        name: "print",
        func: builtin_print,
    },
    BuiltinImpl {
        name: "to_string",
        func: builtin_to_string,
    },
    BuiltinImpl {
        name: "inspect",
        func: builtin_inspect,
    },
    BuiltinImpl {
        name: "safe_div",
        func: builtin_safe_div,
    },
    BuiltinImpl {
        name: "safe_mod",
        func: builtin_safe_mod,
    },
    BuiltinImpl {
        name: "eprint",
        func: builtin_eprint,
    },
    BuiltinImpl {
        name: "set_exit_code",
        func: builtin_set_exit_code,
    },
    BuiltinImpl {
        name: "shl",
        func: builtin_shl,
    },
    BuiltinImpl {
        name: "shr",
        func: builtin_shr,
    },
    BuiltinImpl {
        name: "len",
        func: builtin_list_len,
    },
    BuiltinImpl {
        name: "gen_make",
        func: builtin_gen_make,
    },
    BuiltinImpl {
        name: "gen_idx",
        func: builtin_gen_idx,
    },
    BuiltinImpl {
        name: "gen_items",
        func: builtin_gen_items,
    },
    BuiltinImpl {
        name: "bit_and",
        func: builtin_bit_and,
    },
    BuiltinImpl {
        name: "bit_or",
        func: builtin_bit_or,
    },
    BuiltinImpl {
        name: "bit_xor",
        func: builtin_bit_xor,
    },
    BuiltinImpl {
        name: "bit_not",
        func: builtin_bit_not,
    },
    BuiltinImpl {
        name: "test_bit",
        func: builtin_test_bit,
    },
    BuiltinImpl {
        name: "set_bit",
        func: builtin_set_bit,
    },
    BuiltinImpl {
        name: "clear_bit",
        func: builtin_clear_bit,
    },
    BuiltinImpl {
        name: "toggle_bit",
        func: builtin_toggle_bit,
    },
    BuiltinImpl {
        name: "codepoints",
        func: builtin_codepoints,
    },
    BuiltinImpl {
        name: "from_codepoints",
        func: builtin_from_codepoints,
    },
    BuiltinImpl {
        name: "map_err",
        func: builtin_result_map_err,
    },
    BuiltinImpl {
        name: "cause",
        func: builtin_result_cause,
    },
    BuiltinImpl {
        name: "chain",
        func: builtin_result_chain,
    },
    BuiltinImpl {
        name: "__recover_kind",
        func: builtin_result_recover_kind,
    },
    BuiltinImpl {
        name: "__test_push",
        func: builtin_test_push,
    },
    BuiltinImpl {
        name: "__test_pop",
        func: builtin_test_pop,
    },
    BuiltinImpl {
        name: "__test_pass",
        func: builtin_test_pass,
    },
    BuiltinImpl {
        name: "__test_fail",
        func: builtin_test_fail,
    },
    BuiltinImpl {
        name: "__test_fail_error",
        func: builtin_test_fail_error,
    },
    BuiltinImpl {
        name: "__test_fail_current",
        func: builtin_test_fail_current,
    },
    BuiltinImpl {
        name: "group_count",
        func: builtin_list_group_count,
    },
    BuiltinImpl {
        name: "zip",
        func: builtin_list_zip,
    },
    BuiltinImpl {
        name: "empty_map",
        func: builtin_empty_map,
    },
    BuiltinImpl {
        name: "map_from_entries",
        func: builtin_map_from_entries,
    },
    BuiltinImpl {
        name: "map_len",
        func: builtin_map_len,
    },
    BuiltinImpl {
        name: "map_contains_key",
        func: builtin_map_contains_key,
    },
    BuiltinImpl {
        name: "map_get",
        func: builtin_map_get,
    },
    BuiltinImpl {
        name: "map_insert",
        func: builtin_map_insert,
    },
    BuiltinImpl {
        name: "map_remove",
        func: builtin_map_remove,
    },
    BuiltinImpl {
        name: "map_keys",
        func: builtin_map_keys,
    },
    BuiltinImpl {
        name: "map_values_list",
        func: builtin_map_values_list,
    },
    BuiltinImpl {
        name: "view",
        func: builtin_facet_view,
    },
    BuiltinImpl {
        name: "preview",
        func: builtin_facet_preview,
    },
    BuiltinImpl {
        name: "__facet_chain",
        func: builtin_facet_compose,
    },
    BuiltinImpl {
        name: "__facet_put",
        func: builtin_facet_replace,
    },
    BuiltinImpl {
        name: "set",
        func: builtin_facet_set,
    },
    BuiltinImpl {
        name: "over",
        func: builtin_facet_over,
    },
    BuiltinImpl {
        name: "over_result",
        func: builtin_facet_over_result,
    },
    BuiltinImpl {
        name: "case_set",
        func: builtin_facet_case_set,
    },
    BuiltinImpl {
        name: "case_over",
        func: builtin_facet_case_over,
    },
    BuiltinImpl {
        name: "__facet_list_get",
        func: builtin_facet_list_get,
    },
    BuiltinImpl {
        name: "__facet_list_set",
        func: builtin_facet_list_set,
    },
    BuiltinImpl {
        name: "__facet_list_slice_get",
        func: builtin_facet_list_slice_get,
    },
    BuiltinImpl {
        name: "__facet_list_slice_set",
        func: builtin_facet_list_slice_set,
    },
    BuiltinImpl {
        name: "__facet_map_get",
        func: builtin_facet_map_get,
    },
    BuiltinImpl {
        name: "__facet_map_set_existing",
        func: builtin_facet_map_set_existing,
    },
    BuiltinImpl {
        name: "__test_capture_stdout",
        func: builtin_test_capture_stdout,
    },
    BuiltinImpl {
        name: "__test_capture_stderr",
        func: builtin_test_capture_stderr,
    },
    BuiltinImpl {
        name: "__test_push_stdin",
        func: builtin_test_push_stdin,
    },
    BuiltinImpl {
        name: "__test_begin_it",
        func: builtin_test_begin_it,
    },
    BuiltinImpl {
        name: "compile",
        func: builtin_regex_compile,
    },
    BuiltinImpl {
        name: "is_match",
        func: builtin_regex_is_match,
    },
    BuiltinImpl {
        name: "captures",
        func: builtin_regex_captures,
    },
    BuiltinImpl {
        name: "whole",
        func: builtin_regex_whole,
    },
    BuiltinImpl {
        name: "capture_count",
        func: builtin_regex_capture_count,
    },
    BuiltinImpl {
        name: "get",
        func: builtin_regex_get,
    },
    BuiltinImpl {
        name: "get_name",
        func: builtin_regex_get_name,
    },
    BuiltinImpl {
        name: "find",
        func: builtin_regex_find,
    },
    BuiltinImpl {
        name: "find_all",
        func: builtin_regex_find_all,
    },
    BuiltinImpl {
        name: "split",
        func: builtin_regex_split,
    },
    BuiltinImpl {
        name: "__regex_replace",
        func: builtin_regex_replace,
    },
    BuiltinImpl {
        name: "replace_all",
        func: builtin_regex_replace_all,
    },
    BuiltinImpl {
        name: "escape",
        func: builtin_regex_escape,
    },
    BuiltinImpl {
        name: "group_names",
        func: builtin_regex_group_names,
    },
    BuiltinImpl {
        name: "text",
        func: builtin_regex_match_text,
    },
    BuiltinImpl {
        name: "start",
        func: builtin_regex_match_start,
    },
    BuiltinImpl {
        name: "end",
        func: builtin_regex_match_end,
    },
    BuiltinImpl {
        name: "project_args",
        func: builtin_project_args,
    },
    BuiltinImpl {
        name: "io_get",
        func: builtin_io_get,
    },
    BuiltinImpl {
        name: "io_get_line",
        func: builtin_io_get_line,
    },
    BuiltinImpl {
        name: "file_read",
        func: builtin_file_read,
    },
    BuiltinImpl {
        name: "file_write",
        func: builtin_file_write,
    },
    BuiltinImpl {
        name: "file_append",
        func: builtin_file_append,
    },
    BuiltinImpl {
        name: "file_exists",
        func: builtin_file_exists,
    },
    BuiltinImpl {
        name: "file_delete",
        func: builtin_file_delete,
    },
    BuiltinImpl {
        name: "file_with_open",
        func: builtin_file_with_open,
    },
    BuiltinImpl {
        name: "file_read_chunk",
        func: builtin_file_read_chunk,
    },
    BuiltinImpl {
        name: "file_write_chunk",
        func: builtin_file_write_chunk,
    },
    BuiltinImpl {
        name: "file_flush",
        func: builtin_file_flush,
    },
    BuiltinImpl {
        name: "filesystem_path",
        func: builtin_filesystem_path,
    },
    BuiltinImpl {
        name: "filesystem_join",
        func: builtin_filesystem_join,
    },
    BuiltinImpl {
        name: "filesystem_parent",
        func: builtin_filesystem_parent,
    },
    BuiltinImpl {
        name: "filesystem_name",
        func: builtin_filesystem_name,
    },
    BuiltinImpl {
        name: "filesystem_extension",
        func: builtin_filesystem_extension,
    },
    BuiltinImpl {
        name: "filesystem_exists",
        func: builtin_filesystem_exists,
    },
    BuiltinImpl {
        name: "filesystem_stat",
        func: builtin_filesystem_stat,
    },
    BuiltinImpl {
        name: "filesystem_ls",
        func: builtin_filesystem_ls,
    },
    BuiltinImpl {
        name: "filesystem_tree_depth",
        func: builtin_filesystem_tree_depth,
    },
    BuiltinImpl {
        name: "filesystem_mkdir",
        func: builtin_filesystem_mkdir,
    },
    BuiltinImpl {
        name: "filesystem_mkdir_all",
        func: builtin_filesystem_mkdir_all,
    },
    BuiltinImpl {
        name: "filesystem_rm",
        func: builtin_filesystem_rm,
    },
    BuiltinImpl {
        name: "filesystem_mv",
        func: builtin_filesystem_mv,
    },
    BuiltinImpl {
        name: "filesystem_cp",
        func: builtin_filesystem_cp,
    },
    BuiltinImpl {
        name: "shell_pwd",
        func: builtin_shell_pwd,
    },
    BuiltinImpl {
        name: "shell_cd",
        func: builtin_shell_cd,
    },
    BuiltinImpl {
        name: "shell_exec",
        func: builtin_shell_exec,
    },
    BuiltinImpl {
        name: "seed",
        func: builtin_random_seed,
    },
    BuiltinImpl {
        name: "int_until",
        func: builtin_random_int_until,
    },
    BuiltinImpl {
        name: "int_range",
        func: builtin_random_int_range,
    },
    BuiltinImpl {
        name: "next_int_until",
        func: builtin_random_next_int_until,
    },
    BuiltinImpl {
        name: "next_int_range",
        func: builtin_random_next_int_range,
    },
    BuiltinImpl {
        name: "kind",
        func: builtin_error_kind,
    },
    BuiltinImpl {
        name: "message",
        func: builtin_error_message,
    },
    BuiltinImpl {
        name: "format",
        func: builtin_error_format,
    },
    BuiltinImpl {
        name: "__process_pid",
        func: builtin_process_pid,
    },
    BuiltinImpl {
        name: "__process_spawn",
        func: builtin_process_spawn,
    },
    BuiltinImpl {
        name: "__dynamic_supervisor_spawn",
        func: builtin_dynamic_supervisor_spawn,
    },
    BuiltinImpl {
        name: "__dynamic_supervisor_adopt",
        func: builtin_dynamic_supervisor_adopt,
    },
    BuiltinImpl {
        name: "__dynamic_supervisor_status",
        func: builtin_dynamic_supervisor_status,
    },
    BuiltinImpl {
        name: "__supervisor_spawn",
        func: builtin_supervisor_spawn,
    },
    BuiltinImpl {
        name: "__supervisor_adopt",
        func: builtin_supervisor_adopt,
    },
    BuiltinImpl {
        name: "__supervisor_status",
        func: builtin_supervisor_status,
    },
    BuiltinImpl {
        name: "__supervisor_workers",
        func: builtin_supervisor_workers,
    },
    BuiltinImpl {
        name: "__process_state",
        func: builtin_process_state,
    },
    BuiltinImpl {
        name: "__process_store",
        func: builtin_process_store,
    },
    BuiltinImpl {
        name: "__genserver_call_reply",
        func: builtin_genserver_call_reply,
    },
    BuiltinImpl {
        name: "__genserver_call_reply_later",
        func: builtin_genserver_call_reply_later,
    },
    BuiltinImpl {
        name: "__genserver_call_stop_normal",
        func: builtin_genserver_call_stop_normal,
    },
    BuiltinImpl {
        name: "__genserver_call_stop_error",
        func: builtin_genserver_call_stop_error,
    },
    BuiltinImpl {
        name: "__genserver_cast_next",
        func: builtin_genserver_cast_next,
    },
    BuiltinImpl {
        name: "__genserver_cast_stop_normal",
        func: builtin_genserver_cast_stop_normal,
    },
    BuiltinImpl {
        name: "__genserver_cast_stop_error",
        func: builtin_genserver_cast_stop_error,
    },
    BuiltinImpl {
        name: "__process_self",
        func: builtin_process_self,
    },
    BuiltinImpl {
        name: "__process_context_handler",
        func: builtin_process_context_handler,
    },
    BuiltinImpl {
        name: "__out_handler_write",
        func: builtin_out_handler_write,
    },
    BuiltinImpl {
        name: "__process_sleep",
        func: builtin_process_sleep,
    },
    BuiltinImpl {
        name: "Pending",
        func: builtin_process_init_pending,
    },
    BuiltinImpl {
        name: "PendingAfter",
        func: builtin_process_init_pending_after,
    },
    BuiltinImpl {
        name: "Ready",
        func: builtin_process_init_ready,
    },
    BuiltinImpl {
        name: "__task_call",
        func: builtin_task_call,
    },
    BuiltinImpl {
        name: "__task_async",
        func: builtin_task_async,
    },
    BuiltinImpl {
        name: "__task_await",
        func: builtin_task_await,
    },
    BuiltinImpl {
        name: "__task_launch",
        func: builtin_task_launch,
    },
    BuiltinImpl {
        name: "__task_cast",
        func: builtin_task_cast,
    },
    BuiltinImpl {
        name: "__task_call_timeout",
        func: builtin_task_call_timeout,
    },
    BuiltinImpl {
        name: "__task_async_timeout",
        func: builtin_task_async_timeout,
    },
    BuiltinImpl {
        name: "__task_await_timeout",
        func: builtin_task_await_timeout,
    },
    BuiltinImpl {
        name: "__task_launch_timeout",
        func: builtin_task_launch_timeout,
    },
    BuiltinImpl {
        name: "__task_cast_timeout",
        func: builtin_task_cast_timeout,
    },
    BuiltinImpl {
        name: "__workers_submit",
        func: builtin_workers_submit,
    },
    BuiltinImpl {
        name: "__workers_submit_timeout",
        func: builtin_workers_submit_timeout,
    },
    BuiltinImpl {
        name: "__workers_broadcast",
        func: builtin_workers_broadcast,
    },
    BuiltinImpl {
        name: "__workers_broadcast_timeout",
        func: builtin_workers_broadcast_timeout,
    },
    BuiltinImpl {
        name: "__workers_reserve",
        func: builtin_workers_reserve,
    },
    BuiltinImpl {
        name: "__workers_size",
        func: builtin_workers_size,
    },
    BuiltinImpl {
        name: "__operator_int_add",
        func: builtin_operator_int_add,
    },
    BuiltinImpl {
        name: "__operator_int_sub",
        func: builtin_operator_int_sub,
    },
    BuiltinImpl {
        name: "__operator_int_mul",
        func: builtin_operator_int_mul,
    },
    BuiltinImpl {
        name: "__operator_float_add",
        func: builtin_operator_float_add,
    },
    BuiltinImpl {
        name: "__operator_float_sub",
        func: builtin_operator_float_sub,
    },
    BuiltinImpl {
        name: "__operator_float_mul",
        func: builtin_operator_float_mul,
    },
    BuiltinImpl {
        name: "floor",
        func: builtin_float_floor,
    },
    BuiltinImpl {
        name: "ceil",
        func: builtin_float_ceil,
    },
    BuiltinImpl {
        name: "round",
        func: builtin_float_round,
    },
    BuiltinImpl {
        name: "trunc",
        func: builtin_float_trunc,
    },
    BuiltinImpl {
        name: "pi",
        func: builtin_float_pi,
    },
    BuiltinImpl {
        name: "e",
        func: builtin_float_e,
    },
    BuiltinImpl {
        name: "__operator_int_eq",
        func: builtin_operator_int_eq,
    },
    BuiltinImpl {
        name: "__operator_int_neq",
        func: builtin_operator_int_neq,
    },
    BuiltinImpl {
        name: "__operator_int_lt",
        func: builtin_operator_int_lt,
    },
    BuiltinImpl {
        name: "__operator_int_lte",
        func: builtin_operator_int_lte,
    },
    BuiltinImpl {
        name: "__operator_int_gt",
        func: builtin_operator_int_gt,
    },
    BuiltinImpl {
        name: "__operator_int_gte",
        func: builtin_operator_int_gte,
    },
    BuiltinImpl {
        name: "__operator_float_eq",
        func: builtin_operator_float_eq,
    },
    BuiltinImpl {
        name: "__operator_float_neq",
        func: builtin_operator_float_neq,
    },
    BuiltinImpl {
        name: "__operator_float_lt",
        func: builtin_operator_float_lt,
    },
    BuiltinImpl {
        name: "__operator_float_lte",
        func: builtin_operator_float_lte,
    },
    BuiltinImpl {
        name: "__operator_float_gt",
        func: builtin_operator_float_gt,
    },
    BuiltinImpl {
        name: "__operator_float_gte",
        func: builtin_operator_float_gte,
    },
    BuiltinImpl {
        name: "__compare_int",
        func: builtin_compare_int,
    },
    BuiltinImpl {
        name: "__compare_float",
        func: builtin_compare_float,
    },
    BuiltinImpl {
        name: "__ordering_is_lt",
        func: builtin_ordering_is_lt,
    },
    BuiltinImpl {
        name: "__ordering_is_lte",
        func: builtin_ordering_is_lte,
    },
    BuiltinImpl {
        name: "__ordering_is_gt",
        func: builtin_ordering_is_gt,
    },
    BuiltinImpl {
        name: "__ordering_is_gte",
        func: builtin_ordering_is_gte,
    },
    BuiltinImpl {
        name: "__operator_string_eq",
        func: builtin_operator_string_eq,
    },
    BuiltinImpl {
        name: "__operator_string_neq",
        func: builtin_operator_string_neq,
    },
    BuiltinImpl {
        name: "__operator_boolean_eq",
        func: builtin_operator_boolean_eq,
    },
    BuiltinImpl {
        name: "__operator_boolean_neq",
        func: builtin_operator_boolean_neq,
    },
    BuiltinImpl {
        name: "__operator_string_concat",
        func: builtin_operator_string_concat,
    },
    BuiltinImpl {
        name: "json_parse",
        func: builtin_json_parse,
    },
    BuiltinImpl {
        name: "json_stringify",
        func: builtin_json_stringify,
    },
    BuiltinImpl {
        name: "string_len",
        func: builtin_string_len,
    },
    BuiltinImpl {
        name: "string_contains",
        func: builtin_string_contains,
    },
    BuiltinImpl {
        name: "string_starts_with",
        func: builtin_string_starts_with,
    },
    BuiltinImpl {
        name: "string_ends_with",
        func: builtin_string_ends_with,
    },
    BuiltinImpl {
        name: "string_split",
        func: builtin_string_split,
    },
    BuiltinImpl {
        name: "string_replace",
        func: builtin_string_replace,
    },
];

const _: () = {
    assert!(BUILTIN_IMPLS.len() == BUILTIN_METAS.len());
};

pub(crate) fn call_builtin(
    vm: &mut VM,
    builtin_id: u16,
    args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    let meta = builtin_meta_by_id(builtin_id)
        .ok_or_else(|| RuntimeError::new(format!("Unknown builtin id: {}", builtin_id)))?;
    let expected_arity = expected_builtin_arity(meta.name, meta.arity);
    let arity_matches = if meta.name == "__supervisor_spawn" {
        matches!(args.len(), 2 | 3)
    } else if meta.name == "__supervisor_workers" {
        matches!(args.len(), 3 | 4)
    } else {
        args.len() == usize::from(meta.arity)
    };
    if !arity_matches {
        return Err(RuntimeError::new(format!(
            "builtin {} arity mismatch: expected {}, got {}",
            meta.name,
            expected_arity,
            args.len()
        )));
    }

    let builtin = BUILTIN_IMPLS.get(builtin_id as usize).ok_or_else(|| {
        RuntimeError::new(format!(
            "Missing builtin implementation for id {}",
            builtin_id
        ))
    })?;
    debug_assert_eq!(
        builtin.name, meta.name,
        "builtin implementation order drifted from BUILTIN_METAS"
    );

    (builtin.func)(vm, args)
}

fn expected_builtin_arity(name: &str, default_arity: u8) -> String {
    match name {
        "__supervisor_spawn" => "2 or 3".to_string(),
        "__supervisor_workers" => "3 or 4".to_string(),
        _ => default_arity.to_string(),
    }
}

fn builtin_print(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let s = match &args[0] {
        Value::Str(s) => s.clone(),
        other => inspect_value(vm, other),
    };
    vm.emit_stdout_line(s);
    Ok(Value::Unit)
}

fn builtin_to_string(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    Ok(Value::Str(args[0].to_display_string(vm.type_registry())))
}

fn builtin_inspect(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    Ok(Value::Str(inspect_value(vm, &args[0])))
}

fn builtin_error_kind(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let rich = decode_error_arg(&args[0], "kind", "err")?;
    Ok(Value::Str(surface_path_name(&rich.kind).to_string()))
}

fn builtin_error_message(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let rich = decode_error_arg(&args[0], "message", "err")?;
    Ok(Value::Str(rich.visible_message().to_string()))
}

fn builtin_error_format(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let rich = decode_error_arg(&args[0], "format", "err")?;
    Ok(Value::Str(rich.to_eprint_lines().join("\n")))
}

fn builtin_process_pid(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(process_name) = &args[0] else {
        return Err(RuntimeError::new("__process_pid expects String as name"));
    };
    let Value::Callable(init) = args[1].clone() else {
        return Err(RuntimeError::new(
            "__process_pid expects callable init handler",
        ));
    };
    vm.process_singleton_pid(process_name.clone(), init)
}

fn builtin_process_spawn(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(process_name) = &args[0] else {
        return Err(RuntimeError::new("__process_spawn expects String as name"));
    };
    let Value::Callable(init) = args[1].clone() else {
        return Err(RuntimeError::new(
            "__process_spawn expects callable init handler",
        ));
    };
    vm.process_spawn(process_name.clone(), init)
}

fn builtin_dynamic_supervisor_spawn(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Callable(init) = args[0].clone() else {
        return Err(RuntimeError::new(
            "__dynamic_supervisor_spawn expects callable init handler",
        ));
    };
    vm.dynamic_supervisor_spawn(init)
}

fn builtin_dynamic_supervisor_adopt(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Pid(pid) = &args[0] else {
        return Err(RuntimeError::new("__dynamic_supervisor_adopt expects PID"));
    };
    vm.supervisor_adopt("DynamicSupervisor".into(), pid.clone())
}

fn builtin_dynamic_supervisor_status(
    vm: &mut VM,
    _args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    vm.supervisor_status("DynamicSupervisor".into())
}

fn builtin_supervisor_spawn(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(supervisor_name) = &args[0] else {
        return Err(RuntimeError::new(
            "__supervisor_spawn expects String as supervisor name",
        ));
    };
    match args.as_slice() {
        [_, Value::Callable(init)] => {
            vm.supervisor_spawn(supervisor_name.clone(), None, init.clone())
        }
        [_, Value::Str(worker_name), Value::Callable(init)] => vm.supervisor_spawn(
            supervisor_name.clone(),
            Some(worker_name.clone()),
            init.clone(),
        ),
        [_, _, Value::Callable(_)] => Err(RuntimeError::new(
            "__supervisor_spawn expects String as worker name when provided",
        )),
        [_, Value::Callable(_), ..] => Err(RuntimeError::new(
            "__supervisor_spawn accepts at most 3 arguments",
        )),
        _ => Err(RuntimeError::new(
            "__supervisor_spawn expects callable init handler",
        )),
    }
}

fn builtin_supervisor_adopt(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(supervisor_name) = &args[0] else {
        return Err(RuntimeError::new(
            "__supervisor_adopt expects String as supervisor name",
        ));
    };
    let Value::Pid(pid) = &args[1] else {
        return Err(RuntimeError::new("__supervisor_adopt expects PID"));
    };
    vm.supervisor_adopt(supervisor_name.clone(), pid.clone())
}

fn builtin_supervisor_status(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(supervisor_name) = &args[0] else {
        return Err(RuntimeError::new(
            "__supervisor_status expects String as supervisor name",
        ));
    };
    vm.supervisor_status(supervisor_name.clone())
}

fn builtin_supervisor_workers(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(supervisor_name) = &args[0] else {
        return Err(RuntimeError::new(
            "__supervisor_workers expects String as supervisor name",
        ));
    };
    match args.as_slice() {
        [_, Value::Callable(init), strategy] => {
            let Some(worker_name) = vm.infer_worker_process_name_from_callable(init) else {
                return Err(RuntimeError::new(
                    "__supervisor_workers could not infer worker process from init callable",
                ));
            };
            vm.supervisor_workers(
                supervisor_name.clone(),
                worker_name,
                init.clone(),
                strategy.clone(),
            )
        }
        [_, Value::Str(worker_name), Value::Callable(init), strategy] => {
            vm.supervisor_workers(
                supervisor_name.clone(),
                worker_name.clone(),
                init.clone(),
                strategy.clone(),
            )
        }
        _ => Err(RuntimeError::new(
            "__supervisor_workers expects supervisor name, worker init callable, and WorkerStrategy",
        )),
    }
}

fn builtin_process_state(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new("__process_state expects PID"));
    };
    vm.process_state(&pid)
}

fn builtin_process_store(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new("__process_store expects PID"));
    };
    vm.process_store(&pid, args[1].clone())
}

fn builtin_genserver_call_reply(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new("__genserver_call_reply expects PID"));
    };
    vm.genserver_call_reply(&pid, args[1].clone(), args[2].clone())
}

fn builtin_genserver_call_reply_later(
    vm: &mut VM,
    args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new(
            "__genserver_call_reply_later expects PID",
        ));
    };
    let Value::Callable(callback) = args[2].clone() else {
        return Err(RuntimeError::new(
            "__genserver_call_reply_later expects callback callable",
        ));
    };
    vm.genserver_call_reply_later(&pid, args[1].clone(), callback)
}

fn builtin_genserver_call_stop_normal(
    vm: &mut VM,
    args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new(
            "__genserver_call_stop_normal expects PID",
        ));
    };
    vm.genserver_call_stop_normal(&pid, args[1].clone())
}

fn builtin_genserver_call_stop_error(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new("__genserver_call_stop_error expects PID"));
    };
    let Value::Error(err) = args[1].clone() else {
        return Err(RuntimeError::new(
            "__genserver_call_stop_error expects Error",
        ));
    };
    vm.genserver_call_stop_error(&pid, *err)
}

fn builtin_genserver_cast_next(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new("__genserver_cast_next expects PID"));
    };
    vm.genserver_cast_next(&pid, args[1].clone())
}

fn builtin_genserver_cast_stop_normal(
    vm: &mut VM,
    args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new(
            "__genserver_cast_stop_normal expects PID",
        ));
    };
    vm.genserver_cast_stop_normal(&pid)
}

fn builtin_genserver_cast_stop_error(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Some(pid) = vm.pid_handle_like(&args[0]) else {
        return Err(RuntimeError::new("__genserver_cast_stop_error expects PID"));
    };
    let Value::Error(err) = args[1].clone() else {
        return Err(RuntimeError::new(
            "__genserver_cast_stop_error expects Error",
        ));
    };
    vm.genserver_cast_stop_error(&pid, *err)
}

fn builtin_process_self(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Process::self must be lowered to a process-owned PID binding before runtime",
    ))
}

fn builtin_process_context_handler(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let process_name = decode_string_arg(&args[0], "__process_context_handler", "process_name")?;
    let slot = decode_string_arg(&args[1], "__process_context_handler", "slot")?;
    vm.process_context_handler(process_name.to_string(), slot.to_string())
}

fn builtin_out_handler_write(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Pid(pid) = &args[0] else {
        return Err(RuntimeError::new("__out_handler_write expects PID"));
    };
    let text = decode_string_arg(&args[1], "__out_handler_write", "text")?;
    vm.out_handler_write(pid, text.to_string())
}

fn builtin_process_sleep(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let millis = duration_to_u64(vm, &args[0], "__process_sleep", "duration")?;
    vm.process_sleep(millis)
}

fn builtin_process_init_pending(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Ok(Value::Tagged {
        tag: 0,
        fields: Vec::new(),
    })
}

fn builtin_process_init_pending_after(
    vm: &mut VM,
    args: Vec<Value>,
) -> Result<Value, RuntimeError> {
    let _ = duration_to_u64(vm, &args[0], "PendingAfter", "duration")?;
    Ok(Value::Tagged {
        tag: 1,
        fields: vec![args[0].clone()],
    })
}

fn builtin_process_init_ready(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    Ok(Value::Tagged {
        tag: 2,
        fields: vec![args[0].clone()],
    })
}

fn builtin_task_call(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    invoke_task_body(vm, &args[0], "__task_call", TaskMode::Call)
}

fn builtin_task_async(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    invoke_task_body(vm, &args[0], "__task_async", TaskMode::Async)
}

fn builtin_task_await(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    vm.await_task_handle(&args[0], None)
}

fn builtin_task_launch(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    invoke_task_body(vm, &args[0], "__task_launch", TaskMode::Launch)
}

fn builtin_task_cast(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    invoke_task_body(vm, &args[0], "__task_cast", TaskMode::Cast)
}

fn builtin_task_call_timeout(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    invoke_task_body_with_timeout(
        vm,
        &args[1],
        &args[0],
        "__task_call_timeout",
        TaskMode::Call,
    )
}

fn builtin_task_async_timeout(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    invoke_task_body_with_timeout(
        vm,
        &args[1],
        &args[0],
        "__task_async_timeout",
        TaskMode::Async,
    )
}

fn builtin_task_await_timeout(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let timeout_ms = duration_to_u64(vm, &args[0], "__task_await_timeout", "timeout")?;
    vm.await_task_handle(&args[1], Some(timeout_ms))
}

fn builtin_task_launch_timeout(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    invoke_task_body_with_timeout(
        vm,
        &args[1],
        &args[0],
        "__task_launch_timeout",
        TaskMode::Launch,
    )
}

fn builtin_task_cast_timeout(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    invoke_task_body_with_timeout(
        vm,
        &args[1],
        &args[0],
        "__task_cast_timeout",
        TaskMode::Cast,
    )
}

fn builtin_workers_submit(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let [Value::Workers(handle), Value::Callable(message)] = args.as_slice() else {
        return Err(RuntimeError::new(
            "__workers_submit expects Workers handle and callable template",
        ));
    };
    vm.workers_submit(handle, message.clone())
}

fn builtin_workers_submit_timeout(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let [timeout, Value::Workers(handle), Value::Callable(message)] = args.as_slice() else {
        return Err(RuntimeError::new(
            "__workers_submit_timeout expects Duration, Workers handle, and callable template",
        ));
    };
    let timeout_ms = duration_to_u64(vm, timeout, "__workers_submit_timeout", "timeout")?;
    vm.workers_submit_with_timeout(handle, message.clone(), timeout_ms)
}

fn builtin_workers_broadcast(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let [Value::Workers(handle), Value::Callable(message)] = args.as_slice() else {
        return Err(RuntimeError::new(
            "__workers_broadcast expects Workers handle and callable template",
        ));
    };
    vm.workers_broadcast(handle, message.clone())
}

fn builtin_workers_broadcast_timeout(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let [timeout, Value::Workers(handle), Value::Callable(message)] = args.as_slice() else {
        return Err(RuntimeError::new(
            "__workers_broadcast_timeout expects Duration, Workers handle, and callable template",
        ));
    };
    let timeout_ms = duration_to_u64(vm, timeout, "__workers_broadcast_timeout", "timeout")?;
    vm.workers_broadcast_with_timeout(handle, message.clone(), timeout_ms)
}

fn builtin_workers_reserve(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let [Value::Workers(handle)] = args.as_slice() else {
        return Err(RuntimeError::new(
            "__workers_reserve expects Workers handle",
        ));
    };
    vm.workers_reserve(handle)
}

fn builtin_workers_size(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let [Value::Workers(handle)] = args.as_slice() else {
        return Err(RuntimeError::new("__workers_size expects Workers handle"));
    };
    vm.workers_size(handle)
}

fn invoke_task_body(
    vm: &mut VM,
    value: &Value,
    name: &str,
    mode: TaskMode,
) -> Result<Value, RuntimeError> {
    let Value::Callable(body) = value.clone() else {
        return Err(RuntimeError::new(format!("{name} expects callable body")));
    };
    vm.invoke_task(body, mode)
}

fn invoke_task_body_with_timeout(
    vm: &mut VM,
    value: &Value,
    timeout: &Value,
    name: &str,
    mode: TaskMode,
) -> Result<Value, RuntimeError> {
    let Value::Callable(body) = value.clone() else {
        return Err(RuntimeError::new(format!("{name} expects callable body")));
    };
    let timeout_ms = duration_to_u64(vm, timeout, name, "timeout")?;
    vm.invoke_task_with_timeout(body, mode, Some(timeout_ms))
}

fn builtin_safe_div(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    match (&args[0], &args[1]) {
        (Value::Int(a), Value::Int(b)) => {
            if b.is_zero() {
                Ok(err_result(vm, "ZeroDivisionError", "division by zero"))
            } else {
                Ok(ok_result(Value::Int(a / b)))
            }
        }
        (Value::Float(a), Value::Float(b)) => {
            let (a, b) = expect_finite_float_pair(*a, *b, "safe_div")?;
            if b == 0.0 {
                Ok(err_result(vm, "ZeroDivisionError", "division by zero"))
            } else {
                Ok(ok_result(float_value(a / b, "safe_div")?))
            }
        }
        (left, right) => Err(RuntimeError::new(format!(
            "safe_div expects (Int, Int) or (Float, Float), got ({:?}, {:?})",
            left, right
        ))),
    }
}

fn builtin_safe_mod(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    match (&args[0], &args[1]) {
        (Value::Int(a), Value::Int(b)) => {
            if b.is_zero() {
                Ok(err_result(vm, "ZeroDivisionError", "division by zero"))
            } else {
                Ok(ok_result(Value::Int(a % b)))
            }
        }
        (left, right) => Err(RuntimeError::new(format!(
            "safe_mod expects (Int, Int), got ({:?}, {:?})",
            left, right
        ))),
    }
}

fn expect_int_pair(args: &[Value], name: &str) -> Result<(SurtrInt, SurtrInt), RuntimeError> {
    let (Value::Int(left), Value::Int(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new(format!("{} expects (Int, Int)", name)));
    };
    Ok((left.clone(), right.clone()))
}

fn expect_float_pair(args: &[Value], name: &str) -> Result<(f64, f64), RuntimeError> {
    let (Value::Float(left), Value::Float(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new(format!(
            "{} expects (Float, Float)",
            name
        )));
    };
    Ok((*left, *right))
}

fn expect_finite_float(value: f64, name: &str) -> Result<f64, RuntimeError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(RuntimeError::new(format!(
            "{name} expects finite Float values"
        )))
    }
}

fn expect_finite_float_pair(left: f64, right: f64, name: &str) -> Result<(f64, f64), RuntimeError> {
    Ok((
        expect_finite_float(left, name)?,
        expect_finite_float(right, name)?,
    ))
}

fn float_value(value: f64, name: &str) -> Result<Value, RuntimeError> {
    if value.is_finite() {
        Ok(Value::Float(value))
    } else {
        Err(RuntimeError::new(format!(
            "{name} produced non-finite value"
        )))
    }
}

fn builtin_operator_int_add(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_add")?;
    Ok(Value::Int(left + right))
}

fn builtin_operator_int_sub(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_sub")?;
    Ok(Value::Int(left - right))
}

fn builtin_operator_int_mul(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_mul")?;
    Ok(Value::Int(left * right))
}

fn builtin_operator_float_add(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_add")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_add")?;
    float_value(left + right, "__operator_float_add")
}

fn builtin_operator_float_sub(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_sub")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_sub")?;
    float_value(left - right, "__operator_float_sub")
}

fn builtin_operator_float_mul(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_mul")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_mul")?;
    float_value(left * right, "__operator_float_mul")
}

fn builtin_float_floor(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let value = expect_finite_float(
        args.first()
            .and_then(|arg| match arg {
                Value::Float(value) => Some(*value),
                _ => None,
            })
            .ok_or_else(|| RuntimeError::new("float_floor expects (Float)"))?,
        "float_floor",
    )?;
    float_value(value.floor(), "float_floor")
}

fn builtin_float_ceil(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let value = expect_finite_float(
        args.first()
            .and_then(|arg| match arg {
                Value::Float(value) => Some(*value),
                _ => None,
            })
            .ok_or_else(|| RuntimeError::new("float_ceil expects (Float)"))?,
        "float_ceil",
    )?;
    float_value(value.ceil(), "float_ceil")
}

fn builtin_float_round(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let value = expect_finite_float(
        args.first()
            .and_then(|arg| match arg {
                Value::Float(value) => Some(*value),
                _ => None,
            })
            .ok_or_else(|| RuntimeError::new("float_round expects (Float)"))?,
        "float_round",
    )?;
    float_value(value.round(), "float_round")
}

fn builtin_float_trunc(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let value = expect_finite_float(
        args.first()
            .and_then(|arg| match arg {
                Value::Float(value) => Some(*value),
                _ => None,
            })
            .ok_or_else(|| RuntimeError::new("float_trunc expects (Float)"))?,
        "float_trunc",
    )?;
    float_value(value.trunc(), "float_trunc")
}

fn builtin_float_pi(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    float_value(std::f64::consts::PI, "float_pi")
}

fn builtin_float_e(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    float_value(std::f64::consts::E, "float_e")
}

fn builtin_operator_int_eq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_eq")?;
    Ok(Value::Bool(left == right))
}

fn builtin_operator_int_neq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_neq")?;
    Ok(Value::Bool(left != right))
}

fn builtin_operator_int_lt(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_lt")?;
    Ok(Value::Bool(left < right))
}

fn builtin_operator_int_lte(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_lte")?;
    Ok(Value::Bool(left <= right))
}

fn builtin_operator_int_gt(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_gt")?;
    Ok(Value::Bool(left > right))
}

fn builtin_operator_int_gte(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__operator_int_gte")?;
    Ok(Value::Bool(left >= right))
}

fn builtin_operator_float_eq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_eq")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_eq")?;
    Ok(Value::Bool(left == right))
}

fn builtin_operator_float_neq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_neq")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_neq")?;
    Ok(Value::Bool(left != right))
}

fn builtin_operator_float_lt(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_lt")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_lt")?;
    Ok(Value::Bool(left < right))
}

fn builtin_operator_float_lte(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_lte")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_lte")?;
    Ok(Value::Bool(left <= right))
}

fn builtin_operator_float_gt(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_gt")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_gt")?;
    Ok(Value::Bool(left > right))
}

fn builtin_operator_float_gte(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_gte")?;
    let (left, right) = expect_finite_float_pair(left, right, "__operator_float_gte")?;
    Ok(Value::Bool(left >= right))
}

fn ordering_value(vm: &VM, variant: &str) -> Result<Value, RuntimeError> {
    let tag = find_variant_tag(vm, variant)?;
    Ok(Value::Tagged {
        tag,
        fields: Vec::new(),
    })
}

fn ordering_matches(vm: &VM, value: &Value, variants: &[&str]) -> Result<bool, RuntimeError> {
    let Value::Tagged { tag, fields } = value else {
        return Err(RuntimeError::new("ordering predicate expects Ordering"));
    };
    if !fields.is_empty() {
        return Err(RuntimeError::new("ordering predicate expects Ordering"));
    }
    for variant in variants {
        if *tag == find_variant_tag(vm, variant)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn builtin_compare_int(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_int_pair(&args, "__compare_int")?;
    if left < right {
        ordering_value(vm, "Ordering::Less")
    } else if left > right {
        ordering_value(vm, "Ordering::Greater")
    } else {
        ordering_value(vm, "Ordering::Equal")
    }
}

fn builtin_compare_float(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__compare_float")?;
    let (left, right) = expect_finite_float_pair(left, right, "__compare_float")?;
    if left < right {
        ordering_value(vm, "Ordering::Less")
    } else if left > right {
        ordering_value(vm, "Ordering::Greater")
    } else {
        ordering_value(vm, "Ordering::Equal")
    }
}

fn builtin_ordering_is_lt(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let value = args
        .first()
        .ok_or_else(|| RuntimeError::new("__ordering_is_lt expects Ordering"))?;
    Ok(Value::Bool(ordering_matches(
        vm,
        value,
        &["Ordering::Less"],
    )?))
}

fn builtin_ordering_is_lte(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let value = args
        .first()
        .ok_or_else(|| RuntimeError::new("__ordering_is_lte expects Ordering"))?;
    Ok(Value::Bool(ordering_matches(
        vm,
        value,
        &["Ordering::Less", "Ordering::Equal"],
    )?))
}

fn builtin_ordering_is_gt(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let value = args
        .first()
        .ok_or_else(|| RuntimeError::new("__ordering_is_gt expects Ordering"))?;
    Ok(Value::Bool(ordering_matches(
        vm,
        value,
        &["Ordering::Greater"],
    )?))
}

fn builtin_ordering_is_gte(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let value = args
        .first()
        .ok_or_else(|| RuntimeError::new("__ordering_is_gte expects Ordering"))?;
    Ok(Value::Bool(ordering_matches(
        vm,
        value,
        &["Ordering::Equal", "Ordering::Greater"],
    )?))
}

fn builtin_operator_string_eq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Str(left), Value::Str(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new(
            "__operator_string_eq expects (String, String)",
        ));
    };
    Ok(Value::Bool(left == right))
}

fn builtin_operator_string_neq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Str(left), Value::Str(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new(
            "__operator_string_neq expects (String, String)",
        ));
    };
    Ok(Value::Bool(left != right))
}

fn builtin_operator_boolean_eq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Bool(left), Value::Bool(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new(
            "__operator_boolean_eq expects (Boolean, Boolean)",
        ));
    };
    Ok(Value::Bool(left == right))
}

fn builtin_operator_boolean_neq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Bool(left), Value::Bool(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new(
            "__operator_boolean_neq expects (Boolean, Boolean)",
        ));
    };
    Ok(Value::Bool(left != right))
}

fn builtin_operator_string_concat(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Str(left), Value::Str(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new(
            "__operator_string_concat expects (String, String)",
        ));
    };
    Ok(Value::Str(format!("{}{}", left, right)))
}

fn builtin_eprint(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Error(rich) => {
            if !vm.is_stderr_captured() {
                if let Some((file, line, column)) = error_display_site(rich) {
                    vm.emit_stderr_line(format!("{}:{}:{}", file, line, column));
                }
            }
            for line in rich.to_eprint_lines() {
                vm.emit_stderr_line(line);
            }
        }
        other => {
            let s = inspect_value(vm, other);
            vm.emit_stderr_line(s);
        }
    }
    Ok(Value::Unit)
}

fn error_display_site(rich: &RichError) -> Option<(String, u32, u32)> {
    if rich.location.line == 0 || rich.location.column == 0 {
        return None;
    }
    if rich.location.file == "REPL" {
        return Some((rich.location.file.clone(), rich.location.line + 1, 1));
    }
    Some((
        rich.location.file.clone(),
        rich.location.line,
        rich.location.column,
    ))
}

fn builtin_set_exit_code(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Int(ref code) = args[0] else {
        return Err(RuntimeError::new("set_exit_code expects Int"));
    };
    let exit_code = code.to_i32().ok_or_else(|| {
        RuntimeError::new(format!("set_exit_code out of range for i32: {}", code))
    })?;
    vm.set_exit_code(exit_code);
    Ok(Value::Unit)
}

fn builtin_shl(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(bits)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("shl expects (Int, Int)"));
    };
    let Some(amount) = bits.to_usize() else {
        return Ok(err_result(
            _vm,
            "NegativeShiftCount",
            &format!("shift amount must be non-negative: {}", bits),
        ));
    };
    let shifted = value << amount;
    Ok(ok_result(Value::Int(shifted)))
}

fn builtin_shr(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(bits)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("shr expects (Int, Int)"));
    };
    let Some(amount) = bits.to_usize() else {
        return Ok(err_result(
            _vm,
            "NegativeShiftCount",
            &format!("shift amount must be non-negative: {}", bits),
        ));
    };
    let shifted = value >> amount;
    Ok(ok_result(Value::Int(shifted)))
}

fn builtin_list_len(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(list) = &args[0] else {
        return Err(RuntimeError::new("len expects List as first argument"));
    };
    Ok(Value::Int(list.len.into()))
}

fn builtin_string_len(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(value) = &args[0] else {
        return Err(RuntimeError::new(
            "string_len expects String as first argument",
        ));
    };
    Ok(Value::Int(value.chars().count().into()))
}

fn expect_string_pair(args: &[Value], name: &str) -> Result<(String, String), RuntimeError> {
    let (Value::Str(left), Value::Str(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new(format!(
            "{name} expects (String, String)"
        )));
    };
    Ok((left.clone(), right.clone()))
}

fn builtin_string_contains(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (value, needle) = expect_string_pair(&args, "string_contains")?;
    Ok(Value::Bool(value.contains(&needle)))
}

fn builtin_string_starts_with(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (value, prefix) = expect_string_pair(&args, "string_starts_with")?;
    Ok(Value::Bool(value.starts_with(&prefix)))
}

fn builtin_string_ends_with(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (value, suffix) = expect_string_pair(&args, "string_ends_with")?;
    Ok(Value::Bool(value.ends_with(&suffix)))
}

fn builtin_string_split(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (value, separator) = expect_string_pair(&args, "string_split")?;
    let items = if separator.is_empty() {
        value
            .chars()
            .map(|ch| Value::Str(ch.to_string()))
            .collect::<Vec<_>>()
    } else {
        value
            .split(&separator)
            .map(|part| Value::Str(part.to_string()))
            .collect::<Vec<_>>()
    };
    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_string_replace(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Str(value), Value::Str(from), Value::Str(to)) = (&args[0], &args[1], &args[2])
    else {
        return Err(RuntimeError::new(
            "string_replace expects (String, String, String)",
        ));
    };
    if from.is_empty() {
        Ok(Value::Str(value.clone()))
    } else {
        Ok(Value::Str(value.replace(from, to)))
    }
}

fn builtin_gen_make(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Int(idx) = &args[0] else {
        return Err(RuntimeError::new("gen_make expects Int as first argument"));
    };
    let Value::List(items) = &args[1] else {
        return Err(RuntimeError::new(
            "gen_make expects List as second argument",
        ));
    };

    Ok(Value::Tuple(vec![
        Value::Int(idx.clone()),
        Value::List(items.clone()),
    ]))
}

fn builtin_gen_idx(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (idx, _) = decode_generator_arg(&args[0], "gen_idx", "gen")?;
    Ok(Value::Int(idx.clone()))
}

fn builtin_gen_items(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (_, items) = decode_generator_arg(&args[0], "gen_items", "gen")?;
    Ok(Value::List(items.clone()))
}

fn builtin_bit_and(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(left), Value::Int(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("bit_and expects (Int, Int)"));
    };
    Ok(Value::Int(left & right))
}

fn builtin_bit_or(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(left), Value::Int(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("bit_or expects (Int, Int)"));
    };
    Ok(Value::Int(left | right))
}

fn builtin_bit_xor(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(left), Value::Int(right)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("bit_xor expects (Int, Int)"));
    };
    Ok(Value::Int(left ^ right))
}

fn builtin_bit_not(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Int(value) = &args[0] else {
        return Err(RuntimeError::new("bit_not expects Int"));
    };
    Ok(Value::Int(!value.clone()))
}

fn builtin_test_bit(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(index)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("test_bit expects (Int, Int)"));
    };
    let bit_index = match bit_index_to_usize(vm, index)? {
        Ok(bit_index) => bit_index,
        Err(err) => return Ok(err),
    };
    let mask = bit_mask(bit_index);
    Ok(ok_result(Value::Bool(!(value.clone() & mask).is_zero())))
}

fn builtin_set_bit(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(index)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("set_bit expects (Int, Int)"));
    };
    let bit_index = match bit_index_to_usize(vm, index)? {
        Ok(bit_index) => bit_index,
        Err(err) => return Ok(err),
    };
    let mask = bit_mask(bit_index);
    Ok(ok_result(Value::Int(value.clone() | mask)))
}

fn builtin_clear_bit(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(index)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("clear_bit expects (Int, Int)"));
    };
    let bit_index = match bit_index_to_usize(vm, index)? {
        Ok(bit_index) => bit_index,
        Err(err) => return Ok(err),
    };
    let mask = bit_mask(bit_index);
    Ok(ok_result(Value::Int(value.clone() & !mask)))
}

fn builtin_toggle_bit(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(index)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("toggle_bit expects (Int, Int)"));
    };
    let bit_index = match bit_index_to_usize(vm, index)? {
        Ok(bit_index) => bit_index,
        Err(err) => return Ok(err),
    };
    let mask = bit_mask(bit_index);
    Ok(ok_result(Value::Int(value.clone() ^ mask)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringEncodingMode {
    Utf8,
    Ascii,
}

fn builtin_codepoints(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(value) = &args[0] else {
        return Err(RuntimeError::new(
            "codepoints expects (String, StringEncoding)",
        ));
    };
    let encoding = decode_string_encoding(vm, &args[1])?;
    let items = match encoding {
        StringEncodingMode::Utf8 => value
            .as_bytes()
            .iter()
            .map(|byte| Value::Int(int(*byte)))
            .collect::<Vec<_>>(),
        StringEncodingMode::Ascii => {
            let mut out = Vec::with_capacity(value.len());
            for (idx, ch) in value.chars().enumerate() {
                if ch.is_ascii() {
                    out.push(Value::Int(int(ch as u32)));
                } else {
                    return Ok(err_result(
                        vm,
                        "InvalidStringEncoding",
                        &format!(
                            "ASCII encoding does not support character at index {}: {}",
                            idx, ch
                        ),
                    ));
                }
            }
            out
        }
    };
    Ok(ok_result(Value::List(ListHandle::from_items(items))))
}

fn builtin_from_codepoints(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(values) = &args[0] else {
        return Err(RuntimeError::new(
            "from_codepoints expects (List<Int>, StringEncoding)",
        ));
    };
    let encoding = decode_string_encoding(vm, &args[1])?;
    let mut bytes = Vec::with_capacity(values.len);
    for (idx, value) in values.iter().enumerate() {
        let Value::Int(code) = value else {
            return Err(RuntimeError::new("from_codepoints expects List<Int>"));
        };
        let Some(raw) = code.to_u32() else {
            return Ok(err_result(
                vm,
                "InvalidStringEncoding",
                &format!("negative code at index {}: {}", idx, code),
            ));
        };
        let max = match encoding {
            StringEncodingMode::Utf8 => 255,
            StringEncodingMode::Ascii => 127,
        };
        if raw > max {
            let label = match encoding {
                StringEncodingMode::Utf8 => "UTF-8 byte",
                StringEncodingMode::Ascii => "ASCII code",
            };
            return Ok(err_result(
                vm,
                "InvalidStringEncoding",
                &format!("{} out of range at index {}: {}", label, idx, raw),
            ));
        }
        bytes.push(raw as u8);
    }

    match String::from_utf8(bytes) {
        Ok(text) => Ok(ok_result(Value::Str(text))),
        Err(err) => {
            let utf8_err = err.utf8_error();
            let detail = match utf8_err.error_len() {
                Some(len) => format!(
                    "invalid UTF-8 byte sequence at index {} (len {})",
                    utf8_err.valid_up_to(),
                    len
                ),
                None => format!(
                    "incomplete UTF-8 byte sequence at index {}",
                    utf8_err.valid_up_to()
                ),
            };
            Ok(err_result(vm, "InvalidStringEncoding", &detail))
        }
    }
}

fn builtin_result_map_err(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let result = decode_result_arg(&args[0], "map_err", "result")?;
    let replacement = decode_error_arg(&args[1], "map_err", "err")?;

    Ok(match result {
        Ok(value) => ok_result(value),
        Err(_) => err_result_from_rich_error(replacement),
    })
}

fn builtin_result_cause(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let result = decode_result_arg(&args[0], "cause", "result")?;
    let mut domain_err = decode_error_arg(&args[1], "cause", "err")?;

    Ok(match result {
        Ok(value) => ok_result(value),
        Err(old) => {
            domain_err.append_cause_tail(old);
            err_result_from_rich_error(domain_err)
        }
    })
}

fn builtin_result_chain(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let head = decode_result_arg(&args[0], "chain", "head")?;
    let tail = decode_unit_result_arg(&args[1], "chain", "tail")?;

    Ok(match (head, tail) {
        (Ok(value), Ok(())) => ok_result(value),
        (Err(left), Ok(())) => err_result_from_rich_error(left),
        (Ok(_), Err(right)) => err_result_from_rich_error(right),
        (Err(left), Err(mut right)) => {
            right.append_cause_tail(left);
            err_result_from_rich_error(right)
        }
    })
}

fn builtin_result_recover_kind(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let result = decode_result_arg(&args[0], "__recover_kind", "value")?;
    let marker = decode_callable_arg(&args[1], "__recover_kind", "marker")?;
    let expected_kind = recover_kind_marker_name(vm, &marker)?;
    let handler = decode_callable_arg(&args[2], "__recover_kind", "handler")?;

    match result {
        Ok(value) => Ok(ok_result(value)),
        Err(err) if err.kind == expected_kind => {
            let handler_result = vm.invoke_callable_sync(handler, vec![err_value(err.clone())])?;
            match decode_result_arg(&handler_result, "__recover_kind", "handler result")? {
                Ok(value) => Ok(ok_result(value)),
                Err(rich) => Ok(err_result_from_rich_error(rich)),
            }
        }
        Err(err) => Ok(err_result_from_rich_error(err)),
    }
}

fn recover_kind_marker_name(vm: &VM, marker: &Callable) -> Result<String, RuntimeError> {
    let qualified_name = match &marker.target {
        sindr::runtime::CallableTarget::Function(fun_idx) => vm
            .function_entries()
            .get(*fun_idx as usize)
            .and_then(|entry| entry.qualified_name.as_deref())
            .ok_or_else(|| {
                RuntimeError::new(format!(
                    "__recover_kind marker references unknown function {}",
                    fun_idx
                ))
            })?,
        other => {
            return Err(RuntimeError::new(format!(
                "__recover_kind marker must be a deferror constructor function, got {:?}",
                other
            )))
        }
    };
    Ok(qualified_name.to_string())
}

fn builtin_test_push(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(kind) = &args[0] else {
        return Err(RuntimeError::new("__test_push expects String as kind"));
    };
    let Value::Str(name) = &args[1] else {
        return Err(RuntimeError::new("__test_push expects String as name"));
    };
    vm.push_test_scope(kind, name.clone());
    Ok(Value::Unit)
}

fn builtin_test_pop(vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    vm.pop_test_scope()?;
    Ok(Value::Unit)
}

fn builtin_test_pass(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(name) = &args[0] else {
        return Err(RuntimeError::new("__test_pass expects String as name"));
    };
    vm.record_test_pass(name.clone());
    Ok(Value::Unit)
}

fn builtin_test_fail(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(name) = &args[0] else {
        return Err(RuntimeError::new("__test_fail expects String as name"));
    };
    let Value::Str(detail) = &args[1] else {
        return Err(RuntimeError::new("__test_fail expects String as detail"));
    };
    vm.record_test_fail(name.clone(), detail.clone());
    Ok(Value::Unit)
}

fn builtin_test_fail_error(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(name) = &args[0] else {
        return Err(RuntimeError::new(
            "__test_fail_error expects String as name",
        ));
    };
    let Value::Error(error) = &args[1] else {
        return Err(RuntimeError::new(
            "__test_fail_error expects Error as error",
        ));
    };
    vm.record_test_fail_error(name.clone(), error);
    Ok(Value::Unit)
}

fn builtin_test_fail_current(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(detail) = &args[0] else {
        return Err(RuntimeError::new(
            "__test_fail_current expects String as detail",
        ));
    };
    vm.record_current_scope_fail(detail.clone());
    Ok(Value::Unit)
}

fn builtin_test_capture_stdout(vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    let items = vm
        .take_stdout()
        .into_iter()
        .map(Value::Str)
        .collect::<Vec<_>>();
    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_test_capture_stderr(vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    let items = vm
        .take_stderr()
        .into_iter()
        .map(Value::Str)
        .collect::<Vec<_>>();
    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_test_push_stdin(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let input = decode_string_arg(&args[0], "__test_push_stdin", "input")?;
    vm.push_stdin_input(input);
    Ok(Value::Unit)
}

fn builtin_test_begin_it(vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    vm.begin_test_case_io();
    Ok(Value::Unit)
}

fn builtin_list_group_count(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(values) = &args[0] else {
        return Err(RuntimeError::new("group_count expects List"));
    };

    let mut groups: Vec<(Value, usize)> = Vec::new();
    for value in values.iter() {
        if let Some((_, count)) = groups.iter_mut().find(|(existing, _)| *existing == value) {
            *count += 1;
        } else {
            groups.push((value, 1));
        }
    }

    let items = groups
        .into_iter()
        .map(|(value, count)| Value::Tuple(vec![value, Value::Int(int(count as u64))]))
        .collect::<Vec<_>>();

    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_list_zip(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(left) = &args[0] else {
        return Err(RuntimeError::new("zip expects List as first argument"));
    };
    let Value::List(right) = &args[1] else {
        return Err(RuntimeError::new("zip expects List as second argument"));
    };

    let items = left
        .iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| Value::Tuple(vec![left_value, right_value]))
        .collect::<Vec<_>>();

    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_empty_map(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Ok(Value::HashMap(HashMapHandle::empty()))
}

fn builtin_map_from_entries(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(entries) = &args[0] else {
        return Err(RuntimeError::new(
            "map_from_entries expects List<(String, V)>",
        ));
    };

    let mut map = HashMapHandle::empty();
    for (index, entry) in entries.iter().enumerate() {
        let Value::Tuple(items) = entry else {
            return Err(RuntimeError::new(format!(
                "map_from_entries expects tuple entries at index {}, got {:?}",
                index, entry
            )));
        };
        let [key, value] = items.as_slice() else {
            return Err(RuntimeError::new(format!(
                "map_from_entries expects (String, V) tuples at index {}, got arity {}",
                index,
                items.len()
            )));
        };
        let Value::Str(key) = key else {
            return Err(RuntimeError::new(format!(
                "map_from_entries expects String key at index {}, got {:?}",
                index, key
            )));
        };
        map = map.insert(key.clone(), value.clone());
    }

    Ok(Value::HashMap(map))
}

fn builtin_map_len(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "map_len", "map")?;
    Ok(Value::Int(int(map.len() as u64)))
}

fn builtin_map_contains_key(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "map_contains_key", "map")?;
    let key = decode_string_arg(&args[1], "map_contains_key", "key")?;
    Ok(Value::Bool(map.contains_key(key)))
}

fn builtin_map_get(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "map_get", "map")?;
    let key = decode_string_arg(&args[1], "map_get", "key")?;
    Ok(match map.get(key) {
        Some(value) => ok_result(value),
        None => none_result(vm),
    })
}

fn builtin_map_insert(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "map_insert", "map")?;
    let key = decode_string_arg(&args[1], "map_insert", "key")?;
    let value = args[2].clone();
    Ok(Value::HashMap(map.insert(key.to_string(), value)))
}

fn builtin_map_remove(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "map_remove", "map")?;
    let key = decode_string_arg(&args[1], "map_remove", "key")?;
    Ok(Value::HashMap(map.remove(key)))
}

fn builtin_map_keys(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "map_keys", "map")?;
    let items = map.keys().into_iter().map(Value::Str).collect::<Vec<_>>();
    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_map_values_list(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "map_values_list", "map")?;
    Ok(Value::List(ListHandle::from_items(map.values())))
}

fn facet_index_to_usize(
    vm: &VM,
    index: &SurtrInt,
    len: usize,
) -> Result<Result<usize, Value>, RuntimeError> {
    let value = if index.sign() == Sign::Minus {
        let abs = (-index).to_usize().ok_or_else(|| {
            RuntimeError::new("__facet_list index invariant broken for negative value")
        })?;
        if abs == 0 || abs > len {
            return Ok(Err(err_result(
                vm,
                "IndexOutOfBounds",
                &format!("index {index} out of bounds for len {len}"),
            )));
        }
        len - abs
    } else {
        let Some(value) = index.to_usize() else {
            return Ok(Err(err_result(
                vm,
                "IndexOutOfBounds",
                &format!("index {index} out of bounds for len {len}"),
            )));
        };
        value
    };
    if value >= len {
        return Ok(Err(err_result(
            vm,
            "IndexOutOfBounds",
            &format!("index {index} out of bounds for len {len}"),
        )));
    }
    Ok(Ok(value))
}

fn facet_range_to_bounds(
    vm: &VM,
    start: &SurtrInt,
    end: &SurtrInt,
    len: usize,
) -> Result<Result<(usize, usize), Value>, RuntimeError> {
    let start = match facet_index_to_usize(vm, start, len)? {
        Ok(value) => value,
        Err(err) => return Ok(Err(err)),
    };
    let end = match facet_index_to_usize(vm, end, len)? {
        Ok(value) => value,
        Err(err) => return Ok(Err(err)),
    };
    if start > end {
        return Ok(Err(err_result(
            vm,
            "IndexOutOfBounds",
            &format!("range start {start} exceeds end {end} for len {len}"),
        )));
    }
    Ok(Ok((start, end)))
}

fn key_not_found_result(vm: &VM, key: &str) -> Value {
    err_result(vm, "KeyNotFound", &format!("key not found: {key}"))
}

fn builtin_facet_list_get(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(list) = &args[0] else {
        return Err(RuntimeError::new("__facet_list_get expects List"));
    };
    let Value::Int(index) = &args[1] else {
        return Err(RuntimeError::new("__facet_list_get expects Int index"));
    };
    let index = match facet_index_to_usize(vm, index, list.len)? {
        Ok(index) => index,
        Err(err) => return Ok(err),
    };
    let value = list
        .iter()
        .nth(index)
        .ok_or_else(|| RuntimeError::new("__facet_list_get invariant broken after bounds check"))?;
    Ok(ok_result(value))
}

fn builtin_facet_list_set(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(list) = &args[0] else {
        return Err(RuntimeError::new("__facet_list_set expects List"));
    };
    let Value::Int(index) = &args[1] else {
        return Err(RuntimeError::new("__facet_list_set expects Int index"));
    };
    let index = match facet_index_to_usize(vm, index, list.len)? {
        Ok(index) => index,
        Err(err) => return Ok(err),
    };
    let mut items = list.iter().collect::<Vec<_>>();
    items[index] = args[2].clone();
    Ok(ok_result(Value::List(ListHandle::from_items(items))))
}

fn builtin_facet_list_slice_get(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(list) = &args[0] else {
        return Err(RuntimeError::new("__facet_list_slice_get expects List"));
    };
    let Value::Int(start) = &args[1] else {
        return Err(RuntimeError::new(
            "__facet_list_slice_get expects Int start",
        ));
    };
    let Value::Int(end) = &args[2] else {
        return Err(RuntimeError::new("__facet_list_slice_get expects Int end"));
    };
    let (start, end) = match facet_range_to_bounds(vm, start, end, list.len)? {
        Ok(bounds) => bounds,
        Err(err) => return Ok(err),
    };
    let items = list
        .iter()
        .skip(start)
        .take(end - start + 1)
        .collect::<Vec<_>>();
    Ok(ok_result(Value::List(ListHandle::from_items(items))))
}

fn builtin_facet_list_slice_set(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(list) = &args[0] else {
        return Err(RuntimeError::new("__facet_list_slice_set expects List"));
    };
    let Value::Int(start) = &args[1] else {
        return Err(RuntimeError::new(
            "__facet_list_slice_set expects Int start",
        ));
    };
    let Value::Int(end) = &args[2] else {
        return Err(RuntimeError::new("__facet_list_slice_set expects Int end"));
    };
    let Value::List(replacement) = &args[3] else {
        return Err(RuntimeError::new(
            "__facet_list_slice_set expects List replacement",
        ));
    };
    let (start, end) = match facet_range_to_bounds(vm, start, end, list.len)? {
        Ok(bounds) => bounds,
        Err(err) => return Ok(err),
    };
    let mut items = list.iter().collect::<Vec<_>>();
    items.splice(start..=end, replacement.iter());
    Ok(ok_result(Value::List(ListHandle::from_items(items))))
}

fn builtin_facet_map_get(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "__facet_map_get", "map")?;
    let key = decode_string_arg(&args[1], "__facet_map_get", "key")?;
    match map.get(key) {
        Some(value) => Ok(ok_result(value)),
        None => Ok(key_not_found_result(vm, key)),
    }
}

fn builtin_facet_map_set_existing(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let map = decode_hash_map_arg(&args[0], "__facet_map_set_existing", "map")?;
    let key = decode_string_arg(&args[1], "__facet_map_set_existing", "key")?;
    if !map.contains_key(key) {
        return Ok(key_not_found_result(vm, key));
    }
    Ok(ok_result(Value::HashMap(
        map.insert(key.to_string(), args[2].clone()),
    )))
}

fn builtin_facet_view(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::view should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_facet_preview(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::preview should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_facet_compose(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::chain should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_facet_replace(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::put should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_facet_set(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::set should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_facet_over(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::over should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_facet_over_result(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::over_result should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_facet_case_set(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::case_set should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_facet_case_over(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Facet::case_over should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_regex_compile(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(pattern) = &args[0] else {
        return Err(RuntimeError::new("compile expects String as pattern"));
    };

    match Regex::new(pattern) {
        Ok(_) => Ok(ok_result(Value::Regex(RegexHandle {
            pattern: pattern.clone(),
        }))),
        Err(err) => Ok(err_result(vm, "RegexCompileError", &err.to_string())),
    }
}

fn builtin_regex_is_match(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let pattern = decode_regex_arg(&args[0], "is_match", "re")?;
    let Value::Str(input) = &args[1] else {
        return Err(RuntimeError::new("is_match expects String as input"));
    };
    let re = compile_cached_regex(pattern, "is_match")?;
    Ok(Value::Bool(re.is_match(input)))
}

fn builtin_regex_captures(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let pattern = decode_regex_arg(&args[0], "captures", "re")?;
    let Value::Str(input) = &args[1] else {
        return Err(RuntimeError::new("captures expects String as input"));
    };
    let re = compile_cached_regex(pattern, "captures")?;
    let Some(captures) = re.captures(input) else {
        return Ok(none_result(vm));
    };

    let mut groups = Vec::with_capacity(captures.len());
    for idx in 0..captures.len() {
        groups.push(captures.get(idx).map(|m| (m.start(), m.end())));
    }

    let mut name_to_index = HashMap::new();
    for (idx, maybe_name) in re.capture_names().enumerate() {
        if let Some(name) = maybe_name {
            name_to_index.insert(name.to_string(), idx);
        }
    }

    Ok(ok_result(Value::RegexCaptures(RegexCapturesHandle {
        input: input.clone(),
        groups,
        name_to_index,
    })))
}

fn builtin_regex_whole(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let caps = decode_regex_captures_arg(&args[0], "whole", "caps")?;
    let Some((start, end)) = caps.groups.first().and_then(|item| *item) else {
        return Err(RuntimeError::new(
            "whole expects captures with group 0 present",
        ));
    };
    Ok(Value::Str(
        slice_with_span(&caps.input, start, end, "whole")?.to_string(),
    ))
}

fn builtin_regex_capture_count(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let caps = decode_regex_captures_arg(&args[0], "capture_count", "caps")?;
    Ok(Value::Int(int(caps.groups.len() as u64)))
}

fn builtin_regex_get(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let caps = decode_regex_captures_arg(&args[0], "get", "caps")?;
    let Value::Int(index) = &args[1] else {
        return Err(RuntimeError::new("get expects Int as idx"));
    };
    let Some(index) = index.to_usize() else {
        return Ok(none_result(vm));
    };
    let Some((start, end)) = caps.groups.get(index).and_then(|item| *item) else {
        return Ok(none_result(vm));
    };
    Ok(ok_result(Value::Str(
        slice_with_span(&caps.input, start, end, "get")?.to_string(),
    )))
}

fn builtin_regex_get_name(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let caps = decode_regex_captures_arg(&args[0], "get_name", "caps")?;
    let Value::Str(name) = &args[1] else {
        return Err(RuntimeError::new("get_name expects String as name"));
    };
    let Some(index) = caps.name_to_index.get(name) else {
        return Ok(none_result(vm));
    };
    let Some((start, end)) = caps.groups.get(*index).and_then(|item| *item) else {
        return Ok(none_result(vm));
    };
    Ok(ok_result(Value::Str(
        slice_with_span(&caps.input, start, end, "get_name")?.to_string(),
    )))
}

fn builtin_regex_find(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let pattern = decode_regex_arg(&args[0], "find", "re")?;
    let Value::Str(input) = &args[1] else {
        return Err(RuntimeError::new("find expects String as input"));
    };
    let re = compile_cached_regex(pattern, "find")?;
    let Some(matched) = re.find(input) else {
        return Ok(none_result(vm));
    };
    Ok(ok_result(Value::RegexMatch(RegexMatchHandle {
        input: input.clone(),
        start: matched.start(),
        end: matched.end(),
    })))
}

fn builtin_regex_find_all(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let pattern = decode_regex_arg(&args[0], "find_all", "re")?;
    let Value::Str(input) = &args[1] else {
        return Err(RuntimeError::new("find_all expects String as input"));
    };
    let re = compile_cached_regex(pattern, "find_all")?;
    let items = re
        .find_iter(input)
        .map(|matched| {
            Value::RegexMatch(RegexMatchHandle {
                input: input.clone(),
                start: matched.start(),
                end: matched.end(),
            })
        })
        .collect::<Vec<_>>();
    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_regex_split(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let pattern = decode_regex_arg(&args[0], "split", "re")?;
    let Value::Str(input) = &args[1] else {
        return Err(RuntimeError::new("split expects String as input"));
    };
    let re = compile_cached_regex(pattern, "split")?;
    let items = re
        .split(input)
        .map(|part| Value::Str(part.to_string()))
        .collect::<Vec<_>>();
    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_regex_replace(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let pattern = decode_regex_arg(&args[0], "replace", "re")?;
    let Value::Str(input) = &args[1] else {
        return Err(RuntimeError::new("replace expects String as input"));
    };
    let Value::Str(replacement) = &args[2] else {
        return Err(RuntimeError::new("replace expects String as replacement"));
    };
    let re = compile_cached_regex(pattern, "replace")?;
    Ok(Value::Str(
        re.replace(input, replacement.as_str()).into_owned(),
    ))
}

fn builtin_regex_replace_all(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let pattern = decode_regex_arg(&args[0], "replace_all", "re")?;
    let Value::Str(input) = &args[1] else {
        return Err(RuntimeError::new("replace_all expects String as input"));
    };
    let Value::Str(replacement) = &args[2] else {
        return Err(RuntimeError::new(
            "replace_all expects String as replacement",
        ));
    };
    let re = compile_cached_regex(pattern, "replace_all")?;
    Ok(Value::Str(
        re.replace_all(input, replacement.as_str()).into_owned(),
    ))
}

fn builtin_regex_escape(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Str(text) = &args[0] else {
        return Err(RuntimeError::new("escape expects String as text"));
    };
    Ok(Value::Str(regex::escape(text)))
}

fn builtin_regex_group_names(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let pattern = decode_regex_arg(&args[0], "group_names", "re")?;
    let re = compile_cached_regex(pattern, "group_names")?;
    let items = re
        .capture_names()
        .flatten()
        .map(|name| Value::Str(name.to_string()))
        .collect::<Vec<_>>();
    Ok(Value::List(ListHandle::from_items(items)))
}

fn builtin_regex_match_text(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let matched = decode_regex_match_arg(&args[0], "text", "m")?;
    Ok(Value::Str(
        slice_with_span(&matched.input, matched.start, matched.end, "text")?.to_string(),
    ))
}

fn builtin_regex_match_start(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let matched = decode_regex_match_arg(&args[0], "start", "m")?;
    Ok(Value::Int(int(matched.start as u64)))
}

fn builtin_regex_match_end(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let matched = decode_regex_match_arg(&args[0], "end", "m")?;
    Ok(Value::Int(int(matched.end as u64)))
}

fn builtin_project_args(vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    let args = vm
        .cli_args()
        .iter()
        .cloned()
        .map(Value::Str)
        .collect::<Vec<_>>();
    Ok(Value::List(ListHandle::from_items(args)))
}

fn builtin_io_get(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let prompt = decode_string_arg(&args[0], "io_get", "prompt")?;
    if let Err(message) = emit_io_prompt(vm, prompt) {
        return Ok(input_error(vm, &message));
    }

    if vm.has_injected_stdin() {
        return Ok(match vm.read_injected_char() {
            Some(ch) => ok_result(Value::Str(ch)),
            None => input_error(vm, "end of input"),
        });
    }

    let read = if io::stdin().is_terminal() {
        read_terminal_char()
    } else {
        read_stdin_char()
    };
    Ok(match read {
        Ok(Some(ch)) => ok_result(Value::Str(ch)),
        Ok(None) => input_error(vm, "end of input"),
        Err(message) => input_error(vm, &message),
    })
}

fn builtin_io_get_line(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let prompt = decode_string_arg(&args[0], "io_get_line", "prompt")?;
    if let Err(message) = emit_io_prompt(vm, prompt) {
        return Ok(input_error(vm, &message));
    }

    let read = if vm.has_injected_stdin() {
        Ok(vm.read_injected_line())
    } else {
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map(|count| (count > 0).then_some(line))
            .map_err(|err| err.to_string())
    };
    Ok(match read {
        Ok(Some(line)) => ok_result(Value::Str(strip_line_ending(line))),
        Ok(None) => input_error(vm, "end of input"),
        Err(message) => input_error(vm, &message),
    })
}

fn builtin_file_read(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_string_arg(&args[0], "file_read", "path")?;
    let host_path = vm.resolve_host_path(path);
    match fs::read_to_string(&host_path) {
        Ok(text) => Ok(ok_result(Value::Str(text))),
        Err(err) => Ok(file_path_error_result(vm, path, err)),
    }
}

fn builtin_file_write(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_string_arg(&args[0], "file_write", "path")?;
    let text = decode_string_arg(&args[1], "file_write", "text")?;
    let host_path = vm.resolve_host_path(path);
    match fs::write(&host_path, text) {
        Ok(()) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(file_path_error_result(vm, path, err)),
    }
}

fn builtin_file_append(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_string_arg(&args[0], "file_append", "path")?;
    let text = decode_string_arg(&args[1], "file_append", "text")?;
    let handle = match vm.open_file_resource(path, VmFileMode::Append) {
        Ok(handle) => handle,
        Err(err) => return Ok(file_handle_error_result(vm, Some(path), err)),
    };
    let write_result = vm.write_file_chunk(handle.id, text);
    let close_result = vm.close_file_resource(handle.id);
    match (write_result, close_result) {
        (Ok(()), Ok(())) => Ok(ok_result(Value::Unit)),
        (Err(err), _) => Ok(file_handle_error_result(vm, Some(path), err)),
        (Ok(()), Err(err)) => Ok(file_handle_error_result(vm, Some(path), err)),
    }
}

fn builtin_file_exists(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_string_arg(&args[0], "file_exists", "path")?;
    Ok(Value::Bool(_vm.resolve_host_path(path).exists()))
}

fn builtin_file_delete(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_string_arg(&args[0], "file_delete", "path")?;
    let host_path = vm.resolve_host_path(path);
    match fs::remove_file(&host_path) {
        Ok(()) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(file_path_error_result(vm, path, err)),
    }
}

fn builtin_file_with_open(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_string_arg(&args[0], "file_with_open", "path")?;
    let mode = decode_file_mode(vm, &args[1], "file_with_open", "mode")?;
    let body = decode_callable_arg(&args[2], "file_with_open", "body")?;
    let handle = match vm.open_file_resource(path, mode) {
        Ok(handle) => handle,
        Err(err) => return Ok(file_handle_error_result(vm, Some(path), err)),
    };

    let call_result = vm.invoke_callable_sync(body, vec![Value::FileHandle(handle.clone())]);
    let flush_result = vm.flush_file_resource(handle.id);
    let close_result = vm.close_file_resource(handle.id);

    if let Err(err) = flush_result {
        return Ok(file_handle_error_result(vm, Some(path), err));
    }
    if let Err(err) = close_result {
        return Ok(file_handle_error_result(vm, Some(path), err));
    }

    match call_result {
        Ok(value) => Ok(value),
        Err(err) => Err(err),
    }
}

fn builtin_file_read_chunk(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let handle = decode_file_handle_arg(&args[0], "file_read_chunk", "file")?;
    let max_chars = decode_non_negative_int_arg(&args[1], "file_read_chunk", "max_chars")?;
    match vm.read_file_chunk(handle.id, max_chars) {
        Ok(text) => Ok(ok_result(Value::Str(text))),
        Err(err) => Ok(file_handle_error_result(vm, None, err)),
    }
}

fn builtin_file_write_chunk(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let handle = decode_file_handle_arg(&args[0], "file_write_chunk", "file")?;
    let text = decode_string_arg(&args[1], "file_write_chunk", "text")?;
    match vm.write_file_chunk(handle.id, text) {
        Ok(()) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(file_handle_error_result(vm, None, err)),
    }
}

fn builtin_file_flush(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let handle = decode_file_handle_arg(&args[0], "file_flush", "file")?;
    match vm.flush_file_resource(handle.id) {
        Ok(()) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(file_handle_error_result(vm, None, err)),
    }
}

fn builtin_filesystem_path(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let raw = decode_string_arg(&args[0], "filesystem_path", "raw")?;
    Ok(ok_result(filesystem_file_path(vm, raw)?))
}

fn builtin_filesystem_join(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let base = decode_file_path_arg(vm, &args[0], "filesystem_join", "base")?;
    let child = decode_string_arg(&args[1], "filesystem_join", "child")?;
    let joined = Path::new(base).join(child).to_string_lossy().into_owned();
    Ok(ok_result(filesystem_file_path(vm, &joined)?))
}

fn builtin_filesystem_parent(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_parent", "path")?;
    let Some(parent) = Path::new(path).parent() else {
        return Ok(filesystem_error(vm, "FileSystemInvalidPath", path));
    };
    Ok(ok_result(filesystem_file_path(
        vm,
        &parent.to_string_lossy(),
    )?))
}

fn builtin_filesystem_name(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_name", "path")?;
    let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return Ok(filesystem_error(vm, "FileSystemInvalidPath", path));
    };
    Ok(ok_result(Value::Str(name.to_string())))
}

fn builtin_filesystem_extension(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_extension", "path")?;
    Ok(
        match Path::new(path).extension().and_then(|ext| ext.to_str()) {
            Some(ext) => option_some(vm, Value::Str(ext.to_string()))?,
            None => option_none(vm)?,
        },
    )
}

fn builtin_filesystem_exists(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_exists", "path")?;
    Ok(ok_result(Value::Bool(vm.resolve_host_path(path).exists())))
}

fn builtin_filesystem_stat(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_stat", "path")?;
    match filesystem_entry(vm, path)? {
        Ok(entry) => Ok(ok_result(entry)),
        Err(err) => Ok(err),
    }
}

fn builtin_filesystem_ls(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_ls", "path")?;
    filesystem_snapshot(vm, path, Some(1))
}

fn builtin_filesystem_tree_depth(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_tree_depth", "path")?;
    let depth = decode_int_i64_arg(&args[1], "filesystem_tree_depth", "depth")?;
    if depth < 0 {
        return Ok(filesystem_error_with_message(
            vm,
            "FileSystemInvalidDepth",
            &format!("invalid filesystem tree depth: {depth}"),
        ));
    }
    filesystem_snapshot(vm, path, Some(depth as usize))
}

fn builtin_filesystem_mkdir(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_mkdir", "path")?;
    let host_path = vm.resolve_host_path(path);
    match fs::create_dir(&host_path) {
        Ok(()) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(filesystem_io_error(vm, path, err)),
    }
}

fn builtin_filesystem_mkdir_all(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_mkdir_all", "path")?;
    let host_path = vm.resolve_host_path(path);
    match fs::create_dir_all(&host_path) {
        Ok(()) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(filesystem_io_error(vm, path, err)),
    }
}

fn builtin_filesystem_rm(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "filesystem_rm", "path")?;
    let host_path = vm.resolve_host_path(path);
    let result = match fs::symlink_metadata(&host_path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir(&host_path),
        Ok(_) => fs::remove_file(&host_path),
        Err(err) => Err(err),
    };
    match result {
        Ok(()) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(filesystem_io_error(vm, path, err)),
    }
}

fn builtin_filesystem_mv(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let from = decode_file_path_arg(vm, &args[0], "filesystem_mv", "from")?;
    let to = decode_file_path_arg(vm, &args[1], "filesystem_mv", "to")?;
    match fs::rename(vm.resolve_host_path(from), vm.resolve_host_path(to)) {
        Ok(()) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(filesystem_io_error(vm, from, err)),
    }
}

fn builtin_filesystem_cp(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let from = decode_file_path_arg(vm, &args[0], "filesystem_cp", "from")?;
    let to = decode_file_path_arg(vm, &args[1], "filesystem_cp", "to")?;
    let from_host = vm.resolve_host_path(from);
    if from_host.is_dir() {
        return Ok(filesystem_error_with_message(
            vm,
            "FileSystemUnsupported",
            "directory copy is not supported",
        ));
    }
    match fs::copy(from_host, vm.resolve_host_path(to)) {
        Ok(_) => Ok(ok_result(Value::Unit)),
        Err(err) => Ok(filesystem_io_error(vm, from, err)),
    }
}

fn builtin_shell_pwd(vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    let raw = vm.cwd().to_string_lossy().into_owned();
    Ok(ok_result(filesystem_file_path(vm, &raw)?))
}

fn builtin_shell_cd(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let path = decode_file_path_arg(vm, &args[0], "shell_cd", "path")?;
    let host_path = vm.resolve_host_path(path);
    if !host_path.is_dir() {
        return Ok(shell_error_with_message(
            vm,
            "ShellWorkingDirectoryNotFound",
            &format!("shell working directory not found: {path}"),
        ));
    }
    let cwd = match canonicalize_shell_cwd(vm, &host_path, path) {
        Ok(cwd) => cwd,
        Err(err) => return Ok(err),
    };
    vm.set_cwd(cwd);
    Ok(ok_result(Value::Unit))
}

fn builtin_shell_exec(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let command = decode_string_arg(&args[0], "shell_exec", "command")?;
    let argv = decode_string_list_arg(&args[1], "shell_exec", "args")?;
    let output = match Command::new(command)
        .args(&argv)
        .current_dir(vm.cwd())
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(shell_error_with_message(
                vm,
                "ShellCommandNotFound",
                &format!("shell command not found: {command}"),
            ));
        }
        Err(err) => {
            return Ok(shell_error_with_message(
                vm,
                "ShellSpawnFailed",
                &format!("shell spawn failed for {command}: {err}"),
            ));
        }
    };
    let stdout = match String::from_utf8(output.stdout) {
        Ok(text) => text,
        Err(err) => {
            return Ok(shell_error_with_message(
                vm,
                "ShellIoError",
                &format!("shell stdout is not valid UTF-8: {err}"),
            ));
        }
    };
    let stderr = match String::from_utf8(output.stderr) {
        Ok(text) => text,
        Err(err) => {
            return Ok(shell_error_with_message(
                vm,
                "ShellIoError",
                &format!("shell stderr is not valid UTF-8: {err}"),
            ));
        }
    };
    let result = tagged_by_name(
        vm,
        "CommandResult",
        vec![
            Value::Str(command.to_string()),
            Value::List(ListHandle::from_items(
                argv.into_iter().map(Value::Str).collect(),
            )),
            Value::Int(int(output.status.code().unwrap_or(-1))),
            Value::Str(stdout),
            Value::Str(stderr),
        ],
    )?;
    Ok(ok_result(result))
}

fn builtin_random_seed(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Int(seed) = &args[0] else {
        return Err(RuntimeError::new("seed expects Int as seed"));
    };
    Ok(Value::RandomGenerator(RandomGeneratorHandle {
        state: seed_to_state(seed),
    }))
}

fn builtin_random_int_until(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::Int(end) = &args[0] else {
        return Err(RuntimeError::new("int_until expects Int as end"));
    };
    random_int_range_result(vm, &int(0), end, host_random_generator())
        .map(|(value, _)| value.map(ok_result).unwrap_or_else(|err| err))
}

fn builtin_random_int_range(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(start), Value::Int(end)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("int_range expects Int as start/end"));
    };
    random_int_range_result(vm, start, end, host_random_generator())
        .map(|(value, _)| value.map(ok_result).unwrap_or_else(|err| err))
}

fn builtin_random_next_int_until(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let rng = decode_random_generator_arg(&args[0], "next_int_until", "rng")?;
    let Value::Int(end) = &args[1] else {
        return Err(RuntimeError::new("next_int_until expects Int as end"));
    };
    seeded_random_int_range_result(vm, rng, &int(0), end)
}

fn builtin_random_next_int_range(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let rng = decode_random_generator_arg(&args[0], "next_int_range", "rng")?;
    let (Value::Int(start), Value::Int(end)) = (&args[1], &args[2]) else {
        return Err(RuntimeError::new("next_int_range expects Int as start/end"));
    };
    seeded_random_int_range_result(vm, rng, start, end)
}

fn emit_io_prompt(vm: &mut VM, prompt: &str) -> Result<(), String> {
    vm.emit_stdout_text(prompt.to_string())
        .map_err(|err| format!("prompt write failed: {}", err))
}

fn strip_line_ending(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

fn read_terminal_char() -> Result<Option<String>, String> {
    crossterm::terminal::enable_raw_mode().map_err(|err| err.to_string())?;
    let result = (|| loop {
        match crossterm::event::read().map_err(|err| err.to_string())? {
            crossterm::event::Event::Key(event) => break key_event_to_string(event),
            _ => continue,
        }
    })();
    let disable_result = crossterm::terminal::disable_raw_mode().map_err(|err| err.to_string());
    match (result, disable_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), _) => Err(err),
        (_, Err(err)) => Err(err),
    }
}

fn key_event_to_string(event: crossterm::event::KeyEvent) -> Result<Option<String>, String> {
    use crossterm::event::KeyCode;

    let text = match event.code {
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::Enter => "\n".to_string(),
        KeyCode::Tab => "\t".to_string(),
        KeyCode::Backspace => "\u{8}".to_string(),
        KeyCode::Esc => "\u{1b}".to_string(),
        other => return Err(format!("unsupported key input: {:?}", other)),
    };
    Ok(Some(text))
}

fn read_stdin_char() -> Result<Option<String>, String> {
    let mut stdin = io::stdin().lock();
    let mut buf = [0u8; 4];
    let mut len = 0usize;
    loop {
        let read = stdin
            .read(&mut buf[len..len + 1])
            .map_err(|err| err.to_string())?;
        if read == 0 {
            return if len == 0 {
                Ok(None)
            } else {
                Err("incomplete UTF-8 input before EOF".into())
            };
        }
        len += read;
        match std::str::from_utf8(&buf[..len]) {
            Ok(text) => return Ok(text.chars().next().map(|ch| ch.to_string())),
            Err(err) if err.error_len().is_none() && len < buf.len() => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
}

fn input_error(vm: &VM, detail: &str) -> Value {
    err_result(vm, "InputError", detail)
}

pub fn inspect_value(vm: &VM, value: &Value) -> String {
    if let Value::Callable(callable) = value {
        if let Some(display) = inspect_callable(vm, callable) {
            return display;
        }
    }

    inspect_non_callable_value(vm, value)
}

fn inspect_non_callable_value(vm: &VM, value: &Value) -> String {
    match value {
        Value::Str(text) => quote_surtr_string_literal(text),
        Value::List(handle) => {
            let inner = handle
                .iter()
                .map(|item| inspect_non_callable_value(vm, &item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        Value::HashMap(handle) => {
            if handle.entries.is_empty() {
                return "hash![]".to_string();
            }

            let inner = handle
                .sorted_entries()
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{} => {}",
                        quote_surtr_string_literal(&key),
                        inspect_non_callable_value(vm, &value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("hash![{inner}]")
        }
        Value::Tuple(items) => {
            let inner = items
                .iter()
                .map(|item| inspect_non_callable_value(vm, item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({inner})")
        }
        Value::Tagged { tag, fields } => inspect_tagged_value(vm, *tag, fields),
        _ => value.to_display_string(vm.type_registry()),
    }
}

fn inspect_tagged_value(vm: &VM, tag: u32, fields: &[Value]) -> String {
    if let Some(entry) = vm.type_registry().lookup(tag) {
        if is_duration_type_name(&entry.name) {
            if let Some(Value::Int(ms)) = fields.first() {
                return format!("{ms}ms");
            }
        }
        let render_named_value = || {
            let display_name = surface_path_name(&entry.name);
            let hidden_field_count = entry.private_flags.iter().filter(|flag| **flag).count();
            let mut parts = entry
                .field_names
                .iter()
                .zip(
                    entry
                        .private_flags
                        .iter()
                        .copied()
                        .chain(std::iter::repeat(false)),
                )
                .zip(fields.iter())
                .filter_map(|((name, is_private), val)| {
                    (!is_private)
                        .then(|| format!("{name}: {}", inspect_non_callable_value(vm, val)))
                })
                .collect::<Vec<_>>();
            if hidden_field_count > 0 {
                parts.push("..private".to_string());
            }
            format!("{}({})", display_name, parts.join(", "))
        };

        return match entry.kind {
            sindr::runtime::TypeKind::Struct | sindr::runtime::TypeKind::Record => {
                render_named_value()
            }
            sindr::runtime::TypeKind::EnumVariant => {
                let payload = fields
                    .iter()
                    .skip(1)
                    .map(|val| inspect_non_callable_value(vm, val))
                    .collect::<Vec<_>>()
                    .join(", ");
                if payload.is_empty() {
                    surface_path_name(&entry.name).to_string()
                } else {
                    format!("{}({payload})", surface_path_name(&entry.name))
                }
            }
        };
    }

    match tag {
        0 => format!(
            "Ok({})",
            fields
                .first()
                .map(|v| inspect_non_callable_value(vm, v))
                .unwrap_or_default()
        ),
        1 => format!(
            "{}",
            fields
                .first()
                .map(|v| match v {
                    Value::Error(rich) => rich.to_result_display_string(),
                    _ => format!("Err({})", inspect_non_callable_value(vm, v)),
                })
                .unwrap_or_default()
        ),
        _ => format!("Tagged({}, {:?})", tag, fields),
    }
}

fn inspect_callable(_vm: &VM, callable: &Callable) -> Option<String> {
    let sig = callable_display_signature(callable)?;
    match callable_display_origin(callable)? {
        CallableDisplayOrigin::Capture { module, name } => Some(format!(
            "FnCapture(module: {}, name: {}, sig: {})",
            module, name, sig
        )),
        CallableDisplayOrigin::Closure => Some(format!("Closure{sig}")),
    }
}

enum CallableDisplayOrigin<'a> {
    Capture { module: &'a str, name: &'a str },
    Closure,
}

fn callable_display_signature(callable: &Callable) -> Option<String> {
    let full_signature = callable.metadata.full_signature.as_deref()?;
    if callable.metadata.applied_args == 0 {
        Some(full_signature.to_string())
    } else {
        remaining_callable_signature(full_signature, callable.metadata.applied_args)
    }
}

fn callable_display_origin(callable: &Callable) -> Option<CallableDisplayOrigin<'_>> {
    match callable.metadata.origin {
        sindr::runtime::CallableOrigin::Capture => {
            if let (Some(module), Some(name)) = (
                callable.metadata.module.as_deref(),
                callable.metadata.name.as_deref(),
            ) {
                Some(CallableDisplayOrigin::Capture { module, name })
            } else {
                callable
                    .lexical_captures
                    .first()
                    .and_then(callable_capture_origin_from_value)
            }
        }
        sindr::runtime::CallableOrigin::Closure => callable
            .lexical_captures
            .first()
            .and_then(callable_capture_origin_from_value)
            .or(Some(CallableDisplayOrigin::Closure)),
        sindr::runtime::CallableOrigin::Unknown => callable
            .lexical_captures
            .first()
            .and_then(callable_capture_origin_from_value),
    }
}

fn callable_capture_origin_from_value(value: &Value) -> Option<CallableDisplayOrigin<'_>> {
    let Value::Callable(callable) = value else {
        return None;
    };
    callable_display_origin(callable)
}

fn remaining_callable_signature(signature: &str, applied_args: usize) -> Option<String> {
    let (param_types, return_ty) = callable_signature_parts(signature)?;
    if applied_args > param_types.len() {
        return None;
    }
    let remaining = &param_types[applied_args..];
    if remaining.is_empty() {
        Some(format!("(-> {return_ty})"))
    } else {
        Some(format!("({} -> {return_ty})", remaining.join(", ")))
    }
}

fn callable_signature_parts(signature: &str) -> Option<(Vec<String>, String)> {
    let arrow_idx = find_top_level_arrow(signature)?;
    let return_ty = signature[arrow_idx + 2..].trim().to_string();
    let head = signature[..arrow_idx].trim();
    let open_idx = head.find('(')?;
    let close_idx = find_matching_paren(head, open_idx)?;
    let params_str = head[open_idx + 1..close_idx].trim();
    let param_types = split_top_level_commas(params_str)
        .into_iter()
        .map(|param| {
            param
                .rsplit_once(':')
                .map(|(_, ty)| ty.trim().to_string())
                .unwrap_or_else(|| param.trim().to_string())
        })
        .filter(|param| !param.is_empty())
        .collect::<Vec<_>>();
    Some((param_types, return_ty))
}

fn find_matching_paren(input: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in input.char_indices().skip(open_idx) {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_top_level_arrow(input: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let chars = input.char_indices().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx + 1 < chars.len() {
        let (byte_idx, ch) = chars[idx];
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.checked_sub(1)?,
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.checked_sub(1)?,
            '-' if chars[idx + 1].1 == '>' && paren_depth == 0 && angle_depth == 0 => {
                return Some(byte_idx);
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ',' if paren_depth == 0 && angle_depth == 0 => {
                let part = input[start..idx].trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = input[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

fn decode_regex_arg<'a>(
    value: &'a Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<&'a str, RuntimeError> {
    match value {
        Value::Regex(handle) => Ok(handle.pattern.as_str()),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects Regex as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_regex_captures_arg<'a>(
    value: &'a Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<&'a RegexCapturesHandle, RuntimeError> {
    match value {
        Value::RegexCaptures(handle) => Ok(handle),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects RegexCaptures as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_regex_match_arg<'a>(
    value: &'a Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<&'a RegexMatchHandle, RuntimeError> {
    match value {
        Value::RegexMatch(handle) => Ok(handle),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects RegexMatch as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_hash_map_arg<'a>(
    value: &'a Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<&'a HashMapHandle, RuntimeError> {
    match value {
        Value::HashMap(handle) => Ok(handle),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects HashMap as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_generator_arg<'a>(
    value: &'a Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<(&'a SurtrInt, &'a ListHandle), RuntimeError> {
    let Value::Tuple(items) = value else {
        return Err(RuntimeError::new(format!(
            "{builtin_name} expects Generator as {arg_name}, got {:?}",
            value
        )));
    };

    let [idx, tail] = items.as_slice() else {
        return Err(RuntimeError::new(format!(
            "{builtin_name} expects Generator tuple payload as {arg_name}, got arity {}",
            items.len()
        )));
    };

    let Value::Int(idx) = idx else {
        return Err(RuntimeError::new(format!(
            "{builtin_name} expects Generator idx as Int for {arg_name}, got {:?}",
            idx
        )));
    };
    let Value::List(tail) = tail else {
        return Err(RuntimeError::new(format!(
            "{builtin_name} expects Generator items as List for {arg_name}, got {:?}",
            tail
        )));
    };

    Ok((idx, tail))
}

fn decode_random_generator_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<RandomGeneratorHandle, RuntimeError> {
    match value {
        Value::RandomGenerator(handle) => Ok(*handle),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects RandomGenerator as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn seeded_random_int_range_result(
    vm: &VM,
    rng: RandomGeneratorHandle,
    start: &SurtrInt,
    end: &SurtrInt,
) -> Result<Value, RuntimeError> {
    let (sample, next_rng) = random_int_range_result(vm, start, end, rng)?;
    Ok(match sample {
        Ok(value) => ok_result(Value::Tuple(vec![value, Value::RandomGenerator(next_rng)])),
        Err(err) => err,
    })
}

fn random_int_range_result(
    vm: &VM,
    start: &SurtrInt,
    end: &SurtrInt,
    rng: RandomGeneratorHandle,
) -> Result<(Result<Value, Value>, RandomGeneratorHandle), RuntimeError> {
    let range = end - start;
    if range <= int(0) {
        return Ok((Err(invalid_random_range(vm, start, end)), rng));
    }

    let upper = range.to_biguint().ok_or_else(|| {
        RuntimeError::new(format!(
            "random range should be positive after validation, got {}",
            range
        ))
    })?;
    let (offset, next_rng) = sample_biguint_below(&upper, rng);
    let value = start + BigInt::from_biguint(Sign::Plus, offset);
    Ok((Ok(Value::Int(value)), next_rng))
}

fn invalid_random_range(vm: &VM, start: &SurtrInt, end: &SurtrInt) -> Value {
    err_result(
        vm,
        "InvalidRandomRange",
        &format!("random range must be non-empty: {}..{}", start, end),
    )
}

fn sample_biguint_below(
    upper: &BigUint,
    mut rng: RandomGeneratorHandle,
) -> (BigUint, RandomGeneratorHandle) {
    debug_assert!(!upper.is_zero());
    let bit_len = upper.bits();
    let byte_len = ((bit_len + 7) / 8) as usize;

    loop {
        let mut bytes = vec![0_u8; byte_len];
        for chunk in bytes.chunks_mut(8) {
            let (raw, next_rng) = random_next_u64(rng);
            rng = next_rng;
            let raw_bytes = raw.to_le_bytes();
            chunk.copy_from_slice(&raw_bytes[..chunk.len()]);
        }

        let excess_bits = (8 - (bit_len % 8)) % 8;
        if excess_bits != 0 {
            let mask = 0xff_u8 >> excess_bits;
            if let Some(last) = bytes.last_mut() {
                *last &= mask;
            }
        }

        let candidate = BigUint::from_bytes_le(&bytes);
        if &candidate < upper {
            return (candidate, rng);
        }
    }
}

fn host_random_generator() -> RandomGeneratorHandle {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| {
            duration.as_secs()
                ^ u64::from(duration.subsec_nanos()).rotate_left(32)
                ^ (duration.as_nanos() as u64)
        })
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = u64::from(std::process::id());

    RandomGeneratorHandle {
        state: mix64(now ^ count.rotate_left(17) ^ pid.rotate_left(41)),
    }
}

fn seed_to_state(seed: &SurtrInt) -> u64 {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for byte in seed.to_signed_bytes_le() {
        state = mix64(state ^ u64::from(byte));
    }
    state
}

fn random_next_u64(rng: RandomGeneratorHandle) -> (u64, RandomGeneratorHandle) {
    let next_state = rng.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    (
        mix64(next_state),
        RandomGeneratorHandle { state: next_state },
    )
}

fn mix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn decode_string_arg<'a>(
    value: &'a Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<&'a str, RuntimeError> {
    match value {
        Value::Str(text) => Ok(text),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects String as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_string_list_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<Vec<String>, RuntimeError> {
    let Value::List(list) = value else {
        return Err(RuntimeError::new(format!(
            "{builtin_name} expects List<String> as {arg_name}, got {:?}",
            value
        )));
    };
    list.iter()
        .enumerate()
        .map(|(idx, item)| match item {
            Value::Str(text) => Ok(text),
            other => Err(RuntimeError::new(format!(
                "{builtin_name} expects String at {arg_name}[{idx}], got {:?}",
                other
            ))),
        })
        .collect()
}

fn decode_file_path_arg<'a>(
    vm: &VM,
    value: &'a Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<&'a str, RuntimeError> {
    let Value::Tagged { tag, fields } = value else {
        return Err(RuntimeError::new(format!(
            "{builtin_name} expects FilePath as {arg_name}, got {:?}",
            value
        )));
    };
    let Some(entry) = vm.type_registry().lookup(*tag) else {
        return Err(RuntimeError::new(format!(
            "{builtin_name} observed unknown FilePath tag {tag}"
        )));
    };
    if surface_path_name(&entry.name) != "FilePath" {
        return Err(RuntimeError::new(format!(
            "{builtin_name} expects FilePath as {arg_name}, got {}",
            entry.name
        )));
    }
    match fields.as_slice() {
        [Value::Str(raw)] => Ok(raw),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects FilePath.raw String field for {arg_name}, got {} fields",
            other.len()
        ))),
    }
}

fn decode_int_i64_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<i64, RuntimeError> {
    match value {
        Value::Int(num) => num.to_i64().ok_or_else(|| {
            RuntimeError::new(format!(
                "{builtin_name} Int argument {arg_name} is out of range for i64: {num}"
            ))
        }),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects Int as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_file_handle_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<FileHandleValue, RuntimeError> {
    match value {
        Value::FileHandle(handle) => Ok(handle.clone()),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects FileHandle as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_non_negative_int_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<usize, RuntimeError> {
    match value {
        Value::Int(num) => {
            if num < &int(0) {
                return Err(RuntimeError::new(format!(
                    "{builtin_name} expects non-negative Int as {arg_name}, got {num}"
                )));
            }
            num.to_usize().ok_or_else(|| {
                RuntimeError::new(format!(
                    "{builtin_name} Int argument {arg_name} is out of range for usize: {num}"
                ))
            })
        }
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects Int as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_callable_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<Callable, RuntimeError> {
    match value {
        Value::Callable(callable) => Ok(callable.clone()),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects callable value as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_file_mode(
    vm: &VM,
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<VmFileMode, RuntimeError> {
    let Value::Tagged { tag, .. } = value else {
        return Err(RuntimeError::new(format!(
            "{builtin_name} expects FileMode as {arg_name}, got {:?}",
            value
        )));
    };
    let entry = lookup_tagged_type_entry(
        vm,
        *tag,
        format!("{builtin_name} observed unknown FileMode tag {tag}"),
    )?;
    match type_name_leaf(&entry.name) {
        "Read" => Ok(VmFileMode::Read),
        "Write" => Ok(VmFileMode::Write),
        "Append" => Ok(VmFileMode::Append),
        "ReadWrite" => Ok(VmFileMode::ReadWrite),
        "ReadAppend" => Ok(VmFileMode::ReadAppend),
        _ => Err(RuntimeError::new(format!(
            "{builtin_name} expects FileMode as {arg_name}, got {}",
            entry.name
        ))),
    }
}

fn compile_cached_regex(pattern: &str, builtin_name: &str) -> Result<Regex, RuntimeError> {
    Regex::new(pattern).map_err(|err| {
        RuntimeError::new(format!(
            "{builtin_name} failed to compile cached regex pattern {:?}: {}",
            pattern, err
        ))
    })
}

fn slice_with_span<'a>(
    input: &'a str,
    start: usize,
    end: usize,
    builtin_name: &str,
) -> Result<&'a str, RuntimeError> {
    input.get(start..end).ok_or_else(|| {
        RuntimeError::new(format!(
            "{builtin_name} observed invalid regex span: {}..{}",
            start, end
        ))
    })
}

fn bit_mask(bit_index: usize) -> SurtrInt {
    int(1) << bit_index
}

fn decode_string_encoding(vm: &VM, value: &Value) -> Result<StringEncodingMode, RuntimeError> {
    let Value::Tagged { tag, .. } = value else {
        return Err(RuntimeError::new(
            "expected StringEncoding enum value for encoding argument",
        ));
    };
    let entry = lookup_tagged_type_entry(vm, *tag, format!("unknown StringEncoding tag: {}", tag))?;
    match type_name_leaf(&entry.name) {
        "Utf8" => Ok(StringEncodingMode::Utf8),
        "Ascii" => Ok(StringEncodingMode::Ascii),
        _other => Err(RuntimeError::new(format!(
            "expected StringEncoding variant, got {}",
            entry.name
        ))),
    }
}

fn type_name_leaf(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

fn lookup_tagged_type_entry<'a>(
    vm: &'a VM,
    tag: u32,
    unknown_message: String,
) -> Result<&'a TypeEntry, RuntimeError> {
    vm.type_registry()
        .lookup(tag)
        .ok_or_else(|| RuntimeError::new(unknown_message))
}

fn bit_index_to_usize(vm: &VM, index: &SurtrInt) -> Result<Result<usize, Value>, RuntimeError> {
    if index < &int(0) {
        return Ok(Err(err_result(
            vm,
            "NegativeBitIndex",
            &format!("bit index must be non-negative: {}", index),
        )));
    }

    index
        .to_usize()
        .map(Ok)
        .ok_or_else(|| RuntimeError::new(format!("bit index out of range for usize: {}", index)))
}

fn duration_payload<'a>(vm: &'a VM, value: &'a Value) -> Result<&'a SurtrInt, RuntimeError> {
    match value {
        Value::Tagged { tag, fields } => {
            let Some(entry) = vm.type_registry().lookup(*tag) else {
                return Err(RuntimeError::new(format!(
                    "Duration expects registered struct tag, got unknown tag {}",
                    tag
                )));
            };
            if !is_duration_type_name(&entry.name) {
                return Err(RuntimeError::new(format!(
                    "expected Duration struct tag, got {}",
                    entry.name
                )));
            }
            match fields.first() {
                Some(Value::Int(ms)) => Ok(ms),
                other => Err(RuntimeError::new(format!(
                    "Duration payload must store Int milliseconds, got {:?}",
                    other
                ))),
            }
        }
        other => Err(RuntimeError::new(format!(
            "expected Duration value, got {:?}",
            other
        ))),
    }
}

fn is_duration_type_name(name: &str) -> bool {
    surface_path_name(name) == "Duration"
}

fn duration_to_u64(
    vm: &VM,
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<u64, RuntimeError> {
    let ms = duration_payload(vm, value).map_err(|_| {
        RuntimeError::new(format!(
            "{builtin_name} expects Duration as {arg_name}, got {:?}",
            value
        ))
    })?;
    ms.to_u64().ok_or_else(|| {
        RuntimeError::new(format!(
            "{builtin_name} duration is out of range for u64 milliseconds: {ms}"
        ))
    })
}

#[derive(Debug, Clone)]
struct JsonRuntimeConstructors {
    null: u32,
    bool_: u32,
    int: u32,
    float: u32,
    string: u32,
    array: u32,
    object: u32,
}

#[derive(Debug)]
enum JsonStringifyError {
    Recoverable(String),
    Internal(String),
}

impl JsonStringifyError {
    fn recoverable(message: impl Into<String>) -> Self {
        Self::Recoverable(message.into())
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

fn json_constructors(vm: &VM) -> Result<JsonRuntimeConstructors, RuntimeError> {
    Ok(JsonRuntimeConstructors {
        null: find_variant_tag(vm, "JsonValue::Null")?,
        bool_: find_variant_tag(vm, "JsonValue::Bool")?,
        int: find_variant_tag(vm, "JsonValue::Int")?,
        float: find_variant_tag(vm, "JsonValue::Float")?,
        string: find_variant_tag(vm, "JsonValue::String")?,
        array: find_variant_tag(vm, "JsonValue::Array")?,
        object: find_variant_tag(vm, "JsonValue::Object")?,
    })
}

fn find_variant_tag(vm: &VM, qualified_variant: &str) -> Result<u32, RuntimeError> {
    vm.type_registry()
        .tag_by_name(qualified_variant)
        .ok_or_else(|| {
            RuntimeError::new(format!("missing Json runtime variant {qualified_variant}"))
        })
}

fn json_discriminant(index: i64) -> Value {
    Value::Int(BigInt::from(index))
}

fn json_variant(tag: u32, discriminant: i64, payload: Vec<Value>) -> Value {
    let mut fields = Vec::with_capacity(payload.len() + 1);
    fields.push(json_discriminant(discriminant));
    fields.extend(payload);
    Value::Tagged { tag, fields }
}

fn json_value_to_surtr(
    ctors: &JsonRuntimeConstructors,
    value: serde_json::Value,
) -> Result<Value, RuntimeError> {
    match value {
        serde_json::Value::Null => Ok(json_variant(ctors.null, 0, Vec::new())),
        serde_json::Value::Bool(value) => {
            Ok(json_variant(ctors.bool_, 1, vec![Value::Bool(value)]))
        }
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Ok(json_variant(
                    ctors.int,
                    2,
                    vec![Value::Int(BigInt::from(value))],
                ))
            } else if let Some(value) = number.as_u64() {
                Ok(json_variant(
                    ctors.int,
                    2,
                    vec![Value::Int(BigInt::from(value))],
                ))
            } else if let Some(value) = number.as_f64() {
                if value.is_finite() {
                    Ok(json_variant(ctors.float, 3, vec![Value::Float(value)]))
                } else {
                    Err(RuntimeError::new(
                        "JsonValue::Float cannot represent NaN or infinity",
                    ))
                }
            } else {
                Err(RuntimeError::new(
                    "serde_json number could not be represented",
                ))
            }
        }
        serde_json::Value::String(text) => {
            Ok(json_variant(ctors.string, 4, vec![Value::Str(text)]))
        }
        serde_json::Value::Array(values) => {
            let items = values
                .into_iter()
                .map(|item| json_value_to_surtr(ctors, item))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json_variant(
                ctors.array,
                5,
                vec![Value::List(ListHandle::from_items(items))],
            ))
        }
        serde_json::Value::Object(entries) => {
            let mut map = HashMapHandle::empty();
            for (key, value) in entries {
                let converted = json_value_to_surtr(ctors, value)?;
                map = map.insert(key, converted);
            }
            Ok(json_variant(ctors.object, 6, vec![Value::HashMap(map)]))
        }
    }
}

fn builtin_json_parse(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let [Value::Str(text)] = args.as_slice() else {
        return Err(RuntimeError::new("json_parse expects String"));
    };
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => {
            let ctors = json_constructors(vm)?;
            let converted = json_value_to_surtr(&ctors, value)?;
            Ok(ok_result(converted))
        }
        Err(err) => {
            let detail = err.to_string();
            Ok(err_result(
                vm,
                "JsonParseError",
                &format!(
                    "json parse error at {}:{}: {}",
                    err.line(),
                    err.column(),
                    detail
                ),
            ))
        }
    }
}

fn bigint_to_json_number(value: &BigInt) -> Result<serde_json::Number, JsonStringifyError> {
    if let Some(value) = value.to_i64() {
        Ok(serde_json::Number::from(value))
    } else if let Some(value) = value.to_u64() {
        Ok(serde_json::Number::from(value))
    } else {
        Err(JsonStringifyError::recoverable(format!(
            "JsonValue::Int cannot be represented as a JSON number: {value}"
        )))
    }
}

fn surtr_json_to_serde(
    ctors: &JsonRuntimeConstructors,
    value: &Value,
) -> Result<serde_json::Value, JsonStringifyError> {
    match value {
        Value::Tagged { tag, fields } if *tag == ctors.null && fields.len() == 1 => {
            Ok(serde_json::Value::Null)
        }
        Value::Tagged { tag, fields } if *tag == ctors.bool_ && fields.len() == 2 => {
            match &fields[1] {
                Value::Bool(value) => Ok(serde_json::Value::Bool(*value)),
                got => Err(JsonStringifyError::internal(format!(
                    "JsonValue::Bool expected Boolean, got {got:?}"
                ))),
            }
        }
        Value::Tagged { tag, fields } if *tag == ctors.int && fields.len() == 2 => {
            match &fields[1] {
                Value::Int(value) => bigint_to_json_number(value).map(serde_json::Value::Number),
                got => Err(JsonStringifyError::internal(format!(
                    "JsonValue::Int expected Int, got {got:?}"
                ))),
            }
        }
        Value::Tagged { tag, fields } if *tag == ctors.float && fields.len() == 2 => {
            match &fields[1] {
                Value::Float(value) => serde_json::Number::from_f64(*value)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| {
                        JsonStringifyError::recoverable(
                            "JsonValue::Float cannot represent NaN or infinity",
                        )
                    }),
                got => Err(JsonStringifyError::internal(format!(
                    "JsonValue::Float expected Float, got {got:?}"
                ))),
            }
        }
        Value::Tagged { tag, fields } if *tag == ctors.string && fields.len() == 2 => {
            match &fields[1] {
                Value::Str(text) => Ok(serde_json::Value::String(text.clone())),
                got => Err(JsonStringifyError::internal(format!(
                    "JsonValue::String expected String, got {got:?}"
                ))),
            }
        }
        Value::Tagged { tag, fields } if *tag == ctors.array && fields.len() == 2 => {
            match &fields[1] {
                Value::List(values) => values
                    .iter()
                    .map(|item| surtr_json_to_serde(ctors, &item))
                    .collect::<Result<Vec<_>, _>>()
                    .map(serde_json::Value::Array),
                got => Err(JsonStringifyError::internal(format!(
                    "JsonValue::Array expected List, got {got:?}"
                ))),
            }
        }
        Value::Tagged { tag, fields } if *tag == ctors.object && fields.len() == 2 => {
            match &fields[1] {
                Value::HashMap(map) => {
                    let mut object = serde_json::Map::new();
                    for (key, item) in map.sorted_entries() {
                        object.insert(key, surtr_json_to_serde(ctors, &item)?);
                    }
                    Ok(serde_json::Value::Object(object))
                }
                got => Err(JsonStringifyError::internal(format!(
                    "JsonValue::Object expected HashMap, got {got:?}"
                ))),
            }
        }
        Value::Tagged { tag, fields }
            if [
                ctors.bool_,
                ctors.int,
                ctors.float,
                ctors.string,
                ctors.array,
                ctors.object,
            ]
            .contains(tag) =>
        {
            Err(JsonStringifyError::internal(format!(
                "JsonValue variant has invalid arity: tag {tag}, fields {fields:?}"
            )))
        }
        other => Err(JsonStringifyError::recoverable(format!(
            "json_stringify expects JsonValue, got {other:?}"
        ))),
    }
}

fn builtin_json_stringify(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let [value] = args.as_slice() else {
        return Err(RuntimeError::new("json_stringify expects JsonValue"));
    };
    let ctors = json_constructors(vm)?;
    match surtr_json_to_serde(&ctors, value) {
        Ok(json) => Ok(ok_result(Value::Str(json.to_string()))),
        Err(JsonStringifyError::Recoverable(detail)) => Ok(err_result(
            vm,
            "JsonEncodeError",
            &format!("json encode error: {detail}"),
        )),
        Err(JsonStringifyError::Internal(message)) => Err(RuntimeError::new(message)),
    }
}

fn ok_result(value: Value) -> Value {
    Value::Tagged {
        tag: 0,
        fields: vec![value],
    }
}

fn tagged_by_name(vm: &VM, name: &str, fields: Vec<Value>) -> Result<Value, RuntimeError> {
    let tag = vm
        .type_registry()
        .tag_by_name(name)
        .ok_or_else(|| RuntimeError::new(format!("missing runtime type {name}")))?;
    Ok(Value::Tagged { tag, fields })
}

fn enum_variant_by_name(
    vm: &VM,
    name: &str,
    discriminant: i64,
    payload: Vec<Value>,
) -> Result<Value, RuntimeError> {
    let mut fields = Vec::with_capacity(payload.len() + 1);
    fields.push(Value::Int(int(discriminant)));
    fields.extend(payload);
    tagged_by_name(vm, name, fields)
}

fn option_none(vm: &VM) -> Result<Value, RuntimeError> {
    enum_variant_by_name(vm, "Option::None", 0, Vec::new())
}

fn option_some(vm: &VM, value: Value) -> Result<Value, RuntimeError> {
    enum_variant_by_name(vm, "Option::Some", 1, vec![value])
}

fn option_int(vm: &VM, value: Option<i128>) -> Result<Value, RuntimeError> {
    match value {
        Some(value) => option_some(vm, Value::Int(int(value))),
        None => option_none(vm),
    }
}

fn filesystem_file_path(vm: &VM, raw: &str) -> Result<Value, RuntimeError> {
    tagged_by_name(vm, "FilePath", vec![Value::Str(raw.to_string())])
}

fn filesystem_permissions(vm: &VM, permissions: &fs::Permissions) -> Result<Value, RuntimeError> {
    #[cfg(unix)]
    let executable = permissions.mode() & 0o111 != 0;
    #[cfg(not(unix))]
    let executable = false;

    tagged_by_name(
        vm,
        "FileSystemPermissions",
        vec![Value::Bool(permissions.readonly()), Value::Bool(executable)],
    )
}

fn system_time_epoch_ms(value: io::Result<SystemTime>) -> Option<i128> {
    value
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i128)
}

fn filesystem_metadata(vm: &VM, metadata: &fs::Metadata) -> Result<Value, RuntimeError> {
    tagged_by_name(
        vm,
        "FileSystemMetadata",
        vec![
            option_int(vm, Some(metadata.len() as i128))?,
            option_int(vm, system_time_epoch_ms(metadata.modified()))?,
            option_int(vm, system_time_epoch_ms(metadata.accessed()))?,
            option_int(vm, system_time_epoch_ms(metadata.created()))?,
            option_some(vm, filesystem_permissions(vm, &metadata.permissions())?)?,
        ],
    )
}

fn filesystem_entry_kind(vm: &VM, metadata: &fs::Metadata) -> Result<Value, RuntimeError> {
    let (name, discriminant) = if metadata.file_type().is_symlink() {
        ("FileSystemEntryKind::Symlink", 2)
    } else if metadata.is_file() {
        ("FileSystemEntryKind::File", 0)
    } else if metadata.is_dir() {
        ("FileSystemEntryKind::Directory", 1)
    } else {
        ("FileSystemEntryKind::Other", 3)
    };
    enum_variant_by_name(vm, name, discriminant, Vec::new())
}

fn filesystem_entry(vm: &VM, raw_path: &str) -> Result<Result<Value, Value>, RuntimeError> {
    let host_path = vm.resolve_host_path(raw_path);
    let metadata = match fs::symlink_metadata(&host_path) {
        Ok(metadata) => metadata,
        Err(err) => return Ok(Err(filesystem_io_error(vm, raw_path, err))),
    };
    let name = Path::new(raw_path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .or_else(|| {
            host_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| raw_path.to_string());
    Ok(Ok(tagged_by_name(
        vm,
        "FileSystemEntry",
        vec![
            filesystem_file_path(vm, raw_path)?,
            Value::Str(name),
            filesystem_entry_kind(vm, &metadata)?,
            filesystem_metadata(vm, &metadata)?,
        ],
    )?))
}

fn filesystem_snapshot(
    vm: &VM,
    root_raw: &str,
    max_depth: Option<usize>,
) -> Result<Value, RuntimeError> {
    let root_host = vm.resolve_host_path(root_raw);
    if !root_host.is_dir() {
        return Ok(filesystem_error(vm, "FileSystemNotDirectory", root_raw));
    }

    let mut paths = Vec::new();
    if let Err(err) =
        collect_filesystem_entries(vm, root_raw, 1, max_depth.unwrap_or(1), &mut paths)
    {
        return Ok(err);
    }
    paths.sort();

    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        match filesystem_entry(vm, &path)? {
            Ok(entry) => entries.push(entry),
            Err(err) => return Ok(err),
        }
    }

    Ok(ok_result(tagged_by_name(
        vm,
        "FileSystemSnapshot",
        vec![
            filesystem_file_path(vm, root_raw)?,
            Value::List(ListHandle::from_items(entries)),
        ],
    )?))
}

fn collect_filesystem_entries(
    vm: &VM,
    raw_path: &str,
    current_depth: usize,
    max_depth: usize,
    out: &mut Vec<String>,
) -> Result<(), Value> {
    if max_depth == 0 || current_depth > max_depth {
        return Ok(());
    }
    let host_path = vm.resolve_host_path(raw_path);
    let read_dir = match fs::read_dir(&host_path) {
        Ok(read_dir) => read_dir,
        Err(err) => return Err(filesystem_io_error(vm, raw_path, err)),
    };
    let mut children = Vec::new();
    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => return Err(filesystem_io_error(vm, raw_path, err)),
        };
        let raw_child = Path::new(raw_path)
            .join(entry.file_name())
            .to_string_lossy()
            .into_owned();
        children.push(raw_child);
    }
    children.sort();
    for child in children {
        let is_dir = vm.resolve_host_path(&child).is_dir();
        out.push(child.clone());
        if is_dir {
            collect_filesystem_entries(vm, &child, current_depth + 1, max_depth, out)?;
        }
    }
    Ok(())
}

fn canonicalize_shell_cwd(
    vm: &VM,
    host_path: &Path,
    path: &str,
) -> Result<std::path::PathBuf, Value> {
    fs::canonicalize(host_path).map_err(|err| {
        shell_error_with_message(
            vm,
            "ShellIoError",
            &format!("shell failed to canonicalize working directory {path}: {err}"),
        )
    })
}

fn err_value(rich: RichError) -> Value {
    Value::Error(Box::new(rich))
}

fn err_result_from_rich_error(rich: RichError) -> Value {
    Value::Tagged {
        tag: 1,
        fields: vec![err_value(rich)],
    }
}

fn decode_error_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<RichError, RuntimeError> {
    match value {
        Value::Error(rich) => Ok((**rich).clone()),
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects Error as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_result_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<Result<Value, RichError>, RuntimeError> {
    match value {
        Value::Tagged { tag: 0, fields } => match fields.as_slice() {
            [inner] => Ok(Ok(inner.clone())),
            other => Err(RuntimeError::new(format!(
                "{builtin_name} expects Ok with exactly one field for {arg_name}, got {}",
                other.len()
            ))),
        },
        Value::Tagged { tag: 1, fields } => match fields.as_slice() {
            [Value::Error(rich)] => Ok(Err((**rich).clone())),
            [other] => Err(RuntimeError::new(format!(
                "{builtin_name} expects Err(Error) for {arg_name}, got Err({:?})",
                other
            ))),
            other => Err(RuntimeError::new(format!(
                "{builtin_name} expects Err with exactly one field for {arg_name}, got {}",
                other.len()
            ))),
        },
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects Result as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn decode_unit_result_arg(
    value: &Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<Result<(), RichError>, RuntimeError> {
    match decode_result_arg(value, builtin_name, arg_name)? {
        Ok(Value::Unit) => Ok(Ok(())),
        Ok(other) => Err(RuntimeError::new(format!(
            "{builtin_name} expects Result<()> as {arg_name}, got Ok({:?})",
            other
        ))),
        Err(err) => Ok(Err(err)),
    }
}

fn file_path_error_result(vm: &VM, path: &str, err: io::Error) -> Value {
    let kind = match err.kind() {
        io::ErrorKind::NotFound => "FileNotFound",
        io::ErrorKind::PermissionDenied => "FilePermissionDenied",
        io::ErrorKind::AlreadyExists => "FileAlreadyExists",
        io::ErrorKind::InvalidInput => "FileInvalidPath",
        io::ErrorKind::InvalidData => "FileEncodingError",
        _ => "FileIoError",
    };
    let message = match kind {
        "FileNotFound" => format!("file not found: {path}"),
        "FilePermissionDenied" => format!("permission denied: {path}"),
        "FileAlreadyExists" => format!("file already exists: {path}"),
        "FileInvalidPath" => format!("invalid path: {path}"),
        "FileEncodingError" => format!("invalid UTF-8 while reading {path}: {err}"),
        _ => format!("file I/O failed for {path}: {err}"),
    };
    err_result(vm, kind, &message)
}

fn filesystem_io_error(vm: &VM, path: &str, err: io::Error) -> Value {
    let kind = match err.kind() {
        io::ErrorKind::NotFound => "FileSystemNotFound",
        io::ErrorKind::PermissionDenied => "FileSystemPermissionDenied",
        io::ErrorKind::AlreadyExists => "FileSystemAlreadyExists",
        io::ErrorKind::InvalidInput => "FileSystemInvalidPath",
        _ => "FileSystemIoError",
    };
    let message = match kind {
        "FileSystemNotFound" => format!("filesystem path not found: {path}"),
        "FileSystemPermissionDenied" => format!("filesystem permission denied: {path}"),
        "FileSystemAlreadyExists" => format!("filesystem path already exists: {path}"),
        "FileSystemInvalidPath" => format!("invalid filesystem path: {path}"),
        _ => format!("filesystem I/O failed for {path}: {err}"),
    };
    filesystem_error_with_message(vm, kind, &message)
}

fn filesystem_error(vm: &VM, kind: &str, path: &str) -> Value {
    let message = match kind {
        "FileSystemNotFound" => format!("filesystem path not found: {path}"),
        "FileSystemAlreadyExists" => format!("filesystem path already exists: {path}"),
        "FileSystemPermissionDenied" => format!("filesystem permission denied: {path}"),
        "FileSystemNotDirectory" => format!("filesystem path is not a directory: {path}"),
        "FileSystemIsDirectory" => format!("filesystem path is a directory: {path}"),
        "FileSystemInvalidPath" => format!("invalid filesystem path: {path}"),
        _ => path.to_string(),
    };
    filesystem_error_with_message(vm, kind, &message)
}

fn filesystem_error_with_message(vm: &VM, kind: &str, message: &str) -> Value {
    err_result(vm, kind, message)
}

fn shell_error_with_message(vm: &VM, kind: &str, message: &str) -> Value {
    err_result(vm, kind, message)
}

fn file_handle_error_result(vm: &VM, path: Option<&str>, err: VmFileError) -> Value {
    match err {
        VmFileError::Closed => err_result(vm, "FileClosed", "file is already closed"),
        VmFileError::Io(io_err) => {
            if let Some(path) = path {
                file_path_error_result(vm, path, io_err)
            } else {
                err_result(vm, "FileIoError", &format!("file I/O failed: {io_err}"))
            }
        }
        VmFileError::Encoding(message) => err_result(vm, "FileEncodingError", &message),
        VmFileError::Message(message) => err_result(vm, "FileIoError", &message),
    }
}

fn err_result(vm: &VM, kind: &str, message: &str) -> Value {
    let location = vm.runtime_error_location().unwrap_or_else(|| Location {
        file: vm.source_file().unwrap_or("<runtime>").to_string(),
        func: "<builtin>".into(),
        line: 0,
        column: 0,
        span_start: 0,
        span_end: 0,
    });

    err_result_from_rich_error(RichError {
        kind: kind.into(),
        message: message.into(),
        location,
        diagnostic: None,
        cause: None,
    })
}

fn none_result(vm: &VM) -> Value {
    err_result(vm, "NoneError", "None Value.")
}

#[cfg(test)]
mod tests {
    use super::{
        call_builtin, err_result_from_rich_error, inspect_value, json_variant, ok_result,
        BUILTIN_IMPLS,
    };
    use crate::vm::VM;
    use sindr::builtin::{builtin_id_by_name, builtin_meta_by_id, builtin_meta_by_name};
    use sindr::ir::{Bytecode, Constant, DocEntry, DocKind, FunctionEntry, FunctionFlags, Opcode};
    use sindr::primitives::int;
    use sindr::runtime::{
        Callable, CallableMetadata, CallableOrigin, CallableTarget, HashMapHandle, ListHandle,
        Location, RichError, TypeEntry, TypeKind, TypeRegistry, Value,
    };
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn test_vm() -> VM {
        let mut registry = TypeRegistry::new();
        registry.register(TypeEntry {
            tag: 2,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        VM::new(Bytecode {
            type_registry: registry,
            ..Bytecode::default()
        })
        .with_error_capture()
    }

    fn test_vm_with_types(entries: Vec<TypeEntry>) -> VM {
        let mut registry = TypeRegistry::new();
        for entry in entries {
            registry.register(entry);
        }
        VM::new(Bytecode {
            type_registry: registry,
            ..Bytecode::default()
        })
        .with_error_capture()
    }

    fn sample_error(kind: &str, message: &str) -> RichError {
        RichError {
            kind: kind.into(),
            message: message.into(),
            location: Location {
                file: "<test>".into(),
                func: "<test>".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 0,
            },
            diagnostic: None,
            cause: None,
        }
    }

    fn sample_error_value(kind: &str, message: &str) -> Value {
        Value::Error(Box::new(sample_error(kind, message)))
    }

    fn builtin_id(name: &str) -> u16 {
        builtin_id_by_name(name).unwrap_or_else(|| panic!("missing builtin metadata for {name}"))
    }

    fn json_type_entries() -> Vec<TypeEntry> {
        vec![
            TypeEntry {
                tag: 10,
                name: "JsonValue::Null".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec![],
                private_flags: vec![],
            },
            TypeEntry {
                tag: 11,
                name: "JsonValue::Bool".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec!["value".into()],
                private_flags: vec![false],
            },
            TypeEntry {
                tag: 12,
                name: "JsonValue::Int".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec!["value".into()],
                private_flags: vec![false],
            },
            TypeEntry {
                tag: 13,
                name: "JsonValue::Float".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec!["value".into()],
                private_flags: vec![false],
            },
            TypeEntry {
                tag: 14,
                name: "JsonValue::String".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec!["value".into()],
                private_flags: vec![false],
            },
            TypeEntry {
                tag: 15,
                name: "JsonValue::Array".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec!["values".into()],
                private_flags: vec![false],
            },
            TypeEntry {
                tag: 16,
                name: "JsonValue::Object".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec!["map".into()],
                private_flags: vec![false],
            },
        ]
    }

    fn json_vm() -> VM {
        test_vm_with_types(json_type_entries())
    }

    fn parse_json_ok(text: &str) -> Value {
        let mut vm = json_vm();
        let result = call_builtin(
            &mut vm,
            builtin_id("json_parse"),
            vec![Value::Str(text.into())],
        )
        .expect("json_parse should execute");
        match result {
            Value::Tagged { tag: 0, fields } => fields
                .into_iter()
                .next()
                .expect("Ok result should carry a value"),
            other => panic!("expected Ok result, got {:?}", other),
        }
    }

    fn assert_json_variant(value: &Value, name: &str) {
        let vm = json_vm();
        let Value::Tagged { tag, .. } = value else {
            panic!("expected tagged JsonValue, got {:?}", value);
        };
        let entry = vm
            .type_registry()
            .lookup(*tag)
            .unwrap_or_else(|| panic!("missing type entry for tag {tag}"));
        assert_eq!(entry.name, name);
    }

    fn json_int_value(value: i64) -> Value {
        json_variant(12, 2, vec![Value::Int(int(value))])
    }

    fn json_object_value(entries: Vec<(&str, Value)>) -> Value {
        json_variant(
            16,
            6,
            vec![Value::HashMap(HashMapHandle::from_entries(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.to_string(), value))
                    .collect(),
            ))],
        )
    }

    fn ok_string(value: &Value) -> &str {
        match value {
            Value::Tagged { tag: 0, fields } => match fields.as_slice() {
                [Value::Str(text)] => text,
                other => panic!("expected Ok(String), got {:?}", other),
            },
            other => panic!("expected Ok result, got {:?}", other),
        }
    }

    fn sandbox_dir(prefix: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tmp/sandbox")
            .join(format!("{prefix}-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("sandbox dir should be creatable");
        dir
    }

    fn ok_payload(value: Value) -> Value {
        match value {
            Value::Tagged { tag: 0, fields } => fields
                .into_iter()
                .next()
                .expect("Ok result should carry a payload"),
            other => panic!("expected Ok result, got {:?}", other),
        }
    }

    fn err_kind(value: &Value) -> &str {
        match value {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => &rich.kind,
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    fn filesystem_vm() -> VM {
        test_vm_with_types(vec![
            TypeEntry {
                tag: 20,
                name: "Option::None".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec![],
                private_flags: vec![],
            },
            TypeEntry {
                tag: 21,
                name: "Option::Some".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec!["value".into()],
                private_flags: vec![false],
            },
            TypeEntry {
                tag: 30,
                name: "FilePath".into(),
                kind: TypeKind::Struct,
                field_names: vec!["raw".into()],
                private_flags: vec![false],
            },
            TypeEntry {
                tag: 31,
                name: "FileSystemPermissions".into(),
                kind: TypeKind::Struct,
                field_names: vec!["read_only".into(), "executable".into()],
                private_flags: vec![false, false],
            },
            TypeEntry {
                tag: 32,
                name: "FileSystemMetadata".into(),
                kind: TypeKind::Struct,
                field_names: vec![
                    "size".into(),
                    "modified_at_epoch_ms".into(),
                    "accessed_at_epoch_ms".into(),
                    "created_at_epoch_ms".into(),
                    "permissions".into(),
                ],
                private_flags: vec![false, false, false, false, false],
            },
            TypeEntry {
                tag: 33,
                name: "FileSystemEntry".into(),
                kind: TypeKind::Struct,
                field_names: vec![
                    "path".into(),
                    "name".into(),
                    "kind".into(),
                    "metadata".into(),
                ],
                private_flags: vec![false, false, false, false],
            },
            TypeEntry {
                tag: 34,
                name: "FileSystemSnapshot".into(),
                kind: TypeKind::Struct,
                field_names: vec!["root".into(), "entries".into()],
                private_flags: vec![false, false],
            },
            TypeEntry {
                tag: 35,
                name: "FileSystemEntryKind::File".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec![],
                private_flags: vec![],
            },
            TypeEntry {
                tag: 36,
                name: "FileSystemEntryKind::Directory".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec![],
                private_flags: vec![],
            },
            TypeEntry {
                tag: 37,
                name: "FileSystemEntryKind::Symlink".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec![],
                private_flags: vec![],
            },
            TypeEntry {
                tag: 38,
                name: "FileSystemEntryKind::Other".into(),
                kind: TypeKind::EnumVariant,
                field_names: vec![],
                private_flags: vec![],
            },
            TypeEntry {
                tag: 39,
                name: "CommandResult".into(),
                kind: TypeKind::Struct,
                field_names: vec![
                    "command".into(),
                    "args".into(),
                    "exit_code".into(),
                    "stdout".into(),
                    "stderr".into(),
                ],
                private_flags: vec![false, false, false, false, false],
            },
        ])
    }

    #[test]
    fn builtin_impl_order_matches_metadata() {
        for (id, builtin) in BUILTIN_IMPLS.iter().enumerate() {
            let meta = builtin_meta_by_id(id as u16).expect("builtin metadata by id");
            assert_eq!(builtin.name, meta.name, "builtin impl mismatch at id {id}");
        }
    }

    #[test]
    fn json_parse_returns_err_for_malformed_json() {
        let mut vm = json_vm();
        let result = call_builtin(
            &mut vm,
            builtin_id("json_parse"),
            vec![Value::Str("{".into())],
        )
        .expect("json_parse itself should not raise RuntimeError for malformed user JSON");
        match result {
            Value::Tagged { tag: 1, fields } => match fields.as_slice() {
                [Value::Error(rich)] => assert_eq!(rich.kind, "JsonParseError"),
                other => panic!("expected JsonParseError value, got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn json_parse_classifies_int_and_float_numbers() {
        let int_value = parse_json_ok("1");
        assert_json_variant(&int_value, "JsonValue::Int");

        let decimal_value = parse_json_ok("1.5");
        assert_json_variant(&decimal_value, "JsonValue::Float");

        let exponent_value = parse_json_ok("1e2");
        assert_json_variant(&exponent_value, "JsonValue::Float");
    }

    #[test]
    fn json_stringify_uses_deterministic_object_key_order() {
        let mut vm = json_vm();
        let value = json_object_value(vec![("b", json_int_value(2)), ("a", json_int_value(1))]);
        let result = call_builtin(&mut vm, builtin_id("json_stringify"), vec![value])
            .expect("json_stringify should execute");
        assert_eq!(ok_string(&result), "{\"a\":1,\"b\":2}");
    }

    #[test]
    fn supervisor_spawn_accepts_three_argument_lowering_shape() {
        let mut vm = test_vm();
        let err = call_builtin(
            &mut vm,
            builtin_id("__supervisor_spawn"),
            vec![
                Value::Str("DynSup".into()),
                Value::Str("Worker".into()),
                Value::Int(int(1)),
            ],
        )
        .expect_err("non-callable third argument should fail after arity validation");
        assert!(
            err.message
                .contains("__supervisor_spawn expects callable init handler"),
            "unexpected error: {err}"
        );
    }

    /// Parse the `name(params) -> ret_ty` portion of a `def` declaration.
    fn parse_def_signature(def_rest: &str) -> (String, u8, String) {
        let (name, after_name) = def_rest
            .split_once('(')
            .expect("def declaration must include params");
        let mut angle_depth = 0usize;
        let mut paren_depth = 1usize;
        let mut close_idx = None;
        for (idx, ch) in after_name.char_indices() {
            match ch {
                '<' => angle_depth += 1,
                '>' => angle_depth = angle_depth.saturating_sub(1),
                '(' => paren_depth += 1,
                ')' => {
                    paren_depth -= 1;
                    if paren_depth == 0 && angle_depth == 0 {
                        close_idx = Some(idx);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close_idx = close_idx.expect("def declaration must close params");
        let params = &after_name[..close_idx];
        let after_params = &after_name[close_idx + 1..];
        let ret_ty = after_params
            .trim()
            .strip_prefix("->")
            .expect("def declaration must include return type")
            .trim();
        let param_tys: Vec<String> = if params.trim().is_empty() {
            Vec::new()
        } else {
            let mut params_out = Vec::new();
            let mut start = 0usize;
            let mut angle_depth = 0usize;
            let mut paren_depth = 0usize;
            for (idx, ch) in params.char_indices() {
                match ch {
                    '<' => angle_depth += 1,
                    '>' => angle_depth = angle_depth.saturating_sub(1),
                    '(' => paren_depth += 1,
                    ')' => paren_depth = paren_depth.saturating_sub(1),
                    ',' if angle_depth == 0 && paren_depth == 0 => {
                        params_out.push(params[start..idx].trim().to_string());
                        start = idx + 1;
                    }
                    _ => {}
                }
            }
            params_out.push(params[start..].trim().to_string());

            params_out
                .into_iter()
                .map(|param| {
                    let (_, ty) = param
                        .split_once(':')
                        .expect("builtin params must have `name: Type` form");
                    ty.trim().to_string()
                })
                .collect()
        };
        let sig = format!("({}) -> {}", param_tys.join(", "), ret_ty);
        (name.trim().to_string(), param_tys.len() as u8, sig)
    }

    #[test]
    fn builtin_srt_and_builtin_meta_are_aligned() {
        let sources = [
            include_str!("../../../lib/bootstrap.srt"),
            include_str!("../../../lib/kernel.srt"),
            include_str!("../../../lib/types/int.srt"),
            include_str!("../../../lib/types/list.srt"),
            include_str!("../../../lib/types/generator.srt"),
            include_str!("../../../lib/types/hash_map.srt"),
            include_str!("../../../lib/types/result.srt"),
            include_str!("../../../lib/facet.srt"),
            include_str!("../../../lib/types/string.srt"),
            include_str!("../../../lib/types/regex.srt"),
            include_str!("../../../lib/Random.srt"),
            include_str!("../../../lib/file.srt"),
            include_str!("../../../lib/FileSystem.srt"),
            include_str!("../../../lib/Shell.srt"),
        ];

        // Collect all lines across the std-module files that currently declare
        // builtin value surfaces. Bootstrap intentionally stays almost empty,
        // Kernel owns the cross-cutting builtins, Int currently carries both
        // arithmetic-result builtins and bit-shift helpers, List declares
        // the O(1) length helper, Result carries result/error helpers, and
        // String carries encoding helpers and Regex carries regex wrappers.
        let all_lines: Vec<&str> = sources
            .iter()
            .flat_map(|s| s.lines())
            .map(str::trim)
            .collect();

        // For each @builtin annotation, find the associated def signature.
        // Annotation order is flexible:
        // - `@builtin def ...` can appear inline
        // - `@builtin` can appear on its own line before a following `def`
        //
        // We intentionally scan raw source text here instead of depending on
        // parser lowering details, because this test is meant to guard the
        // human-maintained std-module declaration layer against drift from
        // `BUILTIN_METAS`.
        let mut entries: Vec<(String, u8, String)> = Vec::new();
        let mut i = 0;
        while i < all_lines.len() {
            let line = all_lines[i];
            if let Some(rest) = line.strip_prefix("@builtin def ") {
                // Inline form: @builtin def name(params) -> ret
                let entry = parse_def_signature(rest);
                if let Some(meta) = builtin_meta_by_name(&entry.0) {
                    // Keep only declarations that map to a concrete runtime
                    // builtin contract. Some same-name surface declarations
                    // are lowered as special forms and intentionally have
                    // different signatures.
                    if meta.arity == entry.1 && meta.sig_str == entry.2 {
                        entries.push(entry);
                    }
                }
            } else if line == "@builtin" {
                // Standalone form: find the next `def` line.
                let mut j = i + 1;
                while j < all_lines.len() {
                    let next = all_lines[j];
                    if let Some(rest) = next.strip_prefix("def ") {
                        let entry = parse_def_signature(rest);
                        if let Some(meta) = builtin_meta_by_name(&entry.0) {
                            if meta.arity == entry.1 && meta.sig_str == entry.2 {
                                entries.push(entry);
                            }
                        }
                        break;
                    }
                    j += 1;
                }
            }
            i += 1;
        }

        // The stdlib declaration layer intentionally exposes only the
        // user-surface builtins. Some runtime builtins (for example the
        // trait-backed `to_string`) remain in `BUILTIN_METAS` without a
        // matching `@builtin def` surface declaration.
        //
        // So this test verifies:
        // - every declared builtin surface matches `BUILTIN_METAS`
        // - no declared builtin is duplicated
        // - hidden/runtime-only builtins are allowed to exist only in the
        //   metadata table

        // Source layout is allowed to group builtins by module ownership
        // rather than by builtin id order, so compare by builtin name instead
        // of relying on declaration order in `lib/*.srt`.
        let mut entry_map = std::collections::BTreeMap::new();
        for (name, arity, sig_str) in entries {
            let prev = entry_map.insert(name.clone(), (arity, sig_str));
            assert!(prev.is_none(), "duplicate builtin declaration for {name}");
        }

        for (name, (arity, sig_str)) in &entry_map {
            let meta = builtin_meta_by_name(name)
                .unwrap_or_else(|| panic!("declared builtin {name} is missing from BUILTIN_METAS"));
            assert_eq!(*arity, meta.arity, "arity mismatch for {}", meta.name);
            assert_eq!(sig_str, &meta.sig_str, "sig mismatch for {}", meta.name);
        }

        assert!(
            !entry_map.contains_key("to_string"),
            "to_string is trait-backed and should not be declared via @builtin def"
        );
    }

    #[test]
    fn random_seeded_range_is_repeatable_and_returns_next_state() {
        let mut vm = test_vm();
        let seed = call_builtin(&mut vm, builtin_id("seed"), vec![Value::Int(int(123))])
            .expect("seed should return RandomGenerator");
        let Value::RandomGenerator(original_rng) = seed.clone() else {
            panic!("expected RandomGenerator from seed");
        };

        let first = call_builtin(
            &mut vm,
            builtin_id("next_int_range"),
            vec![seed.clone(), Value::Int(int(-3)), Value::Int(int(3))],
        )
        .expect("next_int_range should return Result");
        let second = call_builtin(
            &mut vm,
            builtin_id("next_int_range"),
            vec![seed, Value::Int(int(-3)), Value::Int(int(3))],
        )
        .expect("next_int_range should return Result");
        assert_eq!(first, second, "same seed should produce same first value");

        let Value::Tagged { tag: 0, fields } = first else {
            panic!("expected Ok((Int, RandomGenerator))");
        };
        let Some(Value::Tuple(items)) = fields.first() else {
            panic!("expected tuple payload");
        };
        let [Value::Int(value), Value::RandomGenerator(next_rng)] = items.as_slice() else {
            panic!("expected Int and next RandomGenerator");
        };
        assert!(value >= &int(-3) && value < &int(3));
        assert_ne!(
            *next_rng, original_rng,
            "equal calls return equal next states, but the state should be opaque and stable"
        );
    }

    #[test]
    fn process_sleep_accepts_zero_duration_value() {
        let mut vm = test_vm();
        let slept = call_builtin(
            &mut vm,
            builtin_id("__process_sleep"),
            vec![Value::Tagged {
                tag: 2,
                fields: vec![Value::Int(int(0))],
            }],
        )
        .expect("process sleep should return Result");
        assert_eq!(slept, super::ok_result(Value::Unit));
    }

    #[test]
    fn random_ranges_validate_half_open_bounds() {
        let mut vm = test_vm();

        let invalid_until =
            call_builtin(&mut vm, builtin_id("int_until"), vec![Value::Int(int(0))])
                .expect("int_until should return Result");
        match invalid_until {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => assert_eq!(rich.kind, "InvalidRandomRange"),
                other => panic!("expected InvalidRandomRange error, got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }

        let invalid_range = call_builtin(
            &mut vm,
            builtin_id("int_range"),
            vec![Value::Int(int(4)), Value::Int(int(4))],
        )
        .expect("int_range should return Result");
        match invalid_range {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => assert_eq!(rich.kind, "InvalidRandomRange"),
                other => panic!("expected InvalidRandomRange error, got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn regex_compile_returns_ok_and_err_shapes() {
        let mut vm = test_vm();

        let ok = call_builtin(
            &mut vm,
            builtin_id("compile"),
            vec![Value::Str("^[a-z]+$".into())],
        )
        .expect("compile should return Result");
        match ok {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(
                    fields.first(),
                    Some(Value::Regex(handle)) if handle.pattern == "^[a-z]+$"
                ));
            }
            other => panic!("expected Ok(Regex), got {:?}", other),
        }

        let err = call_builtin(&mut vm, builtin_id("compile"), vec![Value::Str("[".into())])
            .expect("compile should return Result");
        match err {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "RegexCompileError");
                    assert!(
                        !rich.message.is_empty(),
                        "RegexCompileError should carry regex parser detail"
                    );
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn regex_capture_access_and_find_helpers_work() {
        let mut vm = test_vm();
        let compiled = call_builtin(
            &mut vm,
            builtin_id("compile"),
            vec![Value::Str("^(?<name>[A-Za-z]+)-(?<id>[0-9]+)$".into())],
        )
        .expect("compile should return Result");
        let regex_value = match compiled {
            Value::Tagged { tag: 0, fields } => {
                fields.first().expect("Ok should have payload").clone()
            }
            other => panic!("expected Ok result, got {:?}", other),
        };

        let captures = call_builtin(
            &mut vm,
            builtin_id("captures"),
            vec![regex_value.clone(), Value::Str("alice-42".into())],
        )
        .expect("captures should return Result");
        let captures_value = match captures {
            Value::Tagged { tag: 0, fields } => {
                fields.first().expect("Ok should have payload").clone()
            }
            other => panic!("expected Ok result, got {:?}", other),
        };

        let name = call_builtin(
            &mut vm,
            builtin_id("get_name"),
            vec![captures_value.clone(), Value::Str("name".into())],
        )
        .expect("get_name should return Result");
        assert!(matches!(
            name,
            Value::Tagged { tag: 0, fields } if matches!(fields.first(), Some(Value::Str(s)) if s == "alice")
        ));

        let full = call_builtin(&mut vm, builtin_id("whole"), vec![captures_value.clone()])
            .expect("whole should succeed");
        assert!(matches!(full, Value::Str(text) if text == "alice-42"));

        let count = call_builtin(
            &mut vm,
            builtin_id("capture_count"),
            vec![captures_value.clone()],
        )
        .expect("capture_count should succeed");
        assert!(matches!(count, Value::Int(value) if value == int(3)));

        let found = call_builtin(
            &mut vm,
            builtin_id("find"),
            vec![regex_value.clone(), Value::Str("alice-42".into())],
        )
        .expect("find should return Result");
        let match_value = match found {
            Value::Tagged { tag: 0, fields } => {
                fields.first().expect("Ok should have payload").clone()
            }
            other => panic!("expected Ok result, got {:?}", other),
        };

        let text = call_builtin(&mut vm, builtin_id("text"), vec![match_value.clone()])
            .expect("text should succeed");
        assert!(matches!(text, Value::Str(s) if s == "alice-42"));

        let start = call_builtin(&mut vm, builtin_id("start"), vec![match_value.clone()])
            .expect("start should succeed");
        assert!(matches!(start, Value::Int(value) if value == int(0)));

        let end = call_builtin(&mut vm, builtin_id("end"), vec![match_value])
            .expect("end should succeed");
        assert!(matches!(end, Value::Int(value) if value == int(8)));
    }

    #[test]
    fn safe_mod_returns_zero_division_error_result() {
        let mut vm = test_vm();
        let value = call_builtin(
            &mut vm,
            builtin_id("safe_mod"),
            vec![Value::Int(int(10)), Value::Int(int(0))],
        )
        .expect("safe_mod should return Result");
        match value {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "ZeroDivisionError");
                    assert_eq!(rich.message, "division by zero");
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn safe_mod_rejects_non_int_arguments() {
        let mut vm = test_vm();
        let err = call_builtin(
            &mut vm,
            builtin_id("safe_mod"),
            vec![Value::Bool(true), Value::Int(int(1))],
        )
        .expect_err("safe_mod must reject non-int inputs");
        assert!(err.message.contains("safe_mod expects (Int, Int)"));
    }

    #[test]
    fn float_builtins_reject_non_finite_inputs_and_results() {
        let mut vm = test_vm();

        let err = call_builtin(
            &mut vm,
            builtin_id("__operator_float_add"),
            vec![Value::Float(f64::MAX), Value::Float(f64::MAX)],
        )
        .expect_err("float add must reject infinity result");
        assert!(err.message.contains("non-finite value"));

        let err = call_builtin(
            &mut vm,
            builtin_id("__compare_float"),
            vec![Value::Float(f64::INFINITY), Value::Float(1.0)],
        )
        .expect_err("float compare must reject infinity input");
        assert!(err
            .message
            .contains("__compare_float expects finite Float values"));
    }

    #[test]
    fn float_builtins_expose_rounding_and_constants() {
        let mut vm = test_vm();

        let floor = call_builtin(&mut vm, builtin_id("floor"), vec![Value::Float(1.8)])
            .expect("floor should succeed");
        assert!(matches!(floor, Value::Float(value) if value == 1.0));

        let round = call_builtin(&mut vm, builtin_id("round"), vec![Value::Float(-1.5)])
            .expect("round should succeed");
        assert!(matches!(round, Value::Float(value) if value == -2.0));

        let pi = call_builtin(&mut vm, builtin_id("pi"), vec![]).expect("pi should succeed");
        assert!(matches!(pi, Value::Float(value) if value == std::f64::consts::PI));

        let e = call_builtin(&mut vm, builtin_id("e"), vec![]).expect("e should succeed");
        assert!(matches!(e, Value::Float(value) if value == std::f64::consts::E));
    }

    #[test]
    fn string_split_builtin_preserves_string_contract() {
        let mut vm = test_vm();
        let parts = call_builtin(
            &mut vm,
            builtin_id("string_split"),
            vec![Value::Str("あ|b|".into()), Value::Str("|".into())],
        )
        .expect("string_split should succeed");
        match parts {
            Value::List(list) => assert_eq!(
                list.iter().collect::<Vec<_>>(),
                vec![
                    Value::Str("あ".into()),
                    Value::Str("b".into()),
                    Value::Str("".into()),
                ]
            ),
            other => panic!("expected List, got {other:?}"),
        }

        let chars = call_builtin(
            &mut vm,
            builtin_id("string_split"),
            vec![Value::Str("あb".into()), Value::Str("".into())],
        )
        .expect("string_split should split empty separator into chars");
        match chars {
            Value::List(list) => assert_eq!(
                list.iter().collect::<Vec<_>>(),
                vec![Value::Str("あ".into()), Value::Str("b".into())]
            ),
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn string_replace_builtin_preserves_string_contract() {
        let mut vm = test_vm();
        let replaced = call_builtin(
            &mut vm,
            builtin_id("string_replace"),
            vec![
                Value::Str("banana".into()),
                Value::Str("na".into()),
                Value::Str("NA".into()),
            ],
        )
        .expect("string_replace should succeed");
        assert_eq!(replaced, Value::Str("baNANA".into()));

        let unchanged = call_builtin(
            &mut vm,
            builtin_id("string_replace"),
            vec![
                Value::Str("surtr".into()),
                Value::Str("".into()),
                Value::Str("-".into()),
            ],
        )
        .expect("string_replace should leave empty pattern unchanged");
        assert_eq!(unchanged, Value::Str("surtr".into()));
    }

    #[test]
    fn shl_returns_result_and_negative_shift_error() {
        let mut vm = test_vm();
        let ok = call_builtin(
            &mut vm,
            builtin_id("shl"),
            vec![Value::Int(int(2)), Value::Int(int(3))],
        )
        .expect("shl should return Result");
        match ok {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(16)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let err = call_builtin(
            &mut vm,
            builtin_id("shl"),
            vec![Value::Int(int(2)), Value::Int(int(-1))],
        )
        .expect("negative shl should still return Result");
        match err {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "NegativeShiftCount");
                    assert_eq!(rich.message, "shift amount must be non-negative: -1");
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn shr_returns_result_and_negative_shift_error() {
        let mut vm = test_vm();
        let ok = call_builtin(
            &mut vm,
            builtin_id("shr"),
            vec![Value::Int(int(16)), Value::Int(int(2))],
        )
        .expect("shr should return Result");
        match ok {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(4)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let err = call_builtin(
            &mut vm,
            builtin_id("shr"),
            vec![Value::Int(int(2)), Value::Int(int(-1))],
        )
        .expect("negative shr should still return Result");
        match err {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "NegativeShiftCount");
                    assert_eq!(rich.message, "shift amount must be non-negative: -1");
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn bitwise_builtins_execute_on_ints() {
        let mut vm = test_vm();

        let bit_and = call_builtin(
            &mut vm,
            builtin_id("bit_and"),
            vec![Value::Int(int(6)), Value::Int(int(3))],
        )
        .expect("bit_and should succeed");
        assert_eq!(bit_and, Value::Int(int(2)));

        let bit_or = call_builtin(
            &mut vm,
            builtin_id("bit_or"),
            vec![Value::Int(int(6)), Value::Int(int(3))],
        )
        .expect("bit_or should succeed");
        assert_eq!(bit_or, Value::Int(int(7)));

        let bit_xor = call_builtin(
            &mut vm,
            builtin_id("bit_xor"),
            vec![Value::Int(int(6)), Value::Int(int(3))],
        )
        .expect("bit_xor should succeed");
        assert_eq!(bit_xor, Value::Int(int(5)));

        let bit_not = call_builtin(&mut vm, builtin_id("bit_not"), vec![Value::Int(int(6))])
            .expect("bit_not should succeed");
        assert_eq!(bit_not, Value::Int(int(-7)));
    }

    #[test]
    fn bit_index_helpers_return_results_and_negative_index_errors() {
        let mut vm = test_vm();

        let tested = call_builtin(
            &mut vm,
            builtin_id("test_bit"),
            vec![Value::Int(int(5)), Value::Int(int(2))],
        )
        .expect("test_bit should return Result");
        match tested {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Bool(true))));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let negative = call_builtin(
            &mut vm,
            builtin_id("test_bit"),
            vec![Value::Int(int(5)), Value::Int(int(-1))],
        )
        .expect("negative test_bit should still return Result");
        match negative {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "NegativeBitIndex");
                    assert_eq!(rich.message, "bit index must be non-negative: -1");
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }

        let set = call_builtin(
            &mut vm,
            builtin_id("set_bit"),
            vec![Value::Int(int(0)), Value::Int(int(1))],
        )
        .expect("set_bit should return Result");
        match set {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(2)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let cleared = call_builtin(
            &mut vm,
            builtin_id("clear_bit"),
            vec![Value::Int(int(7)), Value::Int(int(1))],
        )
        .expect("clear_bit should return Result");
        match cleared {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(5)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let toggled = call_builtin(
            &mut vm,
            builtin_id("toggle_bit"),
            vec![Value::Int(int(5)), Value::Int(int(0))],
        )
        .expect("toggle_bit should return Result");
        match toggled {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(4)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }
    }

    #[test]
    fn codepoints_utf8_returns_bytes() {
        let mut vm = test_vm_with_types(vec![TypeEntry {
            tag: 200,
            name: "StringEncoding::Utf8".into(),
            kind: TypeKind::EnumVariant,
            field_names: vec![],
            private_flags: vec![],
        }]);
        let value = call_builtin(
            &mut vm,
            builtin_id("codepoints"),
            vec![
                Value::Str("Aあ".into()),
                Value::Tagged {
                    tag: 200,
                    fields: vec![Value::Int(int(0))],
                },
            ],
        )
        .expect("codepoints should return Result");
        match value {
            Value::Tagged { tag: 0, fields } => match fields.first() {
                Some(Value::List(list)) => {
                    let ints = list
                        .iter()
                        .map(|value| match value {
                            Value::Int(n) => n.to_string(),
                            other => panic!("expected int byte, got {:?}", other),
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(ints, vec!["65", "227", "129", "130"]);
                }
                other => panic!("expected Ok(List<Int>), got {:?}", other),
            },
            other => panic!("expected Ok result, got {:?}", other),
        }
    }

    #[test]
    fn from_codepoints_ascii_rejects_out_of_range_values() {
        let mut vm = test_vm_with_types(vec![TypeEntry {
            tag: 201,
            name: "StringEncoding::Ascii".into(),
            kind: TypeKind::EnumVariant,
            field_names: vec![],
            private_flags: vec![],
        }]);
        let value = call_builtin(
            &mut vm,
            builtin_id("from_codepoints"),
            vec![
                Value::List(ListHandle::from_items(vec![Value::Int(int(128))])),
                Value::Tagged {
                    tag: 201,
                    fields: vec![Value::Int(int(1))],
                },
            ],
        )
        .expect("from_codepoints should return Result");
        match value {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "InvalidStringEncoding");
                    assert_eq!(rich.message, "ASCII code out of range at index 0: 128");
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn group_count_returns_first_seen_counts_as_tuple_list() {
        let mut vm = test_vm();
        let value = call_builtin(
            &mut vm,
            builtin_id("group_count"),
            vec![Value::List(ListHandle::from_items(vec![
                Value::Str("a".into()),
                Value::Str("b".into()),
                Value::Str("a".into()),
                Value::Str("c".into()),
                Value::Str("b".into()),
                Value::Str("a".into()),
            ]))],
        )
        .expect("group_count should return list");

        match value {
            Value::List(list) => {
                let rendered = list
                    .iter()
                    .map(|value| match value {
                        Value::Tuple(items) => items
                            .iter()
                            .map(|item| item.to_display_string(vm.type_registry()))
                            .collect::<Vec<_>>()
                            .join(":"),
                        other => panic!("expected tuple entry, got {:?}", other),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(rendered, vec!["a:3", "b:2", "c:1"]);
            }
            other => panic!("expected List result, got {:?}", other),
        }
    }

    #[test]
    fn zip_returns_shortest_prefix_as_tuple_list() {
        let mut vm = test_vm();
        let value = call_builtin(
            &mut vm,
            builtin_id("zip"),
            vec![
                Value::List(ListHandle::from_items(vec![
                    Value::Int(int(1)),
                    Value::Int(int(2)),
                    Value::Int(int(3)),
                ])),
                Value::List(ListHandle::from_items(vec![
                    Value::Str("x".into()),
                    Value::Str("y".into()),
                ])),
            ],
        )
        .expect("zip should return list");

        match value {
            Value::List(list) => {
                let rendered = list
                    .iter()
                    .map(|value| match value {
                        Value::Tuple(items) => items
                            .iter()
                            .map(|item| item.to_display_string(vm.type_registry()))
                            .collect::<Vec<_>>()
                            .join(":"),
                        other => panic!("expected tuple entry, got {:?}", other),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(rendered, vec!["1:x", "2:y"]);
            }
            other => panic!("expected List result, got {:?}", other),
        }
    }

    #[test]
    fn hash_map_builtins_preserve_order_and_overwrite_semantics() {
        let mut vm = test_vm();
        let entries = Value::List(ListHandle::from_items(vec![
            Value::Tuple(vec![Value::Str("a".into()), Value::Int(int(1))]),
            Value::Tuple(vec![Value::Str("b".into()), Value::Int(int(2))]),
            Value::Tuple(vec![Value::Str("a".into()), Value::Int(int(3))]),
        ]));
        let map = call_builtin(&mut vm, builtin_id("map_from_entries"), vec![entries])
            .expect("map_from_entries should succeed");

        let keys = call_builtin(&mut vm, builtin_id("map_keys"), vec![map.clone()])
            .expect("map_keys should succeed");
        match keys {
            Value::List(list) => {
                let rendered = list
                    .iter()
                    .map(|value| match value {
                        Value::Str(text) => text,
                        other => panic!("expected String key, got {:?}", other),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(rendered, vec!["a", "b"]);
            }
            other => panic!("expected List<String>, got {:?}", other),
        }

        let values = call_builtin(&mut vm, builtin_id("map_values_list"), vec![map.clone()])
            .expect("map_values_list should succeed");
        match values {
            Value::List(list) => {
                let rendered = list
                    .iter()
                    .map(|value| match value {
                        Value::Int(n) => n.to_string(),
                        other => panic!("expected Int value, got {:?}", other),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(rendered, vec!["3", "2"]);
            }
            other => panic!("expected List<Int>, got {:?}", other),
        }

        let got = call_builtin(
            &mut vm,
            builtin_id("map_get"),
            vec![map.clone(), Value::Str("a".into())],
        )
        .expect("map_get should return Result");
        assert!(matches!(
            got,
            Value::Tagged { tag: 0, fields } if matches!(fields.first(), Some(Value::Int(value)) if *value == int(3))
        ));

        let miss = call_builtin(
            &mut vm,
            builtin_id("map_get"),
            vec![map.clone(), Value::Str("missing".into())],
        )
        .expect("map_get should return Result");
        assert!(matches!(
            miss,
            Value::Tagged { tag: 1, fields } if matches!(fields.first(), Some(Value::Error(rich)) if rich.kind == "NoneError")
        ));

        let removed = call_builtin(
            &mut vm,
            builtin_id("map_remove"),
            vec![map.clone(), Value::Str("b".into())],
        )
        .expect("map_remove should succeed");
        let removed_keys = call_builtin(&mut vm, builtin_id("map_keys"), vec![removed])
            .expect("map_keys after remove should succeed");
        match removed_keys {
            Value::List(list) => {
                let rendered = list
                    .iter()
                    .map(|value| match value {
                        Value::Str(text) => text,
                        other => panic!("expected String key, got {:?}", other),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(rendered, vec!["a"]);
            }
            other => panic!("expected List<String>, got {:?}", other),
        }
    }

    #[test]
    fn facet_list_get_and_set_report_index_bounds() {
        let mut vm = test_vm();
        let list = Value::List(ListHandle::from_items(vec![
            Value::Int(int(10)),
            Value::Int(int(20)),
        ]));

        let got = call_builtin(
            &mut vm,
            builtin_id("__facet_list_get"),
            vec![list.clone(), Value::Int(int(1))],
        )
        .expect("facet list get should return Result");
        assert!(matches!(got, Value::Tagged { tag: 0, .. }));

        let missing = call_builtin(
            &mut vm,
            builtin_id("__facet_list_get"),
            vec![list.clone(), Value::Int(int(9))],
        )
        .expect("facet list miss should return Err result");
        assert!(
            matches!(
                missing,
                Value::Tagged {
                    tag: 1,
                    ref fields
                }
                    if matches!(fields.first(), Some(Value::Error(rich)) if rich.kind == "IndexOutOfBounds")
            ),
            "{missing:?}"
        );

        let updated = call_builtin(
            &mut vm,
            builtin_id("__facet_list_set"),
            vec![list, Value::Int(int(0)), Value::Int(int(99))],
        )
        .expect("facet list set should return Result");
        assert!(matches!(updated, Value::Tagged { tag: 0, .. }));
    }

    #[test]
    fn facet_map_get_and_set_report_key_not_found() {
        let mut vm = test_vm();
        let map = Value::HashMap(HashMapHandle::from_entries(vec![(
            "talk".into(),
            Value::Int(int(80)),
        )]));

        let got = call_builtin(
            &mut vm,
            builtin_id("__facet_map_get"),
            vec![map.clone(), Value::Str("talk".into())],
        )
        .expect("facet map get should return Result");
        assert!(matches!(got, Value::Tagged { tag: 0, .. }));

        let missing = call_builtin(
            &mut vm,
            builtin_id("__facet_map_set_existing"),
            vec![map, Value::Str("missing".into()), Value::Int(int(1))],
        )
        .expect("facet map miss should return Err result");
        assert!(
            matches!(
                missing,
                Value::Tagged {
                    tag: 1,
                    ref fields
                }
                    if matches!(fields.first(), Some(Value::Error(rich)) if rich.kind == "KeyNotFound")
            ),
            "{missing:?}"
        );
    }

    #[test]
    fn inspect_formats_hash_map_named_style() {
        let vm = test_vm();
        let value = Value::HashMap(HashMapHandle::from_entries(vec![
            ("line\nfeed".into(), Value::Int(int(1))),
            ("path\\to".into(), Value::Int(int(2))),
        ]));
        assert_eq!(
            inspect_value(&vm, &value),
            "hash![\"line\\nfeed\" => 1, \"path\\\\to\" => 2]"
        );
    }

    #[test]
    fn inspect_quotes_strings_recursively() {
        let vm = test_vm();
        let value = Value::Tuple(vec![
            Value::Str("hello".into()),
            Value::List(ListHandle::from_items(vec![Value::Str(
                "line\nfeed".into(),
            )])),
            Value::Tagged {
                tag: 0,
                fields: vec![Value::Str("world".into())],
            },
        ]);

        assert_eq!(
            inspect_value(&vm, &value),
            "(\"hello\", [\"line\\nfeed\"], Ok(\"world\"))"
        );
    }

    #[test]
    fn io_get_line_reads_injected_input_and_strips_newline() {
        let mut vm = test_vm()
            .with_output_capture()
            .with_stdin_input("surtr\r\nnext\n");
        let value = call_builtin(
            &mut vm,
            builtin_id("io_get_line"),
            vec![Value::Str("name> ".into())],
        )
        .expect("io_get_line should run");

        assert_eq!(vm.captured_stdout(), Some(&["name> ".to_string()][..]));
        assert!(
            matches!(value, Value::Tagged { tag: 0, fields } if matches!(fields.as_slice(), [Value::Str(text)] if text == "surtr"))
        );
    }

    #[test]
    fn io_get_reads_one_injected_character() {
        let mut vm = test_vm().with_stdin_input("あb");
        let first = call_builtin(
            &mut vm,
            builtin_id("io_get"),
            vec![Value::Str(String::new())],
        )
        .expect("first io_get should run");
        let second = call_builtin(
            &mut vm,
            builtin_id("io_get"),
            vec![Value::Str(String::new())],
        )
        .expect("second io_get should run");

        assert!(
            matches!(first, Value::Tagged { tag: 0, fields } if matches!(fields.as_slice(), [Value::Str(text)] if text == "あ"))
        );
        assert!(
            matches!(second, Value::Tagged { tag: 0, fields } if matches!(fields.as_slice(), [Value::Str(text)] if text == "b"))
        );
    }

    #[test]
    fn io_get_reports_eof_as_input_error_result() {
        let mut vm = test_vm().with_stdin_input("");
        let value = call_builtin(
            &mut vm,
            builtin_id("io_get"),
            vec![Value::Str(String::new())],
        )
        .expect("io_get should convert eof into Err");

        match value {
            Value::Tagged { tag: 1, fields } => match fields.as_slice() {
                [Value::Error(rich)] => {
                    assert_eq!(rich.kind, "InputError");
                    assert_eq!(rich.message, "end of input");
                }
                other => panic!("expected Err(InputError), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn file_read_write_append_exists_and_delete_work() {
        let dir = sandbox_dir("builtin-file");
        let path = dir.join("sample.txt");
        let path_text = path.to_string_lossy().into_owned();
        let mut vm = test_vm();

        let write = call_builtin(
            &mut vm,
            builtin_id("file_write"),
            vec![Value::Str(path_text.clone()), Value::Str("alpha".into())],
        )
        .expect("file_write should run");
        assert_eq!(write, ok_result(Value::Unit));

        let read = call_builtin(
            &mut vm,
            builtin_id("file_read"),
            vec![Value::Str(path_text.clone())],
        )
        .expect("file_read should run");
        assert_eq!(read, ok_result(Value::Str("alpha".into())));

        let append = call_builtin(
            &mut vm,
            builtin_id("file_append"),
            vec![Value::Str(path_text.clone()), Value::Str("beta".into())],
        )
        .expect("file_append should run");
        assert_eq!(append, ok_result(Value::Unit));
        assert_eq!(
            fs::read_to_string(&path).expect("appended file should be readable"),
            "alphabeta"
        );

        let exists = call_builtin(
            &mut vm,
            builtin_id("file_exists"),
            vec![Value::Str(path_text.clone())],
        )
        .expect("file_exists should run");
        assert_eq!(exists, Value::Bool(true));

        let delete = call_builtin(
            &mut vm,
            builtin_id("file_delete"),
            vec![Value::Str(path_text.clone())],
        )
        .expect("file_delete should run");
        assert_eq!(delete, ok_result(Value::Unit));
        assert!(!path.exists(), "file_delete should remove the target file");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn filesystem_path_join_name_parent_and_extension_work() {
        let dir = sandbox_dir("builtin-filesystem-paths");
        let base = dir.to_string_lossy().into_owned();
        let mut vm = filesystem_vm();

        let base_path = ok_payload(
            call_builtin(
                &mut vm,
                builtin_id("filesystem_path"),
                vec![Value::Str(base.clone())],
            )
            .expect("filesystem_path should run"),
        );
        let joined = ok_payload(
            call_builtin(
                &mut vm,
                builtin_id("filesystem_join"),
                vec![base_path.clone(), Value::Str("sample.srt".into())],
            )
            .expect("filesystem_join should run"),
        );

        assert_eq!(
            ok_payload(
                call_builtin(&mut vm, builtin_id("filesystem_name"), vec![joined.clone()],)
                    .expect("filesystem_name should run"),
            ),
            Value::Str("sample.srt".into())
        );
        assert!(matches!(
            call_builtin(
                &mut vm,
                builtin_id("filesystem_extension"),
                vec![joined.clone()],
            )
            .expect("filesystem_extension should run"),
            Value::Tagged { tag: 21, fields } if matches!(fields.as_slice(), [Value::Int(_), Value::Str(ext)] if ext == "srt")
        ));
        assert_eq!(
            ok_payload(
                call_builtin(&mut vm, builtin_id("filesystem_parent"), vec![joined])
                    .expect("filesystem_parent should run")
            ),
            base_path
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_ls_returns_err_when_read_dir_fails() {
        let dir = sandbox_dir("builtin-filesystem-ls-read-dir-error");
        let blocked = dir.join("blocked");
        fs::create_dir_all(&blocked).expect("blocked dir should be creatable");
        let original_permissions = fs::metadata(&blocked)
            .expect("blocked metadata should be readable")
            .permissions();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))
            .expect("permissions should be removable");

        let mut vm = filesystem_vm();
        let result = call_builtin(
            &mut vm,
            builtin_id("filesystem_ls"),
            vec![Value::Tagged {
                tag: 30,
                fields: vec![Value::Str(blocked.to_string_lossy().into_owned())],
            }],
        )
        .expect("filesystem_ls should return a Result value");

        fs::set_permissions(&blocked, original_permissions)
            .expect("permissions should be restored");
        let _ = fs::remove_dir_all(dir);

        assert!(matches!(
            err_kind(&result),
            "FileSystemPermissionDenied" | "FileSystemIoError"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_tree_depth_returns_err_when_child_read_dir_fails() {
        let dir = sandbox_dir("builtin-filesystem-tree-read-dir-error");
        let blocked = dir.join("blocked");
        fs::create_dir_all(&blocked).expect("blocked dir should be creatable");
        let original_permissions = fs::metadata(&blocked)
            .expect("blocked metadata should be readable")
            .permissions();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))
            .expect("permissions should be removable");

        let mut vm = filesystem_vm();
        let result = call_builtin(
            &mut vm,
            builtin_id("filesystem_tree_depth"),
            vec![
                Value::Tagged {
                    tag: 30,
                    fields: vec![Value::Str(dir.to_string_lossy().into_owned())],
                },
                Value::Int(2.into()),
            ],
        )
        .expect("filesystem_tree_depth should return a Result value");

        fs::set_permissions(&blocked, original_permissions)
            .expect("permissions should be restored");
        let _ = fs::remove_dir_all(dir);

        assert!(matches!(
            err_kind(&result),
            "FileSystemPermissionDenied" | "FileSystemIoError"
        ));
    }

    #[test]
    fn shell_cd_canonicalize_error_maps_to_shell_io_error_without_mutating_cwd() {
        let vm = filesystem_vm();
        let original = vm.cwd().to_path_buf();
        let err = super::canonicalize_shell_cwd(
            &vm,
            &original.join("missing-after-dir-check"),
            "missing-after-dir-check",
        )
        .expect_err("missing canonical path should map to ShellIoError");

        assert_eq!(err_kind(&err), "ShellIoError");
        assert_eq!(vm.cwd(), original.as_path());
    }

    #[test]
    fn shell_exec_returns_ok_for_launched_nonzero_exit() {
        let mut vm = filesystem_vm();
        let result = ok_payload(
            call_builtin(
                &mut vm,
                builtin_id("shell_exec"),
                vec![
                    Value::Str("sh".into()),
                    Value::List(ListHandle::from_items(vec![
                        Value::Str("-c".into()),
                        Value::Str("printf out; printf err >&2; exit 7".into()),
                    ])),
                ],
            )
            .expect("shell_exec should run"),
        );

        let entry = vm
            .type_registry()
            .lookup_by_name("CommandResult")
            .expect("CommandResult should be registered");
        assert!(matches!(
            result,
            Value::Tagged { tag, fields }
                if tag == entry.tag
                    && matches!(fields.as_slice(), [
                        Value::Str(command),
                        Value::List(_),
                        Value::Int(exit_code),
                        Value::Str(stdout),
                        Value::Str(stderr),
                    ] if command == "sh" && exit_code == &int(7) && stdout == "out" && stderr == "err")
        ));
    }

    #[test]
    fn file_with_open_closes_handle_after_ok_callback() {
        let dir = sandbox_dir("builtin-file-with-open-ok");
        let path = dir.join("ok.txt");
        let path_text = path.to_string_lossy().into_owned();
        let mut registry = TypeRegistry::new();
        registry.register(TypeEntry {
            tag: 10,
            name: "Write".into(),
            kind: TypeKind::EnumVariant,
            field_names: vec![],
            private_flags: vec![],
        });
        let mut vm = VM::new(Bytecode {
            opcodes: vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            constants: vec![Constant::Tag(0), Constant::Unit],
            type_registry: registry,
            functions: vec![FunctionEntry {
                fun_idx: 0,
                entry_pc: 0,
                num_locals: 1,
                arity: 1,
                qualified_name: Some("Test::ok".into()),
                signature: None,
                end_pc: 0,
                span_start: 0,
                span_end: 0,
                flags: Default::default(),
            }],
            ..Bytecode::default()
        })
        .with_error_capture();

        let result = call_builtin(
            &mut vm,
            builtin_id("file_with_open"),
            vec![
                Value::Str(path_text.clone()),
                Value::Tagged {
                    tag: 10,
                    fields: Vec::new(),
                },
                Value::Callable(Callable {
                    target: CallableTarget::Function(0),
                    lexical_captures: Vec::new(),
                    metadata: CallableMetadata::default(),
                }),
            ],
        )
        .expect("file_with_open should run");
        assert_eq!(result, ok_result(Value::Unit));
        assert_eq!(vm.open_file_count(), 0, "with_open should close handles");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn file_with_open_closes_handle_after_err_callback() {
        let dir = sandbox_dir("builtin-file-with-open-err");
        let path = dir.join("err.txt");
        let path_text = path.to_string_lossy().into_owned();
        let mut registry = TypeRegistry::new();
        registry.register(TypeEntry {
            tag: 11,
            name: "Read".into(),
            kind: TypeKind::EnumVariant,
            field_names: vec![],
            private_flags: vec![],
        });
        let mut vm = VM::new(Bytecode {
            opcodes: vec![
                Opcode::LoadConst(0),
                Opcode::MakeErrorLiteral {
                    kind_const_idx: 1,
                    message_const_idx: 2,
                },
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            constants: vec![
                Constant::Tag(1),
                Constant::Str("FileIoError".into()),
                Constant::Str("boom".into()),
            ],
            type_registry: registry,
            functions: vec![FunctionEntry {
                fun_idx: 0,
                entry_pc: 0,
                num_locals: 1,
                arity: 1,
                qualified_name: Some("Test::err".into()),
                signature: None,
                end_pc: 0,
                span_start: 0,
                span_end: 0,
                flags: Default::default(),
            }],
            ..Bytecode::default()
        })
        .with_error_capture();

        let result = call_builtin(
            &mut vm,
            builtin_id("file_with_open"),
            vec![
                Value::Str(path_text.clone()),
                Value::Tagged {
                    tag: 11,
                    fields: Vec::new(),
                },
                Value::Callable(Callable {
                    target: CallableTarget::Function(0),
                    lexical_captures: Vec::new(),
                    metadata: CallableMetadata::default(),
                }),
            ],
        )
        .expect("file_with_open should run");
        match result {
            Value::Tagged { tag: 1, .. } => {}
            other => panic!("expected Err result from callback, got {other:?}"),
        }
        assert_eq!(
            vm.open_file_count(),
            0,
            "with_open should close handles on Err"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn map_err_replaces_error_without_preserving_previous_cause() {
        let mut vm = test_vm();
        let builtin_id = builtin_id_by_name("map_err").expect("map_err metadata");
        let value = call_builtin(
            &mut vm,
            builtin_id,
            vec![
                err_result_from_rich_error(sample_error("Lower", "lower")),
                sample_error_value("Higher", "higher"),
            ],
        )
        .expect("map_err should succeed");

        match value {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "Higher");
                    assert_eq!(rich.message, "higher");
                    assert!(rich.cause.is_none(), "map_err must replace cause chain");
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn cause_wraps_existing_error_under_new_domain_error() {
        let mut vm = test_vm();
        let builtin_id = builtin_id_by_name("cause").expect("cause metadata");
        let value = call_builtin(
            &mut vm,
            builtin_id,
            vec![
                err_result_from_rich_error(sample_error("Lower", "lower")),
                sample_error_value("Higher", "higher"),
            ],
        )
        .expect("cause should succeed");

        match value {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "Higher");
                    let cause = rich.cause.as_deref().expect("expected cause");
                    assert_eq!(cause.kind, "Lower");
                    assert_eq!(cause.message, "lower");
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn chain_appends_left_error_to_tail_of_right_error_chain() {
        let mut vm = test_vm();
        let builtin_id = builtin_id_by_name("chain").expect("chain metadata");
        let mut right = sample_error("Higher", "higher");
        right.append_cause_tail(sample_error("Middle", "middle"));
        let value = call_builtin(
            &mut vm,
            builtin_id,
            vec![
                err_result_from_rich_error(sample_error("Lower", "lower")),
                err_result_from_rich_error(right),
            ],
        )
        .expect("chain should succeed");

        match value {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(
                        rich.to_display_string(),
                        "Higher(\"higher\")\n|_ Middle(\"middle\")\n   |_ Lower(\"lower\")"
                    );
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn set_exit_code_rejects_out_of_range_values() {
        let mut vm = test_vm();
        let huge = Value::Int(int(999999999999999999_i128));
        let err = call_builtin(&mut vm, 6, vec![huge]).expect_err("must reject large exit codes");
        assert!(err.message.contains("set_exit_code out of range for i32"));
    }

    #[test]
    fn project_args_returns_vm_cli_arguments() {
        let mut vm = test_vm();
        vm.set_cli_args(vec![
            "--mode".to_string(),
            "score".to_string(),
            "123m456p789s11z".to_string(),
        ]);

        let value = call_builtin(&mut vm, builtin_id("project_args"), vec![])
            .expect("project_args should succeed");

        match value {
            Value::List(list) => {
                let actual = list
                    .iter()
                    .map(|value| match value {
                        Value::Str(text) => text,
                        other => panic!("expected String value, got {:?}", other),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual,
                    vec![
                        "--mode".to_string(),
                        "score".to_string(),
                        "123m456p789s11z".to_string()
                    ]
                );
            }
            other => panic!("expected List<String>, got {:?}", other),
        }
    }

    #[test]
    fn eprint_writes_rich_errors_to_captured_stderr() {
        let mut vm = test_vm();
        let value = Value::Error(Box::new(sindr::runtime::RichError {
            kind: "Boom".into(),
            message: "broken".into(),
            location: sindr::runtime::Location {
                file: "main.srt".into(),
                func: "Boom".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 4,
            },
            diagnostic: None,
            cause: Some(Box::new(sindr::runtime::RichError {
                kind: "Root".into(),
                message: "root cause".into(),
                location: sindr::runtime::Location {
                    file: "main.srt".into(),
                    func: "Boom".into(),
                    line: 1,
                    column: 1,
                    span_start: 0,
                    span_end: 4,
                },
                diagnostic: None,
                cause: None,
            })),
        }));
        let result = call_builtin(&mut vm, 5, vec![value]).expect("eprint should succeed");
        assert_eq!(result, Value::Unit);
        assert_eq!(
            vm.error_output.as_deref(),
            Some(
                &[
                    "Error: Boom: broken".to_string(),
                    "Caused by: Root: root cause".to_string(),
                ][..]
            )
        );
    }

    #[test]
    fn eprint_falls_back_to_inspect_for_non_error_values() {
        let mut vm = test_vm();
        let result =
            call_builtin(&mut vm, 5, vec![Value::Int(int(42))]).expect("eprint should succeed");
        assert_eq!(result, Value::Unit);
        assert_eq!(vm.error_output.as_deref(), Some(&["42".to_string()][..]));
    }

    #[test]
    fn inspect_formats_bare_builtin_callable_with_doc_metadata() {
        let vm = VM::new(Bytecode {
            docs: vec![DocEntry {
                qualified_name: "Int::shr".into(),
                kind: DocKind::Function,
                module_path: "Int".into(),
                signature: Some(
                    "shr(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>".into(),
                ),
                doc: String::new(),
            }],
            ..Bytecode::default()
        });
        let value = Value::Callable(Callable {
            target: CallableTarget::Builtin(8),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata {
                origin: CallableOrigin::Capture,
                module: Some("Int".into()),
                name: Some("shr".into()),
                full_signature: Some(
                    "shr(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>".into(),
                ),
                applied_args: 0,
            },
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: Int, name: shr, sig: shr(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>)"
        );
    }

    #[test]
    fn inspect_formats_bare_function_callable_with_embedded_signature() {
        let vm = VM::new(Bytecode {
            functions: vec![FunctionEntry {
                fun_idx: 0,
                entry_pc: 0,
                num_locals: 0,
                arity: 2,
                qualified_name: Some("Main::add".into()),
                signature: Some("add(x: Int, y: Int) -> Int".into()),
                end_pc: 0,
                span_start: 0,
                span_end: 0,
                flags: Default::default(),
            }],
            ..Bytecode::default()
        });
        let value = Value::Callable(Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata {
                origin: CallableOrigin::Capture,
                module: Some("Main".into()),
                name: Some("add".into()),
                full_signature: Some("add(x: Int, y: Int) -> Int".into()),
                applied_args: 0,
            },
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: Main, name: add, sig: add(x: Int, y: Int) -> Int)"
        );
    }

    #[test]
    fn inspect_formats_local_function_callable_with_local_module_marker() {
        let vm = VM::new(Bytecode {
            functions: vec![FunctionEntry {
                fun_idx: 0,
                entry_pc: 0,
                num_locals: 0,
                arity: 2,
                qualified_name: Some("add".into()),
                signature: Some("add(x: Int, y: Int) -> Int".into()),
                end_pc: 0,
                span_start: 0,
                span_end: 0,
                flags: Default::default(),
            }],
            ..Bytecode::default()
        });
        let value = Value::Callable(Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata {
                origin: CallableOrigin::Capture,
                module: Some("<local>".into()),
                name: Some("add".into()),
                full_signature: Some("add(x: Int, y: Int) -> Int".into()),
                applied_args: 0,
            },
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: <local>, name: add, sig: add(x: Int, y: Int) -> Int)"
        );
    }

    #[test]
    fn inspect_formats_closure_with_type_style_signature() {
        let vm = VM::new(Bytecode {
            functions: vec![FunctionEntry {
                fun_idx: 0,
                entry_pc: 0,
                num_locals: 0,
                arity: 2,
                qualified_name: None,
                signature: Some("(Int, Int -> Int)".into()),
                end_pc: 0,
                span_start: 0,
                span_end: 0,
                flags: FunctionFlags {
                    closure: true,
                    ..Default::default()
                },
            }],
            ..Bytecode::default()
        });
        let value = Value::Callable(Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata {
                origin: CallableOrigin::Closure,
                module: None,
                name: None,
                full_signature: Some("(Int, Int -> Int)".into()),
                applied_args: 0,
            },
        });

        assert_eq!(inspect_value(&vm, &value), "Closure(Int, Int -> Int)");
    }

    #[test]
    fn inspect_formats_partial_capture_with_remaining_signature() {
        let vm = VM::new(Bytecode::default());
        let value = Value::Callable(Callable {
            target: CallableTarget::Function(9),
            lexical_captures: vec![Value::Unit],
            metadata: CallableMetadata {
                origin: CallableOrigin::Capture,
                module: Some("Add".into()),
                name: Some("add".into()),
                full_signature: Some("add(value: Int, rhs: Int) -> Int".into()),
                applied_args: 1,
            },
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: Add, name: add, sig: (Int -> Int))"
        );
    }

    #[test]
    fn inspect_formats_zero_arg_partial_capture_as_thunk_type() {
        let vm = VM::new(Bytecode::default());
        let value = Value::Callable(Callable {
            target: CallableTarget::Function(9),
            lexical_captures: vec![Value::Unit, Value::Unit],
            metadata: CallableMetadata {
                origin: CallableOrigin::Capture,
                module: Some("Main".into()),
                name: Some("ready".into()),
                full_signature: Some("ready(left: Int, right: Int) -> String".into()),
                applied_args: 2,
            },
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: Main, name: ready, sig: (-> String))"
        );
    }

    #[test]
    fn inspect_keeps_fallback_callable_display_for_unknown_lexical_captures() {
        let vm = VM::new(Bytecode::default());
        let value = Value::Callable(Callable {
            target: CallableTarget::Builtin(8),
            lexical_captures: vec![Value::Int(int(1))],
            metadata: CallableMetadata::default(),
        });

        assert_eq!(inspect_value(&vm, &value), "<builtin:8>");
    }
}
