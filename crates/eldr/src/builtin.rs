use crate::error::RuntimeError;
use crate::value::Value;
use crate::vm::VM;
use sindr::builtin::{builtin_meta_by_id, BUILTIN_METAS};
use sindr::primitives::{ToPrimitive, Zero};
use sindr::runtime::{Location, RichError};

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
                let msg = format!("Error: {}: {}", rich.kind, rich.message);
                buf.push(msg);
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
            } else {
                eprintln!("Error: {}: {}", rich.kind, rich.message);
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
    let amount = bits.to_usize().ok_or_else(|| {
        RuntimeError::new(format!("shl shift amount must be non-negative: {}", bits))
    })?;
    let shifted = value << amount;
    Ok(Value::Int(shifted))
}

fn builtin_shr(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(bits)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("shr expects (Int, Int)"));
    };
    let amount = bits.to_usize().ok_or_else(|| {
        RuntimeError::new(format!("shr shift amount must be non-negative: {}", bits))
    })?;
    let shifted = value >> amount;
    Ok(Value::Int(shifted))
}

fn builtin_list_len(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let Value::List(list) = &args[0] else {
        return Err(RuntimeError::new("len expects List as first argument"));
    };
    Ok(Value::Int(list.len.into()))
}

pub fn inspect_value(vm: &VM, value: &Value) -> String {
    let registry = vm.type_registry();
    value.to_display_string(registry)
}

fn ok_result(value: Value) -> Value {
    Value::Tagged {
        tag: 0,
        fields: vec![value],
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

    Value::Tagged {
        tag: 1,
        fields: vec![Value::Error(Box::new(RichError {
            kind: kind.into(),
            message: message.into(),
            location,
        }))],
    }
}

#[cfg(test)]
mod tests {
    use super::call_builtin;
    use crate::vm::VM;
    use sindr::builtin::{builtin_meta_by_name, BUILTIN_METAS};
    use sindr::ir::Bytecode;
    use sindr::primitives::int;
    use sindr::runtime::{TypeRegistry, Value};

    fn test_vm() -> VM {
        VM::new(Bytecode {
            type_registry: TypeRegistry::new(),
            ..Bytecode::default()
        })
        .with_error_capture()
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
        let (params, after_params) = after_name
            .split_once(')')
            .expect("def declaration must close params");
        let ret_ty = after_params
            .trim()
            .strip_prefix("->")
            .expect("def declaration must include return type")
            .trim();
        let param_tys: Vec<String> = if params.trim().is_empty() {
            Vec::new()
        } else {
            params
                .split(',')
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
        ];

        // Collect all lines across the std-module files that currently declare
        // builtin value surfaces. Bootstrap intentionally stays almost empty,
        // Kernel owns the cross-cutting builtins, Int currently carries both
        // arithmetic-result builtins and bit-shift helpers, and List declares
        // the O(1) length helper.
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

        assert_eq!(entries.len(), BUILTIN_METAS.len());

        // Source layout is allowed to group builtins by module ownership
        // rather than by builtin id order, so compare by builtin name instead
        // of relying on declaration order in `lib/*.srt`.
        let mut entry_map = std::collections::BTreeMap::new();
        for (name, arity, sig_str) in entries {
            let prev = entry_map.insert(name.clone(), (arity, sig_str));
            assert!(prev.is_none(), "duplicate builtin declaration for {name}");
        }

        for meta in BUILTIN_METAS.iter() {
            let (arity, sig_str) = entry_map
                .get(meta.name)
                .unwrap_or_else(|| panic!("missing builtin declaration for {}", meta.name));
            assert_eq!(*arity, meta.arity, "arity mismatch for {}", meta.name);
            assert_eq!(sig_str, &meta.sig_str, "sig mismatch for {}", meta.name);
        }
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
        }));
        let result = call_builtin(&mut vm, 5, vec![value]).expect("eprint should succeed");
        assert_eq!(result, Value::Unit);
        assert_eq!(
            vm.error_output.as_deref(),
            Some(&["Error: Boom: broken".to_string()][..])
        );
    }

    #[test]
    fn eprint_falls_back_to_inspect_for_non_error_values() {
        let mut vm = test_vm();
        let result =
            call_builtin(&mut vm, 5, vec![Value::Int(int(42))]).expect("eprint should succeed");
        assert_eq!(result, Value::Unit);
        assert_eq!(
            vm.error_output.as_deref(),
            Some(&["42".to_string()][..])
        );
    }
}
