use crate::error::RuntimeError;
use crate::value::Value;
use crate::vm::VM;
use sindr::builtin::{builtin_meta_by_id, BUILTIN_METAS};
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
            if *b == 0 {
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
            if *b == 0 {
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
    let Value::Int(code) = args[0] else {
        return Err(RuntimeError::new("set_exit_code expects Int"));
    };
    let exit_code = i32::try_from(code)
        .map_err(|_| RuntimeError::new(format!("set_exit_code out of range for i32: {}", code)))?;
    vm.set_exit_code(exit_code);
    Ok(Value::Unit)
}

fn builtin_shl(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(bits)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("shl expects (Int, Int)"));
    };
    let amount = u32::try_from(*bits).map_err(|_| {
        RuntimeError::new(format!("shl shift amount must be non-negative: {}", bits))
    })?;
    let shifted = value
        .checked_shl(amount)
        .ok_or_else(|| RuntimeError::new(format!("shl shift amount out of range: {}", bits)))?;
    Ok(Value::Int(shifted))
}

fn builtin_shr(_vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let (Value::Int(value), Value::Int(bits)) = (&args[0], &args[1]) else {
        return Err(RuntimeError::new("shr expects (Int, Int)"));
    };
    let amount = u32::try_from(*bits).map_err(|_| {
        RuntimeError::new(format!("shr shift amount must be non-negative: {}", bits))
    })?;
    let shifted = value
        .checked_shr(amount)
        .ok_or_else(|| RuntimeError::new(format!("shr shift amount out of range: {}", bits)))?;
    Ok(Value::Int(shifted))
}

pub fn inspect_value(vm: &VM, value: &Value) -> String {
    let registry = vm.type_registry();
    value.to_display_string(&registry)
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
    use sindr::builtin::BUILTIN_METAS;

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
            include_str!("../../../lib/int.srt"),
        ];

        // Collect all lines across all sources.
        let all_lines: Vec<&str> = sources
            .iter()
            .flat_map(|s| s.lines())
            .map(str::trim)
            .collect();

        // For each @@builtin annotation, find the associated def signature.
        // Annotation order is flexible: @@builtin may appear on its own line
        // (int.srt style) or immediately before `def` on the same line
        // (@@builtin def ..., bootstrap.srt style).
        let mut entries: Vec<(String, u8, String)> = Vec::new();
        let mut i = 0;
        while i < all_lines.len() {
            let line = all_lines[i];
            if let Some(rest) = line.strip_prefix("@@builtin def ") {
                // Inline form: @@builtin def name(params) -> ret
                entries.push(parse_def_signature(rest));
            } else if line == "@@builtin" {
                // Standalone form: find the next `def` line.
                let mut j = i + 1;
                while j < all_lines.len() {
                    let next = all_lines[j];
                    if let Some(rest) = next.strip_prefix("def ") {
                        entries.push(parse_def_signature(rest));
                        break;
                    }
                    j += 1;
                }
            }
            i += 1;
        }

        assert_eq!(entries.len(), BUILTIN_METAS.len());

        for ((name, arity, sig_str), meta) in entries.iter().zip(BUILTIN_METAS.iter()) {
            assert_eq!(name, meta.name, "name mismatch");
            assert_eq!(*arity, meta.arity, "arity mismatch for {name}");
            assert_eq!(sig_str, &meta.sig_str, "sig mismatch for {name}");
        }
    }
}
