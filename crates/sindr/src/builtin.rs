/// Built-in function metadata shared across Sigil / Scar / Forge / Eldr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinMeta {
    pub name: &'static str,
    pub builtin_id: u16,
    pub arity: u8,
    /// Type signature string used by type checker bootstrap and validation.
    pub sig_str: &'static str,
}

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
        name: "eprint",
        builtin_id: 3,
        arity: 1,
        sig_str: "(Error) -> Unit",
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

pub fn builtin_uid(builtin_id: u16) -> u32 {
    BUILTIN_UID_BASE + u32::from(builtin_id)
}

#[cfg(test)]
mod tests {
    use super::{builtin_uid, BUILTIN_METAS};

    #[test]
    fn builtin_ids_match_definition_order() {
        for (idx, meta) in BUILTIN_METAS.iter().enumerate() {
            assert_eq!(meta.builtin_id as usize, idx);
            assert_eq!(builtin_uid(meta.builtin_id), 2 + idx as u32);
        }
    }
}
