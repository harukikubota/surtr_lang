use super::*;

fn initialize_base_scope() -> Scope {
    let mut scope = Scope::new();
    let dummy = Span { start: 0, end: 0 };
    // Standalone resolver tests do not stage std modules, so keep placeholders
    // for builtin-special constructor sugar here. Real module builds
    // overwrite these bindings with the canonical constructor declarations.
    scope.define("Ok", dummy.clone());
    scope.define("Err", dummy.clone());
    scope.define("True", dummy.clone());
    scope.define("False", dummy);
    scope
}

pub(super) fn initialize_scope() -> Scope {
    let mut scope = initialize_base_scope();
    for (idx, meta) in builtin_function_metas().iter().enumerate() {
        if is_global_runtime_builtin(meta.name) {
            scope.define_with_id(meta.name, builtin_uid(idx as u16));
        }
    }
    if let Some(hidden_boundary_idx) = builtin_function_metas()
        .iter()
        .position(|meta| meta.name == "__workers_broadcast_timeout")
    {
        scope.advance_next_id_to(builtin_uid((hidden_boundary_idx + 1) as u16));
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
        builtin: attrs.builtin,
        hidden: attrs.hidden,
        readonly: attrs.readonly,
        visibility: attrs.visibility,
        user_importable: attrs.user_importable,
        user_callable: attrs.user_callable,
    }
}

pub(super) fn is_runtime_builtin_decl(name: &str) -> bool {
    builtin_function_metas().iter().any(|meta| meta.name == name)
}

pub(super) fn is_special_form_builtin_decl(name: &str) -> bool {
    matches!(
        name,
        "if" | "if_then"
            | "if_let"
            | "if_let_then"
            | "is_match"
            | "assert"
            | "ensure"
            | "map_err"
            | "cause"
            | "recover"
            | "recover_kind"
            | "and"
            | "or"
    )
}

pub(super) fn is_doc_only_builtin_decl(name: &str) -> bool {
    matches!(name, "import" | "include")
}
