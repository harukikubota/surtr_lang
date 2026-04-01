use crate::error::RuntimeError;
use crate::value::Value;
use crate::vm::VM;
use sindr::builtin::{builtin_meta_by_id, BUILTIN_METAS};

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
        func: builtin_eprint,
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
    let meta = builtin_meta_by_id(builtin_id).ok_or_else(|| RuntimeError {
        message: format!("Unknown builtin id: {}", builtin_id),
    })?;
    if args.len() != usize::from(meta.arity) {
        return Err(RuntimeError {
            message: format!(
                "builtin {} arity mismatch: expected {}, got {}",
                meta.name,
                meta.arity,
                args.len()
            ),
        });
    }

    let func = BUILTIN_IMPLS
        .get(builtin_id as usize)
        .ok_or_else(|| RuntimeError {
            message: format!("Missing builtin implementation for id {}", builtin_id),
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
                    return Err(RuntimeError {
                        message: format!("Failed to render rich error report: {}", err),
                    });
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

pub fn inspect_value(vm: &VM, value: &Value) -> String {
    let registry = vm.type_registry();
    value.to_display_string(&registry)
}

#[cfg(test)]
mod tests {
    use sindr::builtin::BUILTIN_METAS;

    fn parse_builtin_decl(line: &str) -> (&str, u8, String) {
        let rest = line
            .strip_prefix("@builtin def ")
            .expect("builtin line must start with @builtin def");
        let (name, after_name) = rest
            .split_once('(')
            .expect("builtin declaration must include params");

        let (params, after_params) = after_name
            .split_once(')')
            .expect("builtin declaration must close params");
        let ret_ty = after_params
            .trim()
            .strip_prefix("->")
            .expect("builtin declaration must include return type")
            .trim();

        let param_tys = if params.trim().is_empty() {
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
                .collect::<Vec<_>>()
        };

        let sig = format!("({}) -> {}", param_tys.join(", "), ret_ty);
        (name.trim(), param_tys.len() as u8, sig)
    }

    #[test]
    fn builtin_srt_and_builtin_meta_are_aligned() {
        let source = include_str!("../../../lib/builtin.srt");
        let lines = source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), BUILTIN_METAS.len());

        for (line, meta) in lines.iter().zip(BUILTIN_METAS.iter()) {
            let (name, arity, sig_str) = parse_builtin_decl(line);
            assert_eq!(name, meta.name);
            assert_eq!(arity, meta.arity);
            assert_eq!(sig_str, meta.sig_str);
        }
    }
}
