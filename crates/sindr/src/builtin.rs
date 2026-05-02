use crate::names::{builtin_type_name, TypeName};

/// Built-in function metadata shared across Sigil / Scar / Forge / Eldr.
///
/// Surtr source files under `lib/*.srt` may declare these builtins with
/// `@@builtin`, but the canonical definition order and ids live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinMeta {
    pub name: &'static str,
    pub arity: u8,
    /// Type signature string used by type checker bootstrap and validation.
    pub sig_str: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTypeMeta {
    /// Canonical builtin type head that std-module `@@builtin type`
    /// declarations must match exactly.
    pub name: &'static str,
    pub params: &'static [&'static str],
}

/// Builtin unique ids start after the first two scope-reserved ids.
pub const BUILTIN_UID_BASE: u32 = 2;

pub const BUILTIN_METAS: &[BuiltinMeta] = &[
    BuiltinMeta {
        name: "print",
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "to_string",
        arity: 1,
        sig_str: "($A) -> String",
    },
    BuiltinMeta {
        name: "inspect",
        arity: 1,
        sig_str: "($A) -> String",
    },
    BuiltinMeta {
        name: "safe_div",
        arity: 2,
        sig_str: "($A, $A) -> Result<$A, ZeroDivisionError>",
    },
    BuiltinMeta {
        name: "safe_mod",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, ZeroDivisionError>",
    },
    BuiltinMeta {
        name: "eprint",
        arity: 1,
        sig_str: "(Error) -> Unit",
    },
    BuiltinMeta {
        name: "set_exit_code",
        arity: 1,
        sig_str: "(Int) -> Unit",
    },
    BuiltinMeta {
        name: "shl",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeShiftCount>",
    },
    BuiltinMeta {
        name: "shr",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeShiftCount>",
    },
    BuiltinMeta {
        name: "len",
        arity: 1,
        sig_str: "(List<$A>) -> Int",
    },
    BuiltinMeta {
        name: "gen_make",
        arity: 2,
        sig_str: "(Int, List<$Item>) -> Generator<$State, $Item>",
    },
    BuiltinMeta {
        name: "gen_idx",
        arity: 1,
        sig_str: "(Generator<$State, $Item>) -> Int",
    },
    BuiltinMeta {
        name: "gen_items",
        arity: 1,
        sig_str: "(Generator<$State, $Item>) -> List<$Item>",
    },
    BuiltinMeta {
        name: "bit_and",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_or",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_xor",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_not",
        arity: 1,
        sig_str: "(Int) -> Int",
    },
    BuiltinMeta {
        name: "test_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Boolean, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "set_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "clear_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "toggle_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "codepoints",
        arity: 2,
        sig_str: "(String, StringEncoding) -> Result<List<Int>, InvalidStringEncoding>",
    },
    BuiltinMeta {
        name: "from_codepoints",
        arity: 2,
        sig_str: "(List<Int>, StringEncoding) -> Result<String, InvalidStringEncoding>",
    },
    BuiltinMeta {
        name: "map_err",
        arity: 2,
        sig_str: "(Result<$T>, Error) -> Result<$T>",
    },
    BuiltinMeta {
        name: "cause",
        arity: 2,
        sig_str: "(Result<$T>, Error) -> Result<$T>",
    },
    BuiltinMeta {
        name: "chain",
        arity: 2,
        sig_str: "(Result<$T>, Result<()>) -> Result<$T>",
    },
    BuiltinMeta {
        name: "__test_push",
        arity: 2,
        sig_str: "(String, String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_pop",
        arity: 0,
        sig_str: "() -> Unit",
    },
    BuiltinMeta {
        name: "__test_pass",
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_fail",
        arity: 2,
        sig_str: "(String, String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_fail_error",
        arity: 2,
        sig_str: "(String, Error) -> Unit",
    },
    BuiltinMeta {
        name: "__test_fail_current",
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "group_count",
        arity: 1,
        sig_str: "(List<$A>) -> List<($A, Int)>",
    },
    BuiltinMeta {
        name: "zip",
        arity: 2,
        sig_str: "(List<$A>, List<$B>) -> List<($A, $B)>",
    },
    BuiltinMeta {
        name: "empty_map",
        arity: 0,
        sig_str: "() -> HashMap<$V>",
    },
    BuiltinMeta {
        name: "map_from_entries",
        arity: 1,
        sig_str: "(List<(String, $V)>) -> HashMap<$V>",
    },
    BuiltinMeta {
        name: "map_len",
        arity: 1,
        sig_str: "(HashMap<$V>) -> Int",
    },
    BuiltinMeta {
        name: "map_contains_key",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> Boolean",
    },
    BuiltinMeta {
        name: "map_get",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> Result<$V, NoneError>",
    },
    BuiltinMeta {
        name: "map_insert",
        arity: 3,
        sig_str: "(HashMap<$V>, String, $V) -> HashMap<$V>",
    },
    BuiltinMeta {
        name: "map_remove",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> HashMap<$V>",
    },
    BuiltinMeta {
        name: "map_keys",
        arity: 1,
        sig_str: "(HashMap<$V>) -> List<String>",
    },
    BuiltinMeta {
        name: "map_values_list",
        arity: 1,
        sig_str: "(HashMap<$V>) -> List<$V>",
    },
    BuiltinMeta {
        name: "view",
        arity: 2,
        sig_str: "(Lens<$S, $A>, $S) -> Result<$A>",
    },
    BuiltinMeta {
        name: "compose",
        arity: 2,
        sig_str: "(Lens<$S, $A>, Lens<$A, $B>) -> Lens<$S, $B>",
    },
    BuiltinMeta {
        name: "set",
        arity: 3,
        sig_str: "(Lens<$S, $A>, $S, $A) -> Result<$S>",
    },
    BuiltinMeta {
        name: "over",
        arity: 3,
        sig_str: "(Lens<$S, $A>, $S, ($A -> Result<$A>)) -> Result<$S>",
    },
    BuiltinMeta {
        name: "__test_capture_stdout",
        arity: 0,
        sig_str: "() -> List<String>",
    },
    BuiltinMeta {
        name: "__test_capture_stderr",
        arity: 0,
        sig_str: "() -> List<String>",
    },
    BuiltinMeta {
        name: "compile",
        arity: 1,
        sig_str: "(String) -> Result<Regex, RegexCompileError>",
    },
    BuiltinMeta {
        name: "is_match",
        arity: 2,
        sig_str: "(Regex, String) -> Boolean",
    },
    BuiltinMeta {
        name: "captures",
        arity: 2,
        sig_str: "(Regex, String) -> Result<RegexCaptures, NoneError>",
    },
    BuiltinMeta {
        name: "whole",
        arity: 1,
        sig_str: "(RegexCaptures) -> String",
    },
    BuiltinMeta {
        name: "capture_count",
        arity: 1,
        sig_str: "(RegexCaptures) -> Int",
    },
    BuiltinMeta {
        name: "get",
        arity: 2,
        sig_str: "(RegexCaptures, Int) -> Result<String, NoneError>",
    },
    BuiltinMeta {
        name: "get_name",
        arity: 2,
        sig_str: "(RegexCaptures, String) -> Result<String, NoneError>",
    },
    BuiltinMeta {
        name: "find",
        arity: 2,
        sig_str: "(Regex, String) -> Result<RegexMatch, NoneError>",
    },
    BuiltinMeta {
        name: "find_all",
        arity: 2,
        sig_str: "(Regex, String) -> List<RegexMatch>",
    },
    BuiltinMeta {
        name: "split",
        arity: 2,
        sig_str: "(Regex, String) -> List<String>",
    },
    BuiltinMeta {
        name: "replace",
        arity: 3,
        sig_str: "(Regex, String, String) -> String",
    },
    BuiltinMeta {
        name: "replace_all",
        arity: 3,
        sig_str: "(Regex, String, String) -> String",
    },
    BuiltinMeta {
        name: "escape",
        arity: 1,
        sig_str: "(String) -> String",
    },
    BuiltinMeta {
        name: "group_names",
        arity: 1,
        sig_str: "(Regex) -> List<String>",
    },
    BuiltinMeta {
        name: "text",
        arity: 1,
        sig_str: "(RegexMatch) -> String",
    },
    BuiltinMeta {
        name: "start",
        arity: 1,
        sig_str: "(RegexMatch) -> Int",
    },
    BuiltinMeta {
        name: "end",
        arity: 1,
        sig_str: "(RegexMatch) -> Int",
    },
    BuiltinMeta {
        name: "project_args",
        arity: 0,
        sig_str: "() -> List<String>",
    },
    BuiltinMeta {
        name: "io_get",
        arity: 1,
        sig_str: "(String) -> Result<String, InputError>",
    },
    BuiltinMeta {
        name: "io_get_line",
        arity: 1,
        sig_str: "(String) -> Result<String, InputError>",
    },
    BuiltinMeta {
        name: "seed",
        arity: 1,
        sig_str: "(Int) -> RandomGenerator",
    },
    BuiltinMeta {
        name: "int_until",
        arity: 1,
        sig_str: "(Int) -> Result<Int, InvalidRandomRange>",
    },
    BuiltinMeta {
        name: "int_range",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, InvalidRandomRange>",
    },
    BuiltinMeta {
        name: "next_int_until",
        arity: 2,
        sig_str: "(RandomGenerator, Int) -> Result<(Int, RandomGenerator), InvalidRandomRange>",
    },
    BuiltinMeta {
        name: "next_int_range",
        arity: 3,
        sig_str:
            "(RandomGenerator, Int, Int) -> Result<(Int, RandomGenerator), InvalidRandomRange>",
    },
    BuiltinMeta {
        name: "kind",
        arity: 1,
        sig_str: "(Error) -> String",
    },
    BuiltinMeta {
        name: "message",
        arity: 1,
        sig_str: "(Error) -> String",
    },
    BuiltinMeta {
        name: "format",
        arity: 1,
        sig_str: "(Error) -> String",
    },
    BuiltinMeta {
        name: "__process_pid",
        arity: 2,
        sig_str: "(String, (-> Result<$State>)) -> PID<$Process>",
    },
    BuiltinMeta {
        name: "__process_spawn",
        arity: 2,
        sig_str: "(String, (-> Result<$State>)) -> Result<PID<$Process>>",
    },
    BuiltinMeta {
        name: "__process_state",
        arity: 1,
        sig_str: "(PID<$Process>) -> Result<$State>",
    },
    BuiltinMeta {
        name: "__process_store",
        arity: 2,
        sig_str: "(PID<$Process>, $State) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__process_self",
        arity: 0,
        sig_str: "() -> PID<$Process>",
    },
    BuiltinMeta {
        name: "__process_sleep",
        arity: 1,
        sig_str: "(Duration) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__task_call",
        arity: 1,
        sig_str: "((-> Result<$A>)) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__task_async",
        arity: 1,
        sig_str: "((-> Result<$A>)) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__task_launch",
        arity: 1,
        sig_str: "((-> Result<Unit>)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__task_cast",
        arity: 1,
        sig_str: "((-> Unit)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__task_call_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<$A>)) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__task_async_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<$A>)) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__task_launch_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<Unit>)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__task_cast_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Unit)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__operator_int_add",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "__operator_int_sub",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "__operator_int_mul",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "__operator_float_add",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
    },
    BuiltinMeta {
        name: "__operator_float_sub",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
    },
    BuiltinMeta {
        name: "__operator_float_mul",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
    },
    BuiltinMeta {
        name: "__operator_int_eq",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_neq",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_lt",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_lte",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_gt",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_gte",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_eq",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_neq",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_lt",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_lte",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_gt",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_gte",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_string_eq",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_string_neq",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_boolean_eq",
        arity: 2,
        sig_str: "(Boolean, Boolean) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_boolean_neq",
        arity: 2,
        sig_str: "(Boolean, Boolean) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_string_concat",
        arity: 2,
        sig_str: "(String, String) -> String",
    },
];

/// Canonical builtin type declarations accepted from std-module sources.
///
/// These entries define the exact source-level heads the compiler accepts,
/// including generic parameter names such as `List<$A>` and `Result<$T>`.
pub const BUILTIN_TYPE_METAS: &[BuiltinTypeMeta] = &[
    BuiltinTypeMeta {
        name: TypeName::Int.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Float.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::String.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Boolean.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Unit.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Closure.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::MatchArms.as_str(),
        params: &["$Scrutinee", "$Result"],
    },
    BuiltinTypeMeta {
        name: TypeName::CondClauses.as_str(),
        params: &["$Result"],
    },
    BuiltinTypeMeta {
        name: TypeName::Error.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Regex.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::RegexCaptures.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::RegexMatch.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::RandomGenerator.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::List.as_str(),
        params: &["$A"],
    },
    BuiltinTypeMeta {
        name: TypeName::HashMap.as_str(),
        params: &["$V"],
    },
    BuiltinTypeMeta {
        name: TypeName::Generator.as_str(),
        params: &["$State", "$Item"],
    },
    BuiltinTypeMeta {
        name: TypeName::Result.as_str(),
        params: &["$T"],
    },
    BuiltinTypeMeta {
        name: TypeName::TypeRef.as_str(),
        params: &["$T"],
    },
    BuiltinTypeMeta {
        name: TypeName::Hole.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Lens.as_str(),
        params: &["$S", "$A"],
    },
];

pub fn builtin_meta_by_name(name: &str) -> Option<&'static BuiltinMeta> {
    BUILTIN_METAS.iter().find(|meta| meta.name == name)
}

pub fn builtin_runtime_name<'a>(declared_name: &'a str, qualified_name: Option<&str>) -> &'a str {
    match qualified_name {
        Some("IO::get") => "io_get",
        Some("IO::get_line") => "io_get_line",
        Some("Process::self") => "__process_self",
        Some("Process::sleep") => "__process_sleep",
        Some("Task::call") => "__task_call",
        Some("Task::async") => "__task_async",
        Some("Task::launch") => "__task_launch",
        Some("Task::cast") => "__task_cast",
        _ => declared_name,
    }
}

pub fn builtin_meta_for_decl(
    declared_name: &str,
    qualified_name: Option<&str>,
) -> Option<&'static BuiltinMeta> {
    builtin_meta_by_name(builtin_runtime_name(declared_name, qualified_name))
}

pub fn builtin_id_by_name(name: &str) -> Option<u16> {
    BUILTIN_METAS
        .iter()
        .position(|meta| meta.name == name)
        .and_then(|idx| (idx <= u16::MAX as usize).then_some(idx as u16))
}

pub fn builtin_meta_by_id(builtin_id: u16) -> Option<&'static BuiltinMeta> {
    BUILTIN_METAS.get(builtin_id as usize)
}

pub fn builtin_type_meta_by_name(name: &str) -> Option<&'static BuiltinTypeMeta> {
    BUILTIN_TYPE_METAS.iter().find(|meta| meta.name == name)
}

pub fn builtin_type_supports_inherent_impl(name: &str) -> bool {
    builtin_type_name(name).is_some_and(TypeName::supports_inherent_impl)
}

pub fn builtin_uid(builtin_id: u16) -> u32 {
    BUILTIN_UID_BASE + u32::from(builtin_id)
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_id_by_name, builtin_meta_by_id, builtin_meta_by_name, builtin_uid, BUILTIN_METAS,
    };

    #[test]
    fn builtin_ids_match_definition_order() {
        for (idx, meta) in BUILTIN_METAS.iter().enumerate() {
            let id = idx as u16;
            assert_eq!(builtin_id_by_name(meta.name), Some(id));
            assert_eq!(builtin_uid(id), 2 + idx as u32);
        }
    }

    #[test]
    fn builtin_lookup_returns_none_for_unknown_values() {
        assert!(builtin_meta_by_id(u16::MAX).is_none());
        assert!(builtin_meta_by_name("__missing__").is_none());
    }
}
