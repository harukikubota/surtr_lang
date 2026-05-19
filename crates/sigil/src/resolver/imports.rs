use super::declarations::{
    declaration_import_surface_status, is_module_visible_declaration, ImportSurfaceStatus,
};
use super::scope_init::initialize_scope;
use super::*;
use spire::ast::Visibility;

fn hidden_builtin_import_message(fq_name: &str) -> String {
    format!("Import target `{fq_name}` is a hidden builtin and cannot be imported")
}

fn restricted_surface_import_message(fq_name: &str) -> String {
    format!("Import target `{fq_name}` cannot be imported from user code")
}

fn builtin_special_variant_bare_alias(entry: &DeclarationEntry) -> Option<&'static str> {
    if entry.kind != DeclarationKind::EnumVariant {
        return None;
    }
    match global_surface_name(&entry.fq_name) {
        "Result::Ok" => Some("Ok"),
        "Result::Err" => Some("Err"),
        "Boolean::True" => Some("True"),
        "Boolean::False" => Some("False"),
        _ => None,
    }
}

fn auto_import_trait_names(declaration_index: &DeclarationIndex) -> HashSet<String> {
    declaration_index
        .values()
        .filter(|entry| entry.kind == DeclarationKind::Trait && entry.auto_import)
        .map(|entry| entry.name.clone())
        .collect()
}

fn is_result_facet_chain_conflict(
    existing_name: &str,
    incoming_name: &str,
    short_name: &str,
) -> bool {
    if short_name != "chain" {
        return false;
    }
    let existing = global_surface_name(existing_name);
    let incoming = global_surface_name(incoming_name);
    matches!(
        (existing, incoming),
        ("Result::chain", "Facet::chain") | ("Facet::chain", "Result::chain")
    )
}

fn declaration_is_auto_imported(import_context: &ImportContext<'_>, fq_name: &str) -> bool {
    import_context
        .declaration_index
        .get(fq_name)
        .is_some_and(|entry| {
            entry.auto_import
                || import_context
                    .auto_import_modules
                    .contains(global_surface_name(&entry.module_path))
                || import_context
                    .auto_import_traits
                    .contains(global_surface_name(&entry.module_path))
        })
}

pub(super) fn build_global_scope(
    index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
) -> Scope {
    let mut scope = initialize_scope();
    let mut canonical_trait_names = HashMap::new();
    for entry in index.values() {
        if matches!(
            entry.kind,
            DeclarationKind::Trait | DeclarationKind::TraitMethod
        ) {
            *canonical_trait_names
                .entry(entry.name.clone())
                .or_insert(0usize) += 1;
        }
    }
    for (fq_name, entry) in index {
        if entry.kind == DeclarationKind::BuiltinType {
            continue;
        }
        if entry.visibility != Visibility::Public {
            continue;
        }
        if let Some(uid) = declaration_uids.get(fq_name) {
            if entry.kind == DeclarationKind::Const {
                scope.define_with_id(fq_name, *uid);
                scope.define_with_id(&entry.name, *uid);
                continue;
            }
            scope.define_with_id(fq_name, *uid);
            if global_surface_name(fq_name) != fq_name {
                scope.define_with_id(global_surface_name(fq_name), *uid);
            }
            if global_surface_name(&entry.name) != entry.name {
                scope.define_with_id(global_surface_name(&entry.name), *uid);
            }
            if matches!(
                entry.kind,
                DeclarationKind::Trait | DeclarationKind::TraitMethod
            ) && canonical_trait_names.get(&entry.name) == Some(&1)
            {
                // Trait canonical paths stay visible as `Eq` / `Eq::eq`.
                // Bare method helpers like `eq` are injected only by import/prelude.
                scope.define_with_id(&entry.name, *uid);
            }
            if let Some(alias) = builtin_special_variant_bare_alias(entry) {
                scope.define_with_id(alias, *uid);
            }
        }
    }
    scope
}

pub(super) fn build_module_scope(
    global_scope: &Scope,
    auto_import_modules: &[String],
    declaration_index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
    declaration_uid_kinds: &HashMap<u32, DeclarationKind>,
    stmts: &[Ast],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> Result<Scope, ResolveError> {
    build_module_scope_with_imports(
        global_scope,
        auto_import_modules,
        declaration_index,
        declaration_uids,
        declaration_uid_kinds,
        stmts,
        current_module_path,
        current_stage_index,
    )
    .map(|build| build.scope)
}

pub(super) struct ModuleScopeBuild {
    pub scope: Scope,
    pub explicit_function_imports: Vec<ExplicitFunctionImport>,
    pub effective_auto_import_fq_names: Vec<String>,
    pub shadowed_auto_import_bindings: Vec<(String, u32)>,
}

pub(super) fn build_module_scope_with_imports(
    global_scope: &Scope,
    auto_import_modules: &[String],
    declaration_index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
    declaration_uid_kinds: &HashMap<u32, DeclarationKind>,
    stmts: &[Ast],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> Result<ModuleScopeBuild, ResolveError> {
    let mut scope = global_scope.clone();
    let mut import_state = ImportState::default();
    let mut explicit_function_imports = Vec::new();
    let mut effective_auto_import_fq_names = Vec::new();
    let mut shadowed_auto_import_bindings = Vec::new();
    let auto_import_traits = auto_import_trait_names(declaration_index);
    let auto_import_module_set = auto_import_modules
        .iter()
        .map(|name| name.as_str())
        .collect::<HashSet<_>>();
    let mut import_context = ImportContext {
        auto_import_modules: &auto_import_module_set,
        declaration_index,
        declaration_uids,
        declaration_uid_kinds,
        current_stage_index,
        auto_import_traits: &auto_import_traits,
        import_state: &mut import_state,
        explicit_function_imports: &mut explicit_function_imports,
        effective_auto_import_fq_names: &mut effective_auto_import_fq_names,
        shadowed_auto_import_bindings: &mut shadowed_auto_import_bindings,
    };

    for stmt in stmts {
        if let Ast::Import(span, path, spec) = stmt {
            apply_import_to_scope(&mut scope, &mut import_context, path, spec, span.clone())?;
        }
    }

    for auto_import in auto_import_modules {
        if current_module_path == Some(auto_import.as_str()) {
            continue;
        }
        import_module_into_scope(
            &mut scope,
            &mut import_context,
            auto_import,
            true,
            Span { start: 0, end: 0 },
        )?;
    }
    for auto_import in &auto_import_traits {
        import_trait_into_scope(
            &mut scope,
            &mut import_context,
            auto_import,
            true,
            Span { start: 0, end: 0 },
        )?;
    }

    if let Some(module_path) = current_module_path {
        for entry in declaration_index.values() {
            if entry.module_path == module_path {
                if !is_module_visible_declaration(&entry.kind) {
                    continue;
                }
                if let Some(uid) = declaration_uids.get(&entry.fq_name) {
                    scope.define_with_id(&entry.name, *uid);
                    scope.define_with_id(&entry.fq_name, *uid);
                    if global_surface_name(&entry.name) != entry.name {
                        scope.define_with_id(global_surface_name(&entry.name), *uid);
                    }
                    if global_surface_name(&entry.fq_name) != entry.fq_name {
                        scope.define_with_id(global_surface_name(&entry.fq_name), *uid);
                    }
                    if let Some(alias) = builtin_special_variant_bare_alias(entry) {
                        scope.define_with_id(alias, *uid);
                    }
                }
            }
        }
    }

    Ok(ModuleScopeBuild {
        scope,
        explicit_function_imports,
        effective_auto_import_fq_names,
        shadowed_auto_import_bindings,
    })
}

struct ImportContext<'a> {
    auto_import_modules: &'a HashSet<&'a str>,
    declaration_index: &'a DeclarationIndex,
    declaration_uids: &'a HashMap<String, u32>,
    declaration_uid_kinds: &'a HashMap<u32, DeclarationKind>,
    current_stage_index: usize,
    auto_import_traits: &'a HashSet<String>,
    import_state: &'a mut ImportState,
    explicit_function_imports: &'a mut Vec<ExplicitFunctionImport>,
    effective_auto_import_fq_names: &'a mut Vec<String>,
    shadowed_auto_import_bindings: &'a mut Vec<(String, u32)>,
}

fn lookup_trait_entry<'a>(
    declaration_index: &'a DeclarationIndex,
    trait_name: &str,
) -> Option<&'a DeclarationEntry> {
    match declaration_index.get(trait_name) {
        Some(entry) if entry.kind == DeclarationKind::Trait => Some(entry),
        _ => declaration_index
            .values()
            .find(|entry| entry.kind == DeclarationKind::Trait && entry.name == trait_name),
    }
}

fn special_non_importable_member(
    declaration_index: &DeclarationIndex,
    module_name: &str,
    member_name: &str,
) -> bool {
    matches!(
        declaration_index
            .get(module_name)
            .or_else(|| declaration_index.get(&format!("Global::{module_name}"))),
        Some(entry) if entry.kind == DeclarationKind::Struct && member_name == "deconstruct"
    )
}

fn apply_import_to_scope(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    path: &spire::ast::AstPath,
    spec: &spire::ast::ImportSpec,
    span: Span,
) -> Result<(), ResolveError> {
    let module_name = path.segments.join("::");
    match spec {
        spire::ast::ImportSpec::All => {
            import_module_into_scope(scope, import_context, &module_name, false, span)
        }
        spire::ast::ImportSpec::Single(name) => {
            import_single_into_scope(scope, import_context, &module_name, name, span)
        }
        spire::ast::ImportSpec::List(names) => {
            import_list_into_scope(scope, import_context, &module_name, names, span)
        }
    }
}

#[derive(Debug, Default)]
struct ImportListIssues {
    not_importable: Vec<String>,
    private_functions: Vec<String>,
    unknown_members: Vec<String>,
    hidden_builtins: Vec<String>,
    unavailable_members: Vec<String>,
}

impl ImportListIssues {
    fn is_empty(&self) -> bool {
        self.not_importable.is_empty()
            && self.private_functions.is_empty()
            && self.unknown_members.is_empty()
            && self.hidden_builtins.is_empty()
            && self.unavailable_members.is_empty()
    }

    fn render_message(&self, module_name: &str) -> String {
        let mut sections = vec![format!("Invalid import members in `{module_name}`.")];
        append_import_issue_section(
            &mut sections,
            "Error: not importable members.",
            &self.not_importable,
        );
        append_import_issue_section(
            &mut sections,
            "Error: private functions.",
            &self.private_functions,
        );
        append_import_issue_section(
            &mut sections,
            "Error: unknown import members.",
            &self.unknown_members,
        );
        append_import_issue_section(
            &mut sections,
            "Error: hidden builtins.",
            &self.hidden_builtins,
        );
        append_import_issue_section(
            &mut sections,
            "Error: unavailable import members.",
            &self.unavailable_members,
        );
        sections.join("\n")
    }
}

fn append_import_issue_section(lines: &mut Vec<String>, title: &str, members: &[String]) {
    if members.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(title.to_string());
    lines.extend(members.iter().map(|member| format!("  {member}")));
}

fn import_list_into_scope(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    module_name: &str,
    names: &[String],
    span: Span,
) -> Result<(), ResolveError> {
    let module_exists = import_context
        .declaration_index
        .values()
        .any(|entry| global_surface_name(&entry.module_path) == module_name);
    if !module_exists {
        return Err(ResolveError {
            message: format!("Unknown module import: {}", module_name),
            span,
            related_labels: Vec::new(),
        });
    }

    let mut issues = ImportListIssues::default();
    for name in names {
        import_context
            .import_state
            .record_member_import(module_name, name, &span)?;

        let fq_name = format!("{}::{}", module_name, name);
        let Some(entry) = import_context.declaration_index.values().find(|entry| {
            global_surface_name(&entry.module_path) == module_name
                && (entry.name == *name
                    || entry
                        .name
                        .rsplit("::")
                        .next()
                        .is_some_and(|tail| tail == name))
        }) else {
            if special_non_importable_member(import_context.declaration_index, module_name, name) {
                issues.not_importable.push(fq_name);
                continue;
            }
            issues.unknown_members.push(fq_name);
            continue;
        };

        if special_non_importable_member(import_context.declaration_index, module_name, name) {
            issues.not_importable.push(fq_name);
            continue;
        }
        match declaration_import_surface_status(entry, import_context.current_stage_index) {
            ImportSurfaceStatus::Importable => {}
            ImportSurfaceStatus::NonImportableKind | ImportSurfaceStatus::Restricted => {
                issues.not_importable.push(fq_name);
                continue;
            }
            ImportSurfaceStatus::Hidden => {
                issues.hidden_builtins.push(fq_name);
                continue;
            }
            ImportSurfaceStatus::Private => {
                issues.private_functions.push(fq_name);
                continue;
            }
            ImportSurfaceStatus::FutureStage => {
                issues.unavailable_members.push(fq_name);
                continue;
            }
        }

        bind_import_name(
            scope,
            import_context,
            &entry.name,
            import_context.declaration_uids[&entry.fq_name],
            module_name,
            false,
            span.clone(),
        )?;

        if entry.kind == DeclarationKind::TraitMethod {
            bind_import_name(
                scope,
                import_context,
                name,
                import_context.declaration_uids[&entry.fq_name],
                module_name,
                false,
                span.clone(),
            )?;
        }

        record_explicit_function_import(import_context, entry, name, &span);

        if entry.kind == DeclarationKind::Trait {
            let trait_prefix = format!("{}::", name);
            for method_entry in import_context.declaration_index.values() {
                if global_surface_name(&method_entry.module_path) != module_name
                    || method_entry.kind != DeclarationKind::TraitMethod
                    || !method_entry.name.starts_with(&trait_prefix)
                {
                    continue;
                }
                if method_entry.stage_index > import_context.current_stage_index {
                    issues
                        .unavailable_members
                        .push(method_entry.fq_name.clone());
                    continue;
                }
                bind_import_name(
                    scope,
                    import_context,
                    &method_entry.name,
                    import_context.declaration_uids[&method_entry.fq_name],
                    module_name,
                    false,
                    span.clone(),
                )?;
                if let Some(short_method_name) = method_entry.name.rsplit("::").next() {
                    bind_import_name(
                        scope,
                        import_context,
                        short_method_name,
                        import_context.declaration_uids[&method_entry.fq_name],
                        module_name,
                        false,
                        span.clone(),
                    )?;
                }
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(ResolveError {
            message: issues.render_message(module_name),
            span,
            related_labels: Vec::new(),
        })
    }
}

fn import_module_into_scope(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    module_name: &str,
    auto_import: bool,
    span: Span,
) -> Result<(), ResolveError> {
    if lookup_trait_entry(import_context.declaration_index, module_name).is_some() {
        return import_trait_into_scope(scope, import_context, module_name, auto_import, span);
    }

    if !auto_import {
        import_context
            .import_state
            .record_module_import(module_name, &span)?;
    }
    let mut imported_any = false;
    let mut blocked_by_stage = false;
    for entry in import_context.declaration_index.values() {
        if global_surface_name(&entry.module_path) != module_name {
            continue;
        }
        match declaration_import_surface_status(entry, import_context.current_stage_index) {
            ImportSurfaceStatus::Importable => {}
            ImportSurfaceStatus::FutureStage => {
                blocked_by_stage = true;
                continue;
            }
            ImportSurfaceStatus::NonImportableKind
            | ImportSurfaceStatus::Restricted
            | ImportSurfaceStatus::Hidden
            | ImportSurfaceStatus::Private => continue,
        }
        let uid = import_context.declaration_uids[&entry.fq_name];
        bind_import_name(
            scope,
            import_context,
            &entry.name,
            uid,
            module_name,
            auto_import,
            span.clone(),
        )?;
        imported_any = true;
    }

    if imported_any || (auto_import && import_context.auto_import_modules.contains(module_name)) {
        Ok(())
    } else if blocked_by_stage {
        Err(ResolveError {
            message: format!(
                "Import target `{}` is not available in the current stage",
                module_name
            ),
            span,
            related_labels: Vec::new(),
        })
    } else if matches!(
        import_context
            .declaration_index
            .get(module_name)
            .or_else(|| import_context.declaration_index.get(&format!("Global::{module_name}"))),
        Some(entry) if entry.kind == DeclarationKind::Struct
    ) {
        // Struct declarations stay directly visible by name so `User()` can
        // dispatch by BlockKind, but the type name itself is not importable.
        Err(ResolveError {
            message: format!("Import target `{}` is not importable", module_name),
            span,
            related_labels: Vec::new(),
        })
    } else {
        Err(ResolveError {
            message: format!("Unknown module import: {}", module_name),
            span,
            related_labels: Vec::new(),
        })
    }
}

fn import_trait_into_scope(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    trait_name: &str,
    auto_import: bool,
    span: Span,
) -> Result<(), ResolveError> {
    let Some(entry) = lookup_trait_entry(import_context.declaration_index, trait_name) else {
        if auto_import {
            return Ok(());
        }
        return Err(ResolveError {
            message: format!("Unknown module import: {}", trait_name),
            span,
            related_labels: Vec::new(),
        });
    };

    if entry.kind != DeclarationKind::Trait {
        return Err(ResolveError {
            message: format!("Import target `{}` is not importable", trait_name),
            span,
            related_labels: Vec::new(),
        });
    }

    if entry.stage_index > import_context.current_stage_index {
        if auto_import {
            return Ok(());
        }
        return Err(ResolveError {
            message: format!(
                "Import target `{}` is not available in the current stage",
                trait_name
            ),
            span,
            related_labels: Vec::new(),
        });
    }

    if !auto_import {
        import_context
            .import_state
            .record_module_import(trait_name, &span)?;
    }

    bind_import_name(
        scope,
        import_context,
        &entry.name,
        import_context.declaration_uids[&entry.fq_name],
        trait_name,
        auto_import,
        span.clone(),
    )?;

    let method_prefix = format!("{}::", entry.fq_name);
    for method_entry in import_context.declaration_index.values() {
        if method_entry.kind != DeclarationKind::TraitMethod
            || !method_entry.fq_name.starts_with(&method_prefix)
        {
            continue;
        }
        if method_entry.stage_index > import_context.current_stage_index {
            if auto_import {
                continue;
            }
            return Err(ResolveError {
                message: format!(
                    "Import target `{}` is not available in the current stage",
                    method_entry.fq_name
                ),
                span: span.clone(),
                related_labels: Vec::new(),
            });
        }
        let method_uid = import_context.declaration_uids[&method_entry.fq_name];
        bind_import_name(
            scope,
            import_context,
            &method_entry.name,
            method_uid,
            trait_name,
            auto_import,
            span.clone(),
        )?;
        if let Some(short_method_name) = method_entry.name.rsplit("::").next() {
            bind_import_name(
                scope,
                import_context,
                short_method_name,
                method_uid,
                trait_name,
                auto_import,
                span.clone(),
            )?;
        }
    }

    Ok(())
}

fn record_explicit_function_import(
    import_context: &mut ImportContext<'_>,
    entry: &DeclarationEntry,
    alias: &str,
    span: &Span,
) {
    if !matches!(
        entry.kind,
        DeclarationKind::Def
            | DeclarationKind::Extractor
            | DeclarationKind::TraitMethod
            | DeclarationKind::ImplMethod
    ) {
        return;
    }
    import_context
        .explicit_function_imports
        .push(ExplicitFunctionImport {
            uid: import_context.declaration_uids[&entry.fq_name],
            alias: alias.to_string(),
            fq_name: entry.fq_name.clone(),
            span: span.clone(),
            kind: entry.kind.clone(),
        });
}

fn effective_auto_import_member_kind(kind: &DeclarationKind) -> bool {
    matches!(
        kind,
        DeclarationKind::Def
            | DeclarationKind::Extractor
            | DeclarationKind::TraitMethod
            | DeclarationKind::ImplMethod
            | DeclarationKind::ImplCtorNew
    )
}

fn record_effective_auto_import_binding(
    import_context: &mut ImportContext<'_>,
    uid: u32,
    short_name: &str,
) {
    let Some((fq_name, entry)) = import_context.declaration_uids.iter().find_map(|(fq_name, known_uid)| {
        if *known_uid != uid {
            return None;
        }
        import_context
            .declaration_index
            .get(fq_name)
            .map(|entry| (fq_name.clone(), entry))
    }) else {
        return;
    };
    if short_name != entry.name || !effective_auto_import_member_kind(&entry.kind) {
        return;
    }
    if !import_context
        .effective_auto_import_fq_names
        .contains(&fq_name)
    {
        import_context.effective_auto_import_fq_names.push(fq_name);
    }
}

fn import_single_into_scope(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    module_name: &str,
    name: &str,
    span: Span,
) -> Result<(), ResolveError> {
    import_context
        .import_state
        .record_member_import(module_name, name, &span)?;

    let fq_name = format!("{}::{}", module_name, name);
    let Some(entry) = import_context.declaration_index.values().find(|entry| {
        global_surface_name(&entry.module_path) == module_name
            && (entry.name == name
                || entry
                    .name
                    .rsplit("::")
                    .next()
                    .is_some_and(|tail| tail == name))
    }) else {
        if special_non_importable_member(import_context.declaration_index, module_name, name) {
            return Err(ResolveError {
                message: format!("Import target `{}` is not importable", fq_name),
                span,
                related_labels: Vec::new(),
            });
        }
        let module_exists = import_context
            .declaration_index
            .values()
            .any(|entry| global_surface_name(&entry.module_path) == module_name);
        return Err(ResolveError {
            message: if module_exists {
                format!("Unknown import member: {}", fq_name)
            } else {
                format!("Unknown module import: {}", module_name)
            },
            span,
            related_labels: Vec::new(),
        });
    };

    if special_non_importable_member(import_context.declaration_index, module_name, name) {
        return Err(ResolveError {
            message: format!("Import target `{}` is not importable", fq_name),
            span,
            related_labels: Vec::new(),
        });
    }

    match declaration_import_surface_status(entry, import_context.current_stage_index) {
        ImportSurfaceStatus::Importable => {}
        ImportSurfaceStatus::NonImportableKind => {
            return Err(ResolveError {
                message: format!("Import target `{}` is not importable", fq_name),
                span,
                related_labels: Vec::new(),
            });
        }
        ImportSurfaceStatus::Restricted => {
            return Err(ResolveError {
                message: restricted_surface_import_message(&fq_name),
                span,
                related_labels: Vec::new(),
            });
        }
        ImportSurfaceStatus::Hidden => {
            return Err(ResolveError {
                message: hidden_builtin_import_message(&fq_name),
                span,
                related_labels: Vec::new(),
            });
        }
        ImportSurfaceStatus::Private => {
            return Err(ResolveError {
                message: format!("Import target `{}` is private", fq_name),
                span,
                related_labels: Vec::new(),
            });
        }
        ImportSurfaceStatus::FutureStage => {
            return Err(ResolveError {
                message: format!(
                    "Import target `{}` is not available in the current stage",
                    fq_name
                ),
                span,
                related_labels: Vec::new(),
            });
        }
    }

    bind_import_name(
        scope,
        import_context,
        &entry.name,
        import_context.declaration_uids[&entry.fq_name],
        module_name,
        false,
        span.clone(),
    )?;

    if entry.kind == DeclarationKind::TraitMethod {
        bind_import_name(
            scope,
            import_context,
            name,
            import_context.declaration_uids[&entry.fq_name],
            module_name,
            false,
            span.clone(),
        )?;
    }

    record_explicit_function_import(import_context, entry, name, &span);

    if entry.kind == DeclarationKind::Trait {
        let trait_prefix = format!("{}::", name);
        for method_entry in import_context.declaration_index.values() {
            if global_surface_name(&method_entry.module_path) != module_name
                || method_entry.kind != DeclarationKind::TraitMethod
                || !method_entry.name.starts_with(&trait_prefix)
            {
                continue;
            }
            if method_entry.stage_index > import_context.current_stage_index {
                return Err(ResolveError {
                    message: format!(
                        "Import target `{}` is not available in the current stage",
                        method_entry.fq_name
                    ),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
            bind_import_name(
                scope,
                import_context,
                &method_entry.name,
                import_context.declaration_uids[&method_entry.fq_name],
                module_name,
                false,
                span.clone(),
            )?;
            if let Some(short_method_name) = method_entry.name.rsplit("::").next() {
                bind_import_name(
                    scope,
                    import_context,
                    short_method_name,
                    import_context.declaration_uids[&method_entry.fq_name],
                    module_name,
                    false,
                    span.clone(),
                )?;
            }
        }
    }

    Ok(())
}

fn bind_import_name(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    short_name: &str,
    uid: u32,
    module_name: &str,
    auto_import: bool,
    span: Span,
) -> Result<(), ResolveError> {
    if let Some(existing_uid) = scope.lookup(short_name) {
        if existing_uid == uid {
            if auto_import {
                record_effective_auto_import_binding(import_context, uid, short_name);
            }
            return Ok(());
        }
        if auto_import
            && !import_context
                .declaration_uid_kinds
                .contains_key(&existing_uid)
        {
            scope.define_with_id(short_name, uid);
            record_effective_auto_import_binding(import_context, uid, short_name);
            return Ok(());
        }
        if auto_import
            && module_name == "Result"
            && matches!(short_name, "Ok" | "Err")
            && !import_context
                .declaration_uid_kinds
                .contains_key(&existing_uid)
        {
            scope.define_with_id(short_name, uid);
            record_effective_auto_import_binding(import_context, uid, short_name);
            return Ok(());
        }
        if auto_import
            && module_name == "Show"
            && short_name == "to_string"
            && !import_context
                .declaration_uid_kinds
                .contains_key(&existing_uid)
        {
            scope.define_with_id(short_name, uid);
            record_effective_auto_import_binding(import_context, uid, short_name);
            return Ok(());
        }
        if auto_import {
            let existing_name = import_context
                .declaration_uids
                .iter()
                .find_map(|(fq_name, known_uid)| (*known_uid == existing_uid).then_some(fq_name))
                .cloned()
                .unwrap_or_else(|| format!("<uid:{}>", existing_uid));
            let existing_is_auto_imported =
                declaration_is_auto_imported(import_context, &existing_name);
            if !existing_is_auto_imported {
                import_context
                    .shadowed_auto_import_bindings
                    .push((short_name.to_string(), existing_uid));
                return Ok(());
            }
            let incoming_name = import_context
                .declaration_uids
                .iter()
                .find_map(|(fq_name, known_uid)| (*known_uid == uid).then_some(fq_name))
                .cloned()
                .unwrap_or_else(|| format!("{}::{}", module_name, short_name));
            if is_result_facet_chain_conflict(&existing_name, &incoming_name, short_name) {
                // Keep bare `chain` bound to Result::chain and let Scar reinterpret
                // Facet-shaped `chain(...)` calls as Facet::chain.
                return Ok(());
            }
            return Err(ResolveError {
                message: format!(
                    "Auto-import conflict for `{}` between `{}` and `{}`",
                    short_name, existing_name, incoming_name
                ),
                span,
                related_labels: Vec::new(),
            });
        }
        let existing_name = import_context
            .declaration_uids
            .iter()
            .find_map(|(fq_name, known_uid)| (*known_uid == existing_uid).then_some(fq_name))
            .cloned()
            .unwrap_or_else(|| format!("<uid:{}>", existing_uid));
        let existing_is_auto_imported =
            declaration_is_auto_imported(import_context, &existing_name);
        if existing_is_auto_imported {
            scope.define_with_id(short_name, uid);
            import_context
                .shadowed_auto_import_bindings
                .push((short_name.to_string(), uid));
            record_effective_auto_import_binding(import_context, uid, short_name);
            return Ok(());
        }
        return Err(ResolveError {
            message: format!(
                "Import conflict for `{}` from module `{}`",
                short_name, module_name
            ),
            span,
            related_labels: Vec::new(),
        });
    }

    scope.define_with_id(short_name, uid);
    if auto_import {
        record_effective_auto_import_binding(import_context, uid, short_name);
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct ImportState {
    imported_modules: HashSet<String>,
    imported_members: HashSet<(String, String)>,
}

impl ImportState {
    fn record_module_import(&mut self, module_name: &str, span: &Span) -> Result<(), ResolveError> {
        if self.imported_modules.contains(module_name)
            || self
                .imported_members
                .iter()
                .any(|(module, _)| module == module_name)
        {
            return Err(ResolveError {
                message: format!("Duplicate import: {}", module_name),
                span: span.clone(),
                related_labels: Vec::new(),
            });
        }
        self.imported_modules.insert(module_name.to_string());
        Ok(())
    }

    fn record_member_import(
        &mut self,
        module_name: &str,
        name: &str,
        span: &Span,
    ) -> Result<(), ResolveError> {
        let member = (module_name.to_string(), name.to_string());
        if self.imported_modules.contains(module_name) || self.imported_members.contains(&member) {
            return Err(ResolveError {
                message: format!("Duplicate import: {}::{}", module_name, name),
                span: span.clone(),
                related_labels: Vec::new(),
            });
        }
        self.imported_members.insert(member);
        Ok(())
    }
}
