use super::declarations::{is_importable_declaration, is_module_visible_declaration};
use super::scope_init::initialize_scope;
use super::*;
use spire::ast::Visibility;

fn auto_import_trait_names(declaration_index: &DeclarationIndex) -> HashSet<String> {
    declaration_index
        .values()
        .filter(|entry| entry.kind == DeclarationKind::Trait && entry.auto_import)
        .map(|entry| entry.name.clone())
        .collect()
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
            scope.define_with_id(fq_name, *uid);
            if matches!(
                entry.kind,
                DeclarationKind::Trait | DeclarationKind::TraitMethod
            ) && canonical_trait_names.get(&entry.name) == Some(&1)
            {
                // Trait canonical paths stay visible as `Eq` / `Eq::eq`.
                // Bare method helpers like `eq` are injected only by import/prelude.
                scope.define_with_id(&entry.name, *uid);
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
    let mut scope = global_scope.clone();
    let mut import_state = ImportState::default();
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
                }
            }
        }
    }

    Ok(scope)
}

struct ImportContext<'a> {
    auto_import_modules: &'a HashSet<&'a str>,
    declaration_index: &'a DeclarationIndex,
    declaration_uids: &'a HashMap<String, u32>,
    declaration_uid_kinds: &'a HashMap<u32, DeclarationKind>,
    current_stage_index: usize,
    auto_import_traits: &'a HashSet<String>,
    import_state: &'a mut ImportState,
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
            for name in names {
                import_single_into_scope(scope, import_context, &module_name, name, span.clone())?;
            }
            Ok(())
        }
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
        if entry.module_path != module_name {
            continue;
        }
        if !is_importable_declaration(&entry.kind) {
            continue;
        }
        if entry.visibility != Visibility::Public {
            continue;
        }
        if entry.stage_index > import_context.current_stage_index {
            blocked_by_stage = true;
            continue;
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
        import_context.declaration_index.get(module_name),
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
    let Some(entry) = import_context.declaration_index.get(&fq_name) else {
        let module_exists = import_context
            .declaration_index
            .values()
            .any(|entry| entry.module_path == module_name);
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

    if !is_importable_declaration(&entry.kind) {
        return Err(ResolveError {
            message: format!("Import target `{}` is not importable", fq_name),
            span,
            related_labels: Vec::new(),
        });
    }
    if entry.visibility != Visibility::Public {
        return Err(ResolveError {
            message: format!("Import target `{}` is private", fq_name),
            span,
            related_labels: Vec::new(),
        });
    }

    if entry.stage_index > import_context.current_stage_index {
        return Err(ResolveError {
            message: format!(
                "Import target `{}` is not available in the current stage",
                fq_name
            ),
            span,
            related_labels: Vec::new(),
        });
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

    if entry.kind == DeclarationKind::Trait {
        let trait_prefix = format!("{}::", name);
        for method_entry in import_context.declaration_index.values() {
            if method_entry.module_path != module_name
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
    import_context: &ImportContext<'_>,
    short_name: &str,
    uid: u32,
    module_name: &str,
    auto_import: bool,
    span: Span,
) -> Result<(), ResolveError> {
    if let Some(existing_uid) = scope.lookup(short_name) {
        if existing_uid == uid {
            return Ok(());
        }
        if auto_import
            && !import_context
                .declaration_uid_kinds
                .contains_key(&existing_uid)
        {
            scope.define_with_id(short_name, uid);
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
            return Ok(());
        }
        if auto_import {
            let existing_name = import_context
                .declaration_uids
                .iter()
                .find_map(|(fq_name, known_uid)| (*known_uid == existing_uid).then_some(fq_name))
                .cloned()
                .unwrap_or_else(|| format!("<uid:{}>", existing_uid));
            let existing_is_auto_imported = import_context
                .declaration_index
                .get(&existing_name)
                .is_some_and(|entry| {
                    entry.auto_import
                        || import_context
                            .auto_import_modules
                            .contains(entry.module_path.as_str())
                        || import_context
                            .auto_import_traits
                            .contains(&entry.module_path)
                });
            if !existing_is_auto_imported {
                return Ok(());
            }
            let incoming_name = import_context
                .declaration_uids
                .iter()
                .find_map(|(fq_name, known_uid)| (*known_uid == uid).then_some(fq_name))
                .cloned()
                .unwrap_or_else(|| format!("{}::{}", module_name, short_name));
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
        let existing_is_auto_imported = import_context
            .declaration_index
            .get(&existing_name)
            .is_some_and(|entry| {
                entry.auto_import
                    || import_context
                        .auto_import_modules
                        .contains(entry.module_path.as_str())
                    || import_context
                        .auto_import_traits
                        .contains(&entry.module_path)
            });
        if existing_is_auto_imported {
            scope.define_with_id(short_name, uid);
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
