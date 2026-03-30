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
        other => {
            let registry = vm.type_registry();
            other.to_display_string(&registry)
        }
    };
    match &mut vm.output {
        Some(buf) => buf.push(s),
        None => println!("{}", s),
    }
    Ok(Value::Unit)
}

fn builtin_to_string(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let registry = vm.type_registry();
    let s = args[0].to_display_string(&registry);
    Ok(Value::Str(s))
}

fn builtin_eprint(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Error(rich) => {
            if vm.error_output.is_some() {
                let msg = format!("Error: {}: {}", rich.kind, rich.message);
                vm.error_output.as_mut().expect("checked is_some").push(msg);
            } else if let (Some(source), Some(file)) = (vm.source(), vm.source_file()) {
                use ariadne::{Color, Label, Report, ReportKind, Source};

                let start = rich.location.span_start as usize;
                let end = rich.location.span_end as usize;
                Report::build(ReportKind::Error, (file, start..end))
                    .with_message(rich.kind.clone())
                    .with_label(
                        Label::new((file, start..end))
                            .with_message(rich.message.clone())
                            .with_color(Color::Red),
                    )
                    .finish()
                    .eprint((file, Source::from(source)))
                    .unwrap();
            } else {
                eprintln!("Error: {}: {}", rich.kind, rich.message);
            }
        }
        other => {
            let registry = vm.type_registry();
            let s = other.to_display_string(&registry);
            match &mut vm.error_output {
                Some(buf) => buf.push(s),
                None => eprintln!("{}", s),
            }
        }
    }
    Ok(Value::Unit)
}

#[cfg(test)]
mod tests {
    use sindr::builtin::BUILTIN_METAS;

    #[test]
    fn builtin_srt_and_builtin_meta_are_aligned() {
        let source = include_str!("../../../lib/builtin.srt");
        let lines = source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), BUILTIN_METAS.len());

        for (line, meta) in lines.iter().zip(BUILTIN_METAS.iter()) {
            let rest = line
                .strip_prefix("@builtin def ")
                .expect("builtin line must start with @builtin def");
            let (name, after_name) = rest
                .split_once('(')
                .expect("builtin declaration must include params");
            assert_eq!(name.trim(), meta.name);

            let (params, _) = after_name
                .split_once(')')
                .expect("builtin declaration must close params");
            let arity = if params.trim().is_empty() {
                0
            } else {
                params.split(',').count()
            };
            assert_eq!(arity as u8, meta.arity);
        }
    }
}
