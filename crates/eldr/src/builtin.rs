use crate::error::RuntimeError;
use crate::value::Value;
use crate::vm::VM;
use regex::Regex;
use sindr::builtin::{builtin_meta_by_id, BUILTIN_METAS};
use sindr::ir::DocKind;
use sindr::primitives::{int, SurtrInt, ToPrimitive, Zero};
use sindr::runtime::{
    Callable, CallableTarget, HashMapHandle, ListHandle, Location, RegexCapturesHandle,
    RegexHandle, RegexMatchHandle, RichError,
};
use std::collections::HashMap;

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
        func: builtin_lens_view,
    },
    BuiltinImpl {
        name: "compose",
        func: builtin_lens_compose,
    },
    BuiltinImpl {
        name: "set",
        func: builtin_lens_set,
    },
    BuiltinImpl {
        name: "over",
        func: builtin_lens_over,
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
        name: "replace",
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
    if args.len() != usize::from(meta.arity) {
        return Err(RuntimeError::new(format!(
            "builtin {} arity mismatch: expected {}, got {}",
            meta.name,
            meta.arity,
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
    Ok(Value::Str(rich.kind))
}

fn builtin_error_message(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let rich = decode_error_arg(&args[0], "message", "err")?;
    Ok(Value::Str(rich.visible_message().to_string()))
}

fn builtin_error_format(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let rich = decode_error_arg(&args[0], "format", "err")?;
    Ok(Value::Str(rich.to_eprint_lines().join("\n")))
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
            if *b == 0.0 {
                Ok(err_result(vm, "ZeroDivisionError", "division by zero"))
            } else {
                Ok(ok_result(Value::Float(a / b)))
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
    Ok(Value::Float(left + right))
}

fn builtin_operator_float_sub(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_sub")?;
    Ok(Value::Float(left - right))
}

fn builtin_operator_float_mul(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_mul")?;
    Ok(Value::Float(left * right))
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
    Ok(Value::Bool(left == right))
}

fn builtin_operator_float_neq(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_neq")?;
    Ok(Value::Bool(left != right))
}

fn builtin_operator_float_lt(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_lt")?;
    Ok(Value::Bool(left < right))
}

fn builtin_operator_float_lte(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_lte")?;
    Ok(Value::Bool(left <= right))
}

fn builtin_operator_float_gt(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_gt")?;
    Ok(Value::Bool(left > right))
}

fn builtin_operator_float_gte(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (left, right) = expect_float_pair(&args, "__operator_float_gte")?;
    Ok(Value::Bool(left >= right))
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

fn builtin_lens_view(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Lens::view should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_lens_compose(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Lens::compose should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_lens_set(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Lens::set should be lowered in Forge (runtime builtin call indicates lowering bug)",
    ))
}

fn builtin_lens_over(_vm: &mut VM, _args: Vec<Value>) -> Result<Value, RuntimeError> {
    Err(RuntimeError::new(
        "Lens::over should be lowered in Forge (runtime builtin call indicates lowering bug)",
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
                return "HashMap()".to_string();
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
            format!("HashMap({inner})")
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
        return match entry.kind {
            sindr::runtime::TypeKind::Struct => {
                let pairs = entry
                    .field_names
                    .iter()
                    .zip(fields.iter())
                    .map(|(name, val)| format!("{name}: {}", inspect_non_callable_value(vm, val)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {} }}", entry.name, pairs)
            }
            sindr::runtime::TypeKind::Record => {
                let pairs = entry
                    .field_names
                    .iter()
                    .zip(fields.iter())
                    .map(|(name, val)| format!("{name}: {}", inspect_non_callable_value(vm, val)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({pairs})", entry.name)
            }
            sindr::runtime::TypeKind::EnumVariant => {
                let payload = fields
                    .iter()
                    .skip(1)
                    .map(|val| inspect_non_callable_value(vm, val))
                    .collect::<Vec<_>>()
                    .join(", ");
                if payload.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{}({payload})", entry.name)
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

fn quote_surtr_string_literal(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn inspect_callable(vm: &VM, callable: &Callable) -> Option<String> {
    if !callable.lexical_captures.is_empty() {
        return None;
    }

    match callable.target {
        CallableTarget::Builtin(id) => {
            let meta = builtin_meta_by_id(id)?;
            let doc = vm.bytecode().docs.iter().rev().find(|doc| {
                matches!(doc.kind, DocKind::Function)
                    && doc.qualified_name.rsplit("::").next() == Some(meta.name)
            })?;
            let signature = doc.signature.as_deref()?;
            Some(format!(
                "FnCapture(module: {}, name: {}, signature: {})",
                doc.module_path, meta.name, signature
            ))
        }
        CallableTarget::Function(fun_idx) => {
            let entry = vm.bytecode().functions.get(fun_idx as usize)?;
            let qualified_name = entry.qualified_name.as_deref()?;
            let signature = entry.signature.as_deref().or_else(|| {
                vm.bytecode()
                    .docs
                    .iter()
                    .rev()
                    .find(|doc| {
                        matches!(doc.kind, DocKind::Function)
                            && doc.qualified_name == qualified_name
                    })
                    .and_then(|doc| doc.signature.as_deref())
            })?;
            let (module, name) = split_qualified_name(qualified_name);
            Some(format!(
                "FnCapture(module: {}, name: {}, signature: {})",
                module, name, signature
            ))
        }
    }
}

fn split_qualified_name(qualified_name: &str) -> (&str, &str) {
    match qualified_name.rsplit_once("::") {
        Some((module, name)) if !module.is_empty() => (module, name),
        _ => ("<local>", qualified_name),
    }
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
    let Some(entry) = vm.type_registry().lookup(*tag) else {
        return Err(RuntimeError::new(format!(
            "unknown StringEncoding tag: {}",
            tag
        )));
    };
    match entry
        .name
        .rsplit("::")
        .next()
        .unwrap_or(entry.name.as_str())
    {
        "Utf8" => Ok(StringEncodingMode::Utf8),
        "Ascii" => Ok(StringEncodingMode::Ascii),
        _other => Err(RuntimeError::new(format!(
            "expected StringEncoding variant, got {}",
            entry.name
        ))),
    }
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

fn ok_result(value: Value) -> Value {
    Value::Tagged {
        tag: 0,
        fields: vec![value],
    }
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
    use super::{call_builtin, err_result_from_rich_error, inspect_value, BUILTIN_IMPLS};
    use crate::vm::VM;
    use sindr::builtin::{builtin_id_by_name, builtin_meta_by_id, builtin_meta_by_name};
    use sindr::ir::{Bytecode, DocEntry, DocKind, FunctionEntry};
    use sindr::primitives::int;
    use sindr::runtime::{
        Callable, CallableTarget, HashMapHandle, ListHandle, Location, RichError, TypeEntry,
        TypeKind, TypeRegistry, Value,
    };

    fn test_vm() -> VM {
        VM::new(Bytecode {
            type_registry: TypeRegistry::new(),
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

    #[test]
    fn builtin_impl_order_matches_metadata() {
        for (id, builtin) in BUILTIN_IMPLS.iter().enumerate() {
            let meta = builtin_meta_by_id(id as u16).expect("builtin metadata by id");
            assert_eq!(builtin.name, meta.name, "builtin impl mismatch at id {id}");
        }
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
            include_str!("../../../lib/int.srt"),
            include_str!("../../../lib/list.srt"),
            include_str!("../../../lib/generator.srt"),
            include_str!("../../../lib/hash_map.srt"),
            include_str!("../../../lib/result.srt"),
            include_str!("../../../lib/lens.srt"),
            include_str!("../../../lib/string.srt"),
            include_str!("../../../lib/regex.srt"),
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

        // For each @@builtin annotation, find the associated def signature.
        // Annotation order is flexible:
        // - `@@builtin def ...` can appear inline
        // - `@@builtin` can appear on its own line before a following `def`
        //
        // We intentionally scan raw source text here instead of depending on
        // parser lowering details, because this test is meant to guard the
        // human-maintained std-module declaration layer against drift from
        // `BUILTIN_METAS`.
        let mut entries: Vec<(String, u8, String)> = Vec::new();
        let mut i = 0;
        while i < all_lines.len() {
            let line = all_lines[i];
            if let Some(rest) = line.strip_prefix("@@builtin def ") {
                // Inline form: @@builtin def name(params) -> ret
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
            } else if line == "@@builtin" {
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
        // matching `@@builtin def` surface declaration.
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
            "to_string is trait-backed and should not be declared via @@builtin def"
        );
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
    fn inspect_formats_hash_map_named_style() {
        let vm = test_vm();
        let value = Value::HashMap(HashMapHandle::from_entries(vec![
            ("line\nfeed".into(), Value::Int(int(1))),
            ("path\\to".into(), Value::Int(int(2))),
        ]));
        assert_eq!(
            inspect_value(&vm, &value),
            "HashMap(\"line\\nfeed\" => 1, \"path\\\\to\" => 2)"
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
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: Int, name: shr, signature: shr(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>)"
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
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: Main, name: add, signature: add(x: Int, y: Int) -> Int)"
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
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: <local>, name: add, signature: add(x: Int, y: Int) -> Int)"
        );
    }

    #[test]
    fn inspect_keeps_fallback_callable_display_for_lexical_captures() {
        let vm = VM::new(Bytecode::default());
        let value = Value::Callable(Callable {
            target: CallableTarget::Builtin(8),
            lexical_captures: vec![Value::Int(int(1))],
        });

        assert_eq!(inspect_value(&vm, &value), "<builtin:8>");
    }
}
