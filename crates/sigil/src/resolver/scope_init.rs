use super::*;

fn registered_builtin_scope() -> (Scope, Vec<(u32, &'static sindr::builtin::BuiltinMeta)>) {
    let mut scope = Scope::new();
    let mut bindings = Vec::new();
    for meta in builtin_function_metas() {
        if is_compiler_runtime_builtin(meta.name) {
            let uid = scope.define(meta.name, Span { start: 0, end: 0 });
            bindings.push((uid, meta));
        }
    }
    (scope, bindings)
}

pub(super) fn initialize_scope() -> Scope {
    registered_builtin_scope().0
}

/// Compiler symbols actually registered in the initial scope, in allocator order.
pub fn compiler_builtin_bindings() -> Vec<(u32, &'static sindr::builtin::BuiltinMeta)> {
    registered_builtin_scope().1
}

pub(super) fn is_compiler_runtime_builtin(name: &str) -> bool {
    matches!(
        name,
        "__task_call_timeout"
            | "__process_execute"
            | "__process_postprocess"
            | "__task_await_timeout"
            | "__workers_submit_timeout"
            | "__workers_broadcast_timeout"
            | "__genserver_call_reply"
            | "__genserver_call_reply_later"
            | "__genserver_call_stop_normal"
            | "__genserver_call_stop_error"
            | "__genserver_cast_next"
            | "__genserver_cast_stop_normal"
            | "__genserver_cast_stop_error"
    )
}

pub(super) fn resolve_decl_attrs(attrs: &DeclAttrs) -> ResolvedDeclAttrs {
    ResolvedDeclAttrs {
        doc: attrs.doc.clone(),
        builtin: attrs.builtin,
        derives: attrs.derives.clone(),
        facet_path_kind: attrs.facet_path_kind.clone(),
        hidden: attrs.hidden,
        readonly: attrs.readonly,
        visibility: attrs.visibility,
        user_importable: attrs.user_importable,
        user_callable: attrs.user_callable,
    }
}

pub(super) fn is_runtime_builtin_decl(name: &str) -> bool {
    builtin_function_metas()
        .iter()
        .any(|meta| meta.name == name)
}

pub(super) fn is_special_form_builtin_decl(name: &str, qualified_name: &str) -> bool {
    sindr::pattern::PatternConsumer::from_canonical_name(qualified_name).is_some()
        || matches!(
            name,
            "if" | "if_then"
                | "require"
                | "ensure"
                | "map_err"
                | "cause"
                | "recover"
                | "and"
                | "or"
                | "(,)"
        )
}

pub(super) fn is_doc_only_builtin_decl(name: &str) -> bool {
    matches!(name, "import" | "include")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_scope_requires_standard_declarations_for_public_functions() {
        let (scope, bindings) = registered_builtin_scope();
        for name in ["print", "to_string", "inspect", "eprint", "set_exit_code"] {
            assert_eq!(
                scope.lookup(name),
                None,
                "{name} must come from standard declarations"
            );
            assert!(bindings.iter().all(|(_, meta)| meta.name != name));
        }
        assert!(scope.lookup("__task_call_timeout").is_some());
        assert!(bindings
            .iter()
            .any(|(_, meta)| meta.name == "__task_call_timeout"));
    }
}
