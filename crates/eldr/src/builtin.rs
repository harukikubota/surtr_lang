use crate::error::RuntimeError;
use crate::value::Value;
use crate::vm::VM;
use sindr::builtin::{builtin_meta_by_id, BUILTIN_METAS};
use sindr::ir::DocKind;
use sindr::primitives::{int, SurtrInt, ToPrimitive, Zero};
use sindr::runtime::{Callable, CallableTarget, ListHandle, Location, RichError};

/// Function pointer type for built-in implementations.
pub type BuiltinFn = fn(&mut VM, Vec<Value>) -> Result<Value, RuntimeError>;

struct BuiltinImpl {
    func: BuiltinFn,
}

// Eldr keeps implementation pointers only. Metadata lives in sindr::builtin.
const BUILTIN_IMPLS: &[BuiltinImpl] = &[
    BuiltinImpl {
        func: builtin_print,
    },
    BuiltinImpl {
        func: builtin_to_string,
    },
    BuiltinImpl {
        func: builtin_inspect,
    },
    BuiltinImpl {
        func: builtin_safe_div,
    },
    BuiltinImpl {
        func: builtin_safe_mod,
    },
    BuiltinImpl {
        func: builtin_eprint,
    },
    BuiltinImpl {
        func: builtin_set_exit_code,
    },
    BuiltinImpl { func: builtin_shl },
    BuiltinImpl { func: builtin_shr },
    BuiltinImpl {
        func: builtin_list_len,
    },
    BuiltinImpl {
        func: builtin_bit_and,
    },
    BuiltinImpl {
        func: builtin_bit_or,
    },
    BuiltinImpl {
        func: builtin_bit_xor,
    },
    BuiltinImpl {
        func: builtin_bit_not,
    },
    BuiltinImpl {
        func: builtin_test_bit,
    },
    BuiltinImpl {
        func: builtin_set_bit,
    },
    BuiltinImpl {
        func: builtin_clear_bit,
    },
    BuiltinImpl {
        func: builtin_toggle_bit,
    },
    BuiltinImpl {
        func: builtin_codepoints,
    },
    BuiltinImpl {
        func: builtin_from_codepoints,
    },
    BuiltinImpl {
        func: builtin_result_map_err,
    },
    BuiltinImpl {
        func: builtin_result_cause,
    },
    BuiltinImpl {
        func: builtin_result_chain,
    },
    BuiltinImpl {
        func: builtin_test_push,
    },
    BuiltinImpl {
        func: builtin_test_pop,
    },
    BuiltinImpl {
        func: builtin_test_pass,
    },
    BuiltinImpl {
        func: builtin_test_fail,
    },
    BuiltinImpl {
        func: builtin_test_fail_current,
    },
    BuiltinImpl {
        func: builtin_list_group_count,
    },
    BuiltinImpl {
        func: builtin_list_zip,
    },
    BuiltinImpl {
        func: builtin_lens_view,
    },
    BuiltinImpl {
        func: builtin_lens_compose,
    },
    BuiltinImpl {
        func: builtin_lens_set,
    },
    BuiltinImpl {
        func: builtin_lens_over,
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

    let func = BUILTIN_IMPLS
        .get(builtin_id as usize)
        .ok_or_else(|| {
            RuntimeError::new(format!(
                "Missing builtin implementation for id {}",
                builtin_id
            ))
        })?
        .func;

    func(vm, args)
}

fn builtin_print(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let s = match &args[0] {
        Value::Str(s) => s.clone(),
        other => inspect_value(vm, other),
    };
    match &mut vm.output {
        Some(buf) => buf.push(s),
        None => println!("{}", s),
    }
    Ok(Value::Unit)
}

fn builtin_to_string(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    Ok(Value::Str(inspect_value(vm, &args[0])))
}

fn builtin_inspect(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    Ok(Value::Str(inspect_value(vm, &args[0])))
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

fn builtin_eprint(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Error(rich) => {
            if let Some(buf) = vm.error_output.as_mut() {
                buf.extend(rich.to_eprint_lines());
            } else if let (Some(source), Some(file)) = (vm.source(), vm.source_file()) {
                use ariadne::{Color, Label, Report, ReportKind, Source};

                let start = rich.location.span_start as usize;
                let end = rich.location.span_end as usize;
                if let Err(err) = Report::build(ReportKind::Error, (file, start..end))
                    .with_message(rich.kind.clone())
                    .with_label(
                        Label::new((file, start..end))
                            .with_message(rich.message.clone())
                            .with_color(Color::Red),
                    )
                    .finish()
                    .eprint((file, Source::from(source)))
                {
                    return Err(RuntimeError::new(format!(
                        "Failed to render rich error report: {}",
                        err
                    )));
                }
                for line in rich.to_eprint_lines().into_iter().skip(1) {
                    eprintln!("{}", line);
                }
            } else {
                for line in rich.to_eprint_lines() {
                    eprintln!("{}", line);
                }
            }
        }
        other => {
            let s = inspect_value(vm, other);
            match &mut vm.error_output {
                Some(buf) => buf.push(s),
                None => eprintln!("{}", s),
            }
        }
    }
    Ok(Value::Unit)
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

pub fn inspect_value(vm: &VM, value: &Value) -> String {
    if let Value::Callable(callable) = value {
        if let Some(display) = inspect_callable(vm, callable) {
            return display;
        }
    }

    value.to_display_string(vm.type_registry())
}

fn inspect_callable(vm: &VM, callable: &Callable) -> Option<String> {
    if !callable.lexical_captures.is_empty() || !callable.partial_args.is_empty() {
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
        cause: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{call_builtin, err_result_from_rich_error, inspect_value};
    use crate::vm::VM;
    use sindr::builtin::builtin_meta_by_name;
    use sindr::ir::{Bytecode, DocEntry, DocKind, FunctionEntry};
    use sindr::primitives::int;
    use sindr::runtime::{
        Callable, CallableTarget, ListHandle, Location, RichError, TypeEntry, TypeKind,
        TypeRegistry, Value,
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
            cause: None,
        }
    }

    fn sample_error_value(kind: &str, message: &str) -> Value {
        Value::Error(Box::new(sample_error(kind, message)))
    }

    /// Parse the `name(params) -> ret_ty` portion of a `def` declaration.
    fn parse_decl_name(def_rest: &str) -> &str {
        def_rest
            .split_once('(')
            .map(|(name, _)| name.trim())
            .expect("def declaration must include params")
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
            include_str!("../../../lib/result.srt"),
            include_str!("../../../lib/lens.srt"),
            include_str!("../../../lib/string.srt"),
        ];

        // Collect all lines across the std-module files that currently declare
        // builtin value surfaces. Bootstrap intentionally stays almost empty,
        // Kernel owns the cross-cutting builtins, Int currently carries both
        // arithmetic-result builtins and bit-shift helpers, List declares
        // the O(1) length helper, Result carries result/error helpers, and
        // String carries encoding helpers.
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
                if builtin_meta_by_name(parse_decl_name(rest)).is_some() {
                    let entry = parse_def_signature(rest);
                    entries.push(entry);
                }
            } else if line == "@@builtin" {
                // Standalone form: find the next `def` line.
                let mut j = i + 1;
                while j < all_lines.len() {
                    let next = all_lines[j];
                    if let Some(rest) = next.strip_prefix("def ") {
                        if builtin_meta_by_name(parse_decl_name(rest)).is_some() {
                            let entry = parse_def_signature(rest);
                            entries.push(entry);
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
    fn safe_mod_returns_zero_division_error_result() {
        let mut vm = test_vm();
        let value = call_builtin(&mut vm, 4, vec![Value::Int(int(10)), Value::Int(int(0))])
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
        let err = call_builtin(&mut vm, 4, vec![Value::Bool(true), Value::Int(int(1))])
            .expect_err("safe_mod must reject non-int inputs");
        assert!(err.message.contains("safe_mod expects (Int, Int)"));
    }

    #[test]
    fn shl_returns_result_and_negative_shift_error() {
        let mut vm = test_vm();
        let ok = call_builtin(&mut vm, 7, vec![Value::Int(int(2)), Value::Int(int(3))])
            .expect("shl should return Result");
        match ok {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(16)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let err = call_builtin(&mut vm, 7, vec![Value::Int(int(2)), Value::Int(int(-1))])
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
        let ok = call_builtin(&mut vm, 8, vec![Value::Int(int(16)), Value::Int(int(2))])
            .expect("shr should return Result");
        match ok {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(4)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let err = call_builtin(&mut vm, 8, vec![Value::Int(int(2)), Value::Int(int(-1))])
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

        let bit_and = call_builtin(&mut vm, 10, vec![Value::Int(int(6)), Value::Int(int(3))])
            .expect("bit_and should succeed");
        assert_eq!(bit_and, Value::Int(int(2)));

        let bit_or = call_builtin(&mut vm, 11, vec![Value::Int(int(6)), Value::Int(int(3))])
            .expect("bit_or should succeed");
        assert_eq!(bit_or, Value::Int(int(7)));

        let bit_xor = call_builtin(&mut vm, 12, vec![Value::Int(int(6)), Value::Int(int(3))])
            .expect("bit_xor should succeed");
        assert_eq!(bit_xor, Value::Int(int(5)));

        let bit_not =
            call_builtin(&mut vm, 13, vec![Value::Int(int(6))]).expect("bit_not should succeed");
        assert_eq!(bit_not, Value::Int(int(-7)));
    }

    #[test]
    fn bit_index_helpers_return_results_and_negative_index_errors() {
        let mut vm = test_vm();

        let tested = call_builtin(&mut vm, 14, vec![Value::Int(int(5)), Value::Int(int(2))])
            .expect("test_bit should return Result");
        match tested {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Bool(true))));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let negative = call_builtin(&mut vm, 14, vec![Value::Int(int(5)), Value::Int(int(-1))])
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

        let set = call_builtin(&mut vm, 15, vec![Value::Int(int(0)), Value::Int(int(1))])
            .expect("set_bit should return Result");
        match set {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(2)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let cleared = call_builtin(&mut vm, 16, vec![Value::Int(int(7)), Value::Int(int(1))])
            .expect("clear_bit should return Result");
        match cleared {
            Value::Tagged { tag: 0, fields } => {
                assert!(matches!(fields.first(), Some(Value::Int(value)) if *value == int(5)));
            }
            other => panic!("expected Ok result, got {:?}", other),
        }

        let toggled = call_builtin(&mut vm, 17, vec![Value::Int(int(5)), Value::Int(int(0))])
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
            18,
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
            19,
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
            28,
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
            29,
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
    fn map_err_replaces_error_without_preserving_previous_cause() {
        let mut vm = test_vm();
        let builtin_id = builtin_meta_by_name("map_err")
            .expect("map_err metadata")
            .builtin_id;
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
        let builtin_id = builtin_meta_by_name("cause")
            .expect("cause metadata")
            .builtin_id;
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
        let builtin_id = builtin_meta_by_name("chain")
            .expect("chain metadata")
            .builtin_id;
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
            partial_args: Vec::new(),
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
            partial_args: Vec::new(),
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
            partial_args: Vec::new(),
        });

        assert_eq!(
            inspect_value(&vm, &value),
            "FnCapture(module: <local>, name: add, signature: add(x: Int, y: Int) -> Int)"
        );
    }

    #[test]
    fn inspect_keeps_legacy_callable_display_for_partial_application() {
        let vm = VM::new(Bytecode::default());
        let value = Value::Callable(Callable {
            target: CallableTarget::Builtin(8),
            lexical_captures: Vec::new(),
            partial_args: vec![Value::Int(int(1))],
        });

        assert_eq!(inspect_value(&vm, &value), "<builtin:8>");
    }
}
