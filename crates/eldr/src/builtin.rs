use crate::error::RuntimeError;
use crate::value::Value;
use crate::vm::VM;

/// Function pointer type for built-in implementations.
pub type BuiltinFn = fn(&mut VM, Vec<Value>) -> Result<Value, RuntimeError>;

/// Definition of a built-in function — the single source of truth.
/// Sigil, Scar, and Eldr all read from this table.
pub struct BuiltinDef {
    pub name: &'static str,
    pub builtin_id: u16,
    pub func: BuiltinFn,
    pub arity: u8,
    /// Type signature string for Scar's type registration.
    pub sig_str: &'static str,
}

/// The canonical built-in function table.
pub const BUILTINS: &[BuiltinDef] = &[
    BuiltinDef {
        name: "print",
        builtin_id: 0,
        func: builtin_print,
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinDef {
        name: "to_string",
        builtin_id: 1,
        func: builtin_to_string,
        arity: 1,
        sig_str: "($A) -> String",
    },
    BuiltinDef {
        name: "eprint",
        builtin_id: 2,
        func: builtin_eprint,
        arity: 1,
        sig_str: "(Error) -> Unit",
    },
];

// ── Built-in implementations ──

fn builtin_print(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    let s = match &args[0] {
        Value::Str(s) => s.clone(),
        other => {
            let registry = vm.type_registry().clone();
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
    let registry = vm.type_registry().clone();
    let s = args[0].to_display_string(&registry);
    Ok(Value::Str(s))
}

fn builtin_eprint(vm: &mut VM, args: Vec<Value>) -> Result<Value, RuntimeError> {
    match &args[0] {
        Value::Error(rich) => {
            let msg = format!(
                "Error: {}\n  --> {}:{}:{}\n  {}\n",
                rich.kind,
                rich.location.file,
                rich.location.line,
                rich.location.column,
                rich.message,
            );
            match &mut vm.error_output {
                Some(buf) => buf.push(msg),
                None => eprint!("{}", msg),
            }
        }
        other => {
            let registry = vm.type_registry().clone();
            let s = other.to_display_string(&registry);
            match &mut vm.error_output {
                Some(buf) => buf.push(s),
                None => eprintln!("{}", s),
            }
        }
    }
    Ok(Value::Unit)
}
