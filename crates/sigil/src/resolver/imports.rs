use super::declarations::{is_importable_declaration, is_module_visible_declaration};
use super::scope_init::{initialize_scope, AUTO_IMPORT_MODULES};
use super::*;

pub(super) fn build_global_scope(
    index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
) -> Scope {
    let mut scope = initialize_scope();
    for (fq_name, entry) in index {
        if entry.kind == DeclarationKind::BuiltinType {
            continue;
        }
        if let Some(uid) = declaration_uids.get(fq_name) {
            scope.define_with_id(fq_name, *uid);
        }
    }
    scope
}

pub(super) fn build_module_scope(
    global_scope: &Scope,
    declaration_index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
    declaration_uid_kinds: &HashMap<u32, DeclarationKind>,
    stmts: &[Ast],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> Result<Scope, ResolveError> {
    let mut scope = global_scope.clone();
    let mut import_state = ImportState::default();
    let mut import_context = ImportContext {
        declaration_index,
        declaration_uids,
        declaration_uid_kinds,
        current_stage_index,
        import_state: &mut import_state,
    };

    for stmt in stmts {
        if let Ast::Import(span, path, spec) = stmt {
            apply_import_to_scope(&mut scope, &mut import_context, path, spec, span.clone())?;
        }
    }

    for auto_import in AUTO_IMPORT_MODULES {
        if current_module_path == Some(*auto_import) {
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

    if let Some(module_path) = current_module_path {
        for entry in declaration_index.values() {
            if entry.module_path == module_path {
                if !is_module_visible_declaration(&entry.kind) {
                    continue;
                }
                if let Some(uid) = declaration_uids.get(&entry.fq_name) {
                    scope.define_with_id(&entry.name, *uid);
                }
            }
        }
    }

    Ok(scope)
}

struct ImportContext<'a> {
    declaration_index: &'a DeclarationIndex,
    declaration_uids: &'a HashMap<String, u32>,
    declaration_uid_kinds: &'a HashMap<u32, DeclarationKind>,
    current_stage_index: usize,
    import_state: &'a mut ImportState,
}

fn apply_import_to_scope(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    path: &spire::ast::AstPath,
    spec: &spire::ast::ImportSpec,
    span: Span,
) -> Result<(), ResolveError> {
    let module_name = path.segments.join("::");
    if AUTO_IMPORT_MODULES
        .iter()
        .any(|auto| auto == &module_name.as_str())
    {
        return Err(ResolveError {
            message: format!(
                "Duplicate import: `{}` is auto-imported and cannot be explicitly imported",
                module_name
            ),
            span,
        });
    }
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
        if entry.stage_index >= import_context.current_stage_index {
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

    if imported_any || (auto_import && AUTO_IMPORT_MODULES.contains(&module_name)) {
        Ok(())
    } else if blocked_by_stage {
        Err(ResolveError {
            message: format!(
                "Import target `{}` is not available in the current stage",
                module_name
            ),
            span,
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
        })
    } else {
        Err(ResolveError {
            message: format!("Unknown module import: {}", module_name),
            span,
        })
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
        });
    };

    if !is_importable_declaration(&entry.kind) {
        return Err(ResolveError {
            message: format!("Import target `{}` is not importable", fq_name),
            span,
        });
    }

    if entry.stage_index >= import_context.current_stage_index {
        return Err(ResolveError {
            message: format!(
                "Import target `{}` is not available in the current stage",
                fq_name
            ),
            span,
        });
    }

    bind_import_name(
        scope,
        import_context,
        &entry.name,
        import_context.declaration_uids[&entry.fq_name],
        module_name,
        false,
        span,
    )
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
            && module_name == "Result"
            && matches!(short_name, "Ok" | "Err")
            && !import_context
                .declaration_uid_kinds
                .contains_key(&existing_uid)
        {
            scope.define_with_id(short_name, uid);
            return Ok(());
        }
        if auto_import {
            return Ok(());
        }
        return Err(ResolveError {
            message: format!(
                "Import conflict for `{}` from module `{}`",
                short_name, module_name
            ),
            span,
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
            });
        }
        self.imported_members.insert(member);
        Ok(())
    }
}
