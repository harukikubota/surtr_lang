use super::*;

pub(super) const AUTO_IMPORT_MODULES: &[&str] = &["Bootstrap", "Kernel", "Result"];

fn initialize_base_scope() -> Scope {
    let mut scope = Scope::new();
    let dummy = Span { start: 0, end: 0 };
    // Standalone resolver tests do not stage std modules, so keep placeholders
    // for `Ok` / `Err` here. Real module builds auto-import `Result`, which
    // overwrites these bindings with the canonical constructor declarations.
    scope.define("Ok", dummy.clone());
    scope.define("Err", dummy);
    scope
}

pub(super) fn initialize_scope() -> Scope {
    let mut scope = initialize_base_scope();
    for meta in BUILTIN_METAS {
        if is_global_runtime_builtin(meta.name) {
            scope.define_with_id(meta.name, builtin_uid(meta.builtin_id));
        }
    }
    scope
}

fn is_global_runtime_builtin(name: &str) -> bool {
    matches!(
        name,
        "print" | "to_string" | "inspect" | "safe_div" | "safe_mod" | "eprint" | "set_exit_code"
    )
}

pub(super) fn resolve_decl_attrs(attrs: &DeclAttrs) -> ResolvedDeclAttrs {
    ResolvedDeclAttrs {
        doc: attrs.doc.clone(),
    }
}

pub(super) fn is_runtime_builtin_decl(name: &str) -> bool {
    BUILTIN_METAS.iter().any(|meta| meta.name == name)
}

pub(super) fn is_special_form_builtin_decl(name: &str) -> bool {
    matches!(
        name,
        "if" | "if_then"
            | "assert"
            | "ensure"
            | "and"
            | "or"
            | "eq"
            | "neq"
            | "lt"
            | "lte"
            | "gt"
            | "gte"
            | "concat"
    )
}
