/// Built-in function metadata shared across Sigil / Scar / Forge / Eldr.
///
/// Surtr source files under `lib/*.srt` may declare these builtins with
/// `@@builtin`, but the canonical definition order and ids live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinMeta {
    pub name: &'static str,
    pub builtin_id: u16,
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
        builtin_id: 0,
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "to_string",
        builtin_id: 1,
        arity: 1,
        sig_str: "($A) -> String",
    },
    BuiltinMeta {
        name: "inspect",
        builtin_id: 2,
        arity: 1,
        sig_str: "($A) -> String",
    },
    BuiltinMeta {
        name: "safe_div",
        builtin_id: 3,
        arity: 2,
        sig_str: "($A, $A) -> Result<$A>",
    },
    BuiltinMeta {
        name: "safe_mod",
        builtin_id: 4,
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int>",
    },
    BuiltinMeta {
        name: "eprint",
        builtin_id: 5,
        arity: 1,
        sig_str: "(Error) -> Unit",
    },
    BuiltinMeta {
        name: "set_exit_code",
        builtin_id: 6,
        arity: 1,
        sig_str: "(Int) -> Unit",
    },
    BuiltinMeta {
        name: "shl",
        builtin_id: 7,
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "shr",
        builtin_id: 8,
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "wrap",
        builtin_id: 9,
        arity: 1,
        sig_str: "($A) -> List<$A>",
    },
    BuiltinMeta {
        name: "map",
        builtin_id: 10,
        arity: 2,
        sig_str: "(List<$A>, ($A -> $B)) -> List<$B>",
    },
    BuiltinMeta {
        name: "flat_map",
        builtin_id: 11,
        arity: 2,
        sig_str: "(List<$A>, ($A -> List<$B>)) -> List<$B>",
    },
];

/// Canonical builtin type declarations accepted from std-module sources.
///
/// These entries define the exact source-level heads the compiler accepts,
/// including generic parameter names such as `List<$A>` and `Result<$T>`.
pub const BUILTIN_TYPE_METAS: &[BuiltinTypeMeta] = &[
    BuiltinTypeMeta {
        name: "Int",
        params: &[],
    },
    BuiltinTypeMeta {
        name: "Float",
        params: &[],
    },
    BuiltinTypeMeta {
        name: "String",
        params: &[],
    },
    BuiltinTypeMeta {
        name: "Boolean",
        params: &[],
    },
    BuiltinTypeMeta {
        name: "Unit",
        params: &[],
    },
    BuiltinTypeMeta {
        name: "Error",
        params: &[],
    },
    BuiltinTypeMeta {
        name: "List",
        params: &["$A"],
    },
    BuiltinTypeMeta {
        name: "Result",
        params: &["$T"],
    },
];

pub fn builtin_meta_by_name(name: &str) -> Option<&'static BuiltinMeta> {
    BUILTIN_METAS.iter().find(|meta| meta.name == name)
}

pub fn builtin_meta_by_id(builtin_id: u16) -> Option<&'static BuiltinMeta> {
    let idx = builtin_id as usize;
    BUILTIN_METAS
        .get(idx)
        .filter(|meta| meta.builtin_id == builtin_id)
}

pub fn builtin_type_meta_by_name(name: &str) -> Option<&'static BuiltinTypeMeta> {
    BUILTIN_TYPE_METAS.iter().find(|meta| meta.name == name)
}

pub fn builtin_uid(builtin_id: u16) -> u32 {
    BUILTIN_UID_BASE + u32::from(builtin_id)
}

#[cfg(test)]
mod tests {
    use super::{builtin_meta_by_id, builtin_meta_by_name, builtin_uid, BUILTIN_METAS};

    #[test]
    fn builtin_ids_match_definition_order() {
        for (idx, meta) in BUILTIN_METAS.iter().enumerate() {
            assert_eq!(meta.builtin_id as usize, idx);
            assert_eq!(builtin_uid(meta.builtin_id), 2 + idx as u32);
        }
    }

    #[test]
    fn builtin_lookup_returns_none_for_unknown_values() {
        assert!(builtin_meta_by_id(u16::MAX).is_none());
        assert!(builtin_meta_by_name("__missing__").is_none());
    }
}
