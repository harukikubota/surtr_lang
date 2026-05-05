use super::*;

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
    for (idx, meta) in BUILTIN_METAS.iter().enumerate() {
        if is_global_runtime_builtin(meta.name) {
            scope.define_with_id(meta.name, builtin_uid(idx as u16));
        }
    }
    scope
}

fn is_global_runtime_builtin(name: &str) -> bool {
    matches!(
        name,
        "print"
            | "to_string"
            | "inspect"
            | "safe_div"
            | "safe_mod"
            | "eprint"
            | "set_exit_code"
            | "__process_pid"
            | "__process_spawn"
            | "__process_state"
            | "__process_store"
            | "__process_self"
            | "__process_context_handler"
            | "__out_handler_write"
            | "__process_sleep"
            | "__task_call"
            | "__task_async"
            | "__task_launch"
            | "__task_cast"
            | "__task_call_timeout"
            | "__task_async_timeout"
            | "__task_launch_timeout"
            | "__task_cast_timeout"
    )
}

pub(super) fn resolve_decl_attrs(attrs: &DeclAttrs) -> ResolvedDeclAttrs {
    ResolvedDeclAttrs {
        doc: attrs.doc.clone(),
        hidden: attrs.hidden,
        visibility: attrs.visibility,
    }
}

pub(super) fn is_runtime_builtin_decl(name: &str) -> bool {
    BUILTIN_METAS.iter().any(|meta| meta.name == name)
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
