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
        sig_str: "($A, $A) -> Result<$A, ZeroDivisionError>",
    },
    BuiltinMeta {
        name: "safe_mod",
        builtin_id: 4,
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, ZeroDivisionError>",
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
        sig_str: "(Int, Int) -> Result<Int, NegativeShiftCount>",
    },
    BuiltinMeta {
        name: "shr",
        builtin_id: 8,
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeShiftCount>",
    },
    BuiltinMeta {
        name: "len",
        builtin_id: 9,
        arity: 1,
        sig_str: "(List<$A>) -> Int",
    },
    BuiltinMeta {
        name: "bit_and",
        builtin_id: 10,
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_or",
        builtin_id: 11,
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_xor",
        builtin_id: 12,
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_not",
        builtin_id: 13,
        arity: 1,
        sig_str: "(Int) -> Int",
    },
    BuiltinMeta {
        name: "test_bit",
        builtin_id: 14,
        arity: 2,
        sig_str: "(Int, Int) -> Result<Boolean, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "set_bit",
        builtin_id: 15,
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "clear_bit",
        builtin_id: 16,
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "toggle_bit",
        builtin_id: 17,
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "codepoints",
        builtin_id: 18,
        arity: 2,
        sig_str: "(String, StringEncoding) -> Result<List<Int>, InvalidStringEncoding>",
    },
    BuiltinMeta {
        name: "from_codepoints",
        builtin_id: 19,
        arity: 2,
        sig_str: "(List<Int>, StringEncoding) -> Result<String, InvalidStringEncoding>",
    },
    BuiltinMeta {
        name: "map_err",
        builtin_id: 20,
        arity: 2,
        sig_str: "(Result<$T>, Error) -> Result<$T>",
    },
    BuiltinMeta {
        name: "cause",
        builtin_id: 21,
        arity: 2,
        sig_str: "(Result<$T>, Error) -> Result<$T>",
    },
    BuiltinMeta {
        name: "chain",
        builtin_id: 22,
        arity: 2,
        sig_str: "(Result<$T>, Result<()>) -> Result<$T>",
    },
    BuiltinMeta {
        name: "__test_push",
        builtin_id: 23,
        arity: 2,
        sig_str: "(String, String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_pop",
        builtin_id: 24,
        arity: 0,
        sig_str: "() -> Unit",
    },
    BuiltinMeta {
        name: "__test_pass",
        builtin_id: 25,
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_fail",
        builtin_id: 26,
        arity: 2,
        sig_str: "(String, String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_fail_current",
        builtin_id: 27,
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "group_count",
        builtin_id: 28,
        arity: 1,
        sig_str: "(List<$A>) -> List<($A, Int)>",
    },
    BuiltinMeta {
        name: "zip",
        builtin_id: 29,
        arity: 2,
        sig_str: "(List<$A>, List<$B>) -> List<($A, $B)>",
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
    BuiltinTypeMeta {
        name: "TypeRef",
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
