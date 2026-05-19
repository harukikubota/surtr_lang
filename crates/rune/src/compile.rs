use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use diagnostics::{SourceId, SourceRegistry};
use forge::bytecode::{
    populate_error_template_lines, stable_hash_hex, synthesize_source_map, SourceFileEntry,
};
use sindr::policy::EntryPoint;
use spire::ast::{Ast, Span};
use spire::error::ParseError;

use crate::error::{ExecutionEnv, RuneError, RuneResult};

type SharedScriptCompilePrefix = Arc<xldr::CompilationPrefixSnapshot>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScriptCompilePlan {
    pub(crate) source_for_parse: String,
    pub(crate) selected_entry_name: Option<String>,
    pub(crate) normalized_entrypoint: Option<EntryPoint>,
    pub(crate) include_directives: Vec<xldr::ScriptIncludeDirective>,
    pub(crate) include_modules: Vec<xldr::ModuleInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScriptPlanError {
    pub(crate) message: String,
    pub(crate) span: Span,
}

impl ScriptPlanError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

pub(crate) fn script_plan_error_as_rune_error(
    file_path: &str,
    source: &str,
    error: ScriptPlanError,
) -> RuneError {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register(file_path, source.to_string());
    RuneError::diagnostic(
        1,
        &sources,
        source_id,
        "parse",
        diagnostics::parse_error_spec(source, &error.message, error.span),
    )
}

fn load_error_span(source: &str) -> Span {
    let len = source.chars().count();
    if len == 0 {
        return Span { start: 0, end: 0 };
    }
    let start = source
        .chars()
        .position(|ch| !ch.is_ascii_whitespace())
        .unwrap_or(0);
    Span {
        start,
        end: (start + 1).min(len),
    }
}

fn module_source_collection_error_as_rune_error(
    file_path: &str,
    source: &str,
    message: impl Into<String>,
) -> RuneError {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register(file_path, source.to_string());
    RuneError::diagnostic(
        1,
        &sources,
        source_id,
        "resolve",
        diagnostics::simple_error("LoadError", message, load_error_span(source), None),
    )
}

fn source_id_for_span(compile_sources: &xldr::CompileSources, span: &Span) -> SourceId {
    if let Some(source) = compile_sources
        .sources
        .source(compile_sources.user_source_id)
    {
        if source.chars().count() >= span.end {
            return compile_sources.user_source_id;
        }
    }

    let mut candidates = compile_sources.module_source_ids.clone();
    candidates.push(compile_sources.user_source_id);
    candidates.sort_by_key(|id| id.0);
    candidates.dedup_by_key(|id| id.0);

    let mut best_code: Option<(SourceId, usize)> = None;
    let mut best_any: Option<(SourceId, usize)> = None;

    for source_id in candidates {
        let Some(source) = compile_sources.sources.source(source_id) else {
            continue;
        };
        let chars: Vec<char> = source.chars().collect();
        let len = chars.len();
        if len < span.end {
            continue;
        }
        let is_code = chars
            .get(span.start)
            .is_some_and(|ch| !ch.is_ascii_whitespace());
        if is_code {
            match best_code {
                None => best_code = Some((source_id, len)),
                Some((_, best_len)) if len < best_len => best_code = Some((source_id, len)),
                _ => {}
            }
        }
        match best_any {
            None => best_any = Some((source_id, len)),
            Some((_, best_len)) if len < best_len => best_any = Some((source_id, len)),
            _ => {}
        }
    }

    best_code
        .or(best_any)
        .map(|(source_id, _)| source_id)
        .unwrap_or(compile_sources.user_source_id)
}

fn diagnostic_location_for_span(
    compile_sources: &xldr::CompileSources,
    span: &Span,
) -> (SourceId, Span) {
    if let Some((source_id, local_span)) = xldr::decode_rebased_module_span(span) {
        if compile_sources.sources.get(source_id).is_some() {
            return (source_id, local_span);
        }
    }
    (source_id_for_span(compile_sources, span), span.clone())
}

fn impl_header_span(source: &str, span: &Span) -> Span {
    let chars = source.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return span.clone();
    }
    let anchor = span.start.min(chars.len().saturating_sub(1));
    let line_start = chars[..anchor]
        .iter()
        .rposition(|ch| *ch == '\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let line_end = chars[anchor..]
        .iter()
        .position(|ch| *ch == '\n')
        .map(|idx| anchor + idx)
        .unwrap_or(chars.len());
    let mut header_start = line_start;
    while header_start < line_end && chars[header_start].is_whitespace() {
        header_start += 1;
    }
    let mut header_end = (line_start..line_end)
        .find(|idx| chars[*idx] == '{')
        .unwrap_or(line_end);
    while header_end > header_start && chars[header_end - 1].is_whitespace() {
        header_end -= 1;
    }
    if header_start < header_end {
        Span {
            start: header_start,
            end: header_end,
        }
    } else {
        span.clone()
    }
}

fn resolve_spec_for_error(
    compile_sources: &xldr::CompileSources,
    error: &sigil::error::ResolveError,
) -> (SourceId, diagnostics::DiagnosticSpec) {
    let (source_id, span) = diagnostic_location_for_span(compile_sources, &error.span);
    let source = compile_sources.sources.source(source_id).unwrap_or("");
    let primary_span = if error.message.starts_with("Multiple impl blocks for `")
        || error
            .message
            .starts_with("Multiple trait impl blocks for `")
    {
        impl_header_span(source, &span)
    } else {
        span
    };
    let mut spec = diagnostics::resolve_error_spec(source, &error.message, primary_span.clone());
    for related in &error.related_labels {
        let (label_source_id, label_span) =
            diagnostic_location_for_span(compile_sources, &related.span);
        let label_source = compile_sources
            .sources
            .source(label_source_id)
            .unwrap_or("");
        let label_span = if error.message.starts_with("Multiple impl blocks for `")
            || error
                .message
                .starts_with("Multiple trait impl blocks for `")
        {
            impl_header_span(label_source, &label_span)
        } else {
            label_span
        };
        spec.labels.push(diagnostics::DiagnosticLabel {
            source_id: Some(label_source_id),
            span: label_span,
            message: related.message.clone(),
            color: Some(diagnostics::Color::Red),
        });
    }
    (source_id, spec)
}

pub(crate) fn collect_default_script_compile_sources(
    env: ExecutionEnv,
    file_path: &str,
    source: &str,
    include_modules: &[xldr::ModuleInput],
    stdlib_variant: xldr::StdlibVariant,
) -> RuneResult<xldr::CompileSources> {
    let module_inputs = xldr::cached_additional_default_std_module_inputs().map_err(|e| {
        module_source_collection_error_as_rune_error(
            file_path,
            source,
            format!(
                "{}: failed to collect definition sources: {}",
                env.command_name(),
                e
            ),
        )
    })?;

    let mut module_input_stages = vec![module_inputs];
    for module_input in include_modules {
        module_input_stages.push(vec![module_input.clone()]);
    }

    let module_sources =
        xldr::collect_module_sources_with_stdlib_variant(stdlib_variant, &[], &module_input_stages)
            .map_err(|e| {
                module_source_collection_error_as_rune_error(
                    file_path,
                    source,
                    format!(
                        "{}: failed to collect definition sources: {}",
                        env.command_name(),
                        e
                    ),
                )
            })?;
    Ok(xldr::compose_script_compile_sources_with_stdlib_variant(
        file_path,
        source,
        module_sources,
        stdlib_variant,
    ))
}

fn load_default_stdlib_snapshot(
    env: ExecutionEnv,
    sources: &xldr::CompileSources,
) -> RuneResult<std::sync::Arc<xldr::DefaultStdlibSnapshot>> {
    let user_source_id = sources.user_source_id;
    let snapshot = match sources.stdlib_variant {
        xldr::StdlibVariant::Default => xldr::default_stdlib_semantic_snapshot(),
        xldr::StdlibVariant::TestEnabled => xldr::test_enabled_stdlib_semantic_snapshot(),
    };
    snapshot.map_err(|e| {
        module_source_collection_error_as_rune_error(
            sources
                .sources
                .file_name(user_source_id)
                .unwrap_or("<script>"),
            sources.sources.source(user_source_id).unwrap_or(""),
            format!(
                "{}: failed to load stdlib snapshot: {}",
                env.command_name(),
                e
            ),
        )
    })
}

fn cached_test_prefix_root() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target")
        .join("surtr-test-cache")
        .join("prefix")
}

fn script_prefix_typecheck_context(
    compile_unit_kind: sindr::policy::CompileUnitKind,
) -> scar::TypecheckContext {
    scar::TypecheckContext {
        runtime_policy: xldr::derive_runtime_policy(
            compile_unit_kind,
            xldr::SourceKind::Script,
            None,
        ),
        enforce_builtin_type_contracts: false,
        allow_error_function_params: false,
    }
}

fn build_cached_script_compile_prefix(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
    std_snapshot: &xldr::DefaultStdlibSnapshot,
    module_stages: &[Vec<sigil::StagedModuleAst>],
    sources: &SourceRegistry,
) -> RuneResult<SharedScriptCompilePrefix> {
    let rebuilt_declaration_index =
        sigil::precollect_declaration_index(module_stages).map_err(|e| {
            let (source_id, spec) = resolve_spec_for_error(compile_sources, &e);
            RuneError::diagnostic(1, sources, source_id, "resolve", spec)
        })?;

    let cache_key = xldr::test_semantic_prefix_cache_key(env.compile_unit_kind(), compile_sources)
        .map_err(|e| {
            RuneError::message(
                1,
                format!("test: failed to build semantic prefix key: {}", e),
            )
        })?;
    let cache_path = cached_test_prefix_root().join(format!("{cache_key}.semantic"));

    static PREFIX_CACHE: OnceLock<
        Mutex<HashMap<String, Result<SharedScriptCompilePrefix, String>>>,
    > = OnceLock::new();
    let cache = PREFIX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache
        .lock()
        .expect("script compile prefix cache poisoned")
        .get(&cache_key)
    {
        return cached
            .clone()
            .map_err(|message| RuneError::message(1, message));
    }

    let prefix = if let Some(payload) =
        xldr::load_cached_test_semantic_prefix(&cache_path, &cache_key)
    {
        Arc::new(xldr::CompilationPrefixSnapshot::from_parts(
            rebuilt_declaration_index.clone(),
            payload.resolve_state,
            payload.scar_checkpoint,
            payload.bytecode,
        ))
    } else {
        let resolved = sigil::resolve_staged_program_from_state(
            module_stages,
            Vec::new(),
            &rebuilt_declaration_index,
            None,
            std_snapshot.default_stage_count,
            std_snapshot.resolve_state(),
        )
        .map_err(|e| {
            let (source_id, spec) = resolve_spec_for_error(compile_sources, &e);
            RuneError::diagnostic(1, sources, source_id, "resolve", spec)
        })?;
        let resume_state = resolved.resume_state;
        let mut scar_session = std_snapshot.compile_prefix().restored_scar_session();
        let typed = scar_session
            .typecheck_staged_program_with_context(
                resolved,
                script_prefix_typecheck_context(env.compile_unit_kind()),
            )
            .map_err(|e| {
                let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
                let local_error = diagnostics::TypeErrorDiagnostic::new(e.message, span, e.hint);
                RuneError::diagnostic(
                    1,
                    sources,
                    source_id,
                    "typecheck",
                    diagnostics::type_error_spec_by_id(sources, source_id, &local_error),
                )
            })?;
        let mut forge_session = std_snapshot.compile_prefix().forge_session();
        let (chunk, _) = forge_session
            .codegen_chunk_typed_program(typed)
            .map_err(|e| {
                let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
                RuneError::diagnostic(
                    1,
                    sources,
                    source_id,
                    "codegen",
                    diagnostics::simple_error("CodegenError", &e.message, span, None),
                )
            })?;
        let bytecode = forge::compose_bytecode_with_chunk(std_snapshot.bytecode().clone(), chunk)
            .map_err(|e| {
                let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
                RuneError::diagnostic(
                    1,
                    sources,
                    source_id,
                    "codegen",
                    diagnostics::simple_error("CodegenError", &e.message, span, None),
                )
            })?;
        scar_session.reconcile_function_indices(bytecode.functions.iter().filter_map(|entry| {
            entry
                .qualified_name
                .as_deref()
                .map(|qualified_name| (qualified_name, entry.fun_idx))
        }));
        let resolve_state = sigil::ResolveResumeState {
            next_local_id: resume_state.next_local_id.max(
                bytecode
                    .functions
                    .iter()
                    .map(|entry| entry.fun_idx.saturating_add(1))
                    .max()
                    .unwrap_or(0),
            ),
        };
        let prefix = Arc::new(xldr::CompilationPrefixSnapshot::from_parts(
            rebuilt_declaration_index.clone(),
            resolve_state,
            scar_session.checkpoint(),
            bytecode,
        ));
        xldr::store_cached_test_semantic_prefix_snapshot(&cache_path, &cache_key, &prefix);
        prefix
    };

    cache
        .lock()
        .expect("script compile prefix cache poisoned")
        .insert(cache_key, Ok(Arc::clone(&prefix)));
    Ok(prefix)
}

fn parse_program_with_module_sources<'a>(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
    std_snapshot: &'a xldr::DefaultStdlibSnapshot,
) -> RuneResult<(
    std::borrow::Cow<'a, [Vec<sigil::StagedModuleAst>]>,
    Vec<spire::ast::Ast>,
)> {
    let compile_unit_kind = env.compile_unit_kind();
    let source_kind = env.source_kind();
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let expanded =
        xldr::expand_snapshot_module_stages(compile_sources, std_snapshot, compile_unit_kind)
            .map_err(|e| {
        RuneError::diagnostic(
            1,
            sources,
            e.source_id,
            "parse",
            diagnostics::parse_error_spec(
                sources.source(e.source_id).unwrap_or(""),
                e.message(),
                e.span(),
            ),
        )
    })?;

    let user_source = sources.source(user_source_id).unwrap_or("");
    let user_ast = parse_script_ast_for_compile(user_source, user_source_id.0, source_kind)
        .map_err(|script_err: ParseError| {
            RuneError::diagnostic(
                1,
                sources,
                user_source_id,
                "parse",
                diagnostics::parse_error_spec(
                    user_source,
                    script_err.message(),
                    script_err.span().clone(),
                ),
            )
        })?;

    Ok((expanded.module_stages, user_ast))
}

pub(crate) fn compile_source(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
    compile_plan: &ScriptCompilePlan,
) -> RuneResult<forge::bytecode::Bytecode> {
    let compile_unit_kind = env.compile_unit_kind();
    let source_kind = env.source_kind();
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let user_source = sources.source(user_source_id).unwrap_or("");

    let std_snapshot = load_default_stdlib_snapshot(env, compile_sources)?;
    let (mut module_stages, mut user_ast) =
        parse_program_with_module_sources(env, compile_sources, &std_snapshot)?;
    let (script_process_stage, script_user_ast) =
        xldr::extract_process_modules_from_user_ast(user_ast);
    let has_script_process_stage = !script_process_stage.is_empty();
    user_ast = script_user_ast;
    if has_script_process_stage {
        module_stages.to_mut().push(script_process_stage);
    }
    if let Some(entry_name) = compile_plan.selected_entry_name.as_deref() {
        user_ast = rewrite_script_ast_for_entry(user_ast, entry_name);
    }
    let suffix_module_stages = if module_stages.len() > std_snapshot.default_stage_count {
        &module_stages[std_snapshot.default_stage_count..]
    } else {
        &[]
    };
    let docs = sigil::collect_doc_entries_with_base(
        &std_snapshot.docs,
        suffix_module_stages,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let signatures = sigil::collect_signature_entries_with_base(
        &std_snapshot.signatures,
        suffix_module_stages,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );

    let mut cached_prefix: Option<SharedScriptCompilePrefix> = None;
    let rebuilt_declaration_index;
    let declaration_index = if module_stages.len() == std_snapshot.default_stage_count {
        std_snapshot.declaration_index()
    } else if matches!(env, ExecutionEnv::Test) && !has_script_process_stage {
        cached_prefix = Some(build_cached_script_compile_prefix(
            env,
            compile_sources,
            &std_snapshot,
            &module_stages,
            sources,
        )?);
        let Some(prefix) = cached_prefix.as_ref() else {
            return Err(RuneError::usage("test prefix cache was not built"));
        };
        &prefix.declaration_index
    } else {
        rebuilt_declaration_index =
            sigil::precollect_declaration_index(&module_stages).map_err(|e| {
                let (source_id, spec) = resolve_spec_for_error(compile_sources, &e);
                RuneError::diagnostic(1, sources, source_id, "resolve", spec)
            })?;
        &rebuilt_declaration_index
    };

    let resume_state = cached_prefix
        .as_ref()
        .map(|prefix| prefix.resolve_state)
        .unwrap_or(std_snapshot.resolve_state());
    let start_stage_index = if cached_prefix.is_some() {
        module_stages.len()
    } else {
        std_snapshot.default_stage_count
    };
    let resolved = sigil::resolve_staged_program_from_state(
        &module_stages,
        user_ast,
        declaration_index,
        Some(compile_sources.user_module_path.clone()),
        start_stage_index,
        resume_state,
    )
    .map_err(|e| {
        let (source_id, spec) = resolve_spec_for_error(compile_sources, &e);
        RuneError::diagnostic(1, sources, source_id, "resolve", spec)
    })?;

    let prefix_bytecode = cached_prefix
        .as_ref()
        .map(|prefix| prefix.bytecode.clone())
        .unwrap_or_else(|| std_snapshot.bytecode().clone());
    let active_prefix = cached_prefix
        .as_ref()
        .map(|prefix| prefix.as_ref())
        .unwrap_or_else(|| std_snapshot.compile_prefix());
    let mut scar_session = active_prefix.restored_scar_session();
    let typed = scar_session
        .typecheck_staged_program_with_context(
            resolved,
            scar::TypecheckContext {
                runtime_policy: xldr::derive_runtime_policy(
                    compile_unit_kind,
                    source_kind,
                    compile_plan.normalized_entrypoint.as_ref(),
                ),
                enforce_builtin_type_contracts: false,
                allow_error_function_params: false,
            },
        )
        .map_err(|e| {
            let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
            let local_error = diagnostics::TypeErrorDiagnostic::new(e.message, span, e.hint);
            RuneError::diagnostic(
                1,
                sources,
                source_id,
                "typecheck",
                diagnostics::type_error_spec_by_id(sources, source_id, &local_error),
            )
        })?;

    let mut forge_session = active_prefix.forge_session();
    let (chunk, _) = forge_session
        .codegen_chunk_typed_program(typed)
        .map_err(|e| {
            let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
            RuneError::diagnostic(
                1,
                sources,
                source_id,
                "codegen",
                diagnostics::simple_error("CodegenError", &e.message, span, None),
            )
        })?;
    let mut bytecode = forge::compose_bytecode_with_chunk(prefix_bytecode, chunk).map_err(|e| {
        let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
        RuneError::diagnostic(
            1,
            sources,
            source_id,
            "codegen",
            diagnostics::simple_error("CodegenError", &e.message, span, None),
        )
    })?;

    populate_error_template_lines(&mut bytecode.error_templates, user_source);
    bytecode.docs = docs;
    bytecode.signatures = signatures;
    bytecode.compile_info.bytecode_version = 1;
    bytecode.compile_info.debug_level = 2;
    bytecode.compile_info.num_locals = bytecode.num_locals;
    bytecode.compile_info.build_profile = Some("full".into());
    bytecode.compile_info.source_hash = Some(stable_hash_hex(user_source));
    bytecode.compile_info.module_hash = Some(stable_hash_hex(&compile_sources.user_module_path));
    bytecode.sources = collect_viewer_sources(compile_sources);
    if bytecode.source_map.is_none() {
        bytecode.source_map = synthesize_source_map(
            &bytecode.opcodes,
            &bytecode.functions,
            &bytecode.error_templates,
            &bytecode.dbg_templates,
            user_source,
            Some(
                compile_sources
                    .sources
                    .file_name(user_source_id)
                    .unwrap_or("<script>"),
            ),
        );
    }
    bytecode.refresh_viewer_metadata();

    Ok(bytecode)
}

fn collect_viewer_sources(compile_sources: &xldr::CompileSources) -> Vec<SourceFileEntry> {
    let mut ids = compile_sources.module_source_ids.clone();
    ids.push(compile_sources.user_source_id);
    ids.sort_by_key(|id| id.0);
    ids.dedup_by_key(|id| id.0);

    ids.into_iter()
        .filter_map(|source_id| {
            let entry = compile_sources.sources.get(source_id)?;
            Some(SourceFileEntry {
                source_id: source_id.0,
                path: entry.file_name.clone(),
                normalized_path: Some(entry.file_name.clone()),
                content_hash: Some(stable_hash_hex(&entry.source)),
                text: Some(entry.source.clone()),
            })
        })
        .collect()
}

pub(crate) fn prepare_script_compile_plan(
    file_path: &str,
    source: &str,
    cli_entry: Option<&str>,
) -> Result<ScriptCompilePlan, ScriptPlanError> {
    let prepared = xldr::prepare_script_sources(file_path, source, xldr::SourceKind::Script)
        .map_err(script_source_prepare_error_to_plan_error)?;

    let selected_entry_name = match cli_entry {
        Some(name) => Some(name.to_string()),
        None => None,
    };

    let normalized_entrypoint = selected_entry_name.as_ref().map(|name| {
        EntryPoint::script_short_name(name, xldr::script_pseudo_module_path(file_path))
    });

    Ok(ScriptCompilePlan {
        source_for_parse: prepared.source_for_parse,
        selected_entry_name,
        normalized_entrypoint,
        include_directives: prepared.include_directives,
        include_modules: prepared.include_modules,
    })
}

fn script_source_prepare_error_to_plan_error(
    error: xldr::ScriptSourcePrepareError,
) -> ScriptPlanError {
    match error {
        xldr::ScriptSourcePrepareError::Parse { message, span }
        | xldr::ScriptSourcePrepareError::IncludeRead { message, span } => {
            ScriptPlanError::new(message, span)
        }
    }
}
fn parse_script_ast_for_compile(
    source: &str,
    source_id: u32,
    source_kind: xldr::SourceKind,
) -> Result<Vec<Ast>, ParseError> {
    let strict_context = xldr::derive_parser_context(
        source_id,
        source_kind,
        sindr::policy::CompileUnitKind::Script,
        None,
    );
    spire::parse_with_context(source, strict_context)
}

fn rewrite_script_ast_for_entry(user_ast: Vec<Ast>, entry_name: &str) -> Vec<Ast> {
    let mut out = user_ast
        .into_iter()
        .filter(|stmt| {
            matches!(
                stmt,
                Ast::Def(..)
                    | Ast::ExtractorDef(..)
                    | Ast::TraitDef(_, _, _, _, _)
                    | Ast::TraitImplDef(_, _, _, _, _, _)
                    | Ast::BuiltinDecl(_, _, _, _, _)
                    | Ast::StructDef(..)
                    | Ast::RecordDef(..)
                    | Ast::DeferrorDef(_, _, _, _, _)
                    | Ast::ImplDef(_, _, _, _)
                    | Ast::SupervisorInit(_, _)
                    | Ast::Import(_, _, _)
            )
        })
        .collect::<Vec<_>>();

    let span = Span { start: 0, end: 0 };
    out.push(Ast::App(
        span.clone(),
        Box::new(Ast::Var(span, entry_name.to_string())),
        Vec::new(),
    ));
    out
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        collect_default_script_compile_sources, compile_source, diagnostic_location_for_span,
        load_error_span, module_source_collection_error_as_rune_error,
        parse_script_ast_for_compile, prepare_script_compile_plan, source_id_for_span,
    };
    use crate::error::ExecutionEnv;
    use crate::error::RuneError;
    use spire::ast::Span;
    use xldr::{SourceKind, StagedModule};

    #[test]
    fn script_compile_plan_extracts_include_directives() {
        let temp =
            std::env::temp_dir().join(format!("surtr_script_compile_plan_{}", std::process::id()));
        let fixtures = temp.join("fixtures");
        fs::create_dir_all(&fixtures).expect("test fixture dir should be created");
        fs::write(
            fixtures.join("Helper.srt"),
            r#"
defmod Helper {
  def add(a: Int, b: Int) -> Int { a + b }
}
"#,
        )
        .expect("test include fixture should be written");
        let script_path = temp.join("sample.srt");
        let source = r#"include 'fixtures/Helper.srt'
import Helper::add
print(to_string(add(1, 2)))
"#;

        let plan = prepare_script_compile_plan(
            script_path.to_str().expect("script path must be utf-8"),
            source,
            None,
        )
        .expect("compile plan must succeed");

        assert_eq!(plan.include_directives.len(), 1);
        assert_eq!(plan.include_directives[0].file_path, "fixtures/Helper.srt");
        assert_eq!(plan.include_modules.len(), 1);
        assert_eq!(plan.include_modules[0].module_path, "Global::Helper");
        assert_eq!(plan.source_for_parse.len(), source.len());
        assert!(!plan
            .source_for_parse
            .contains("include 'fixtures/Helper.srt'"));
        assert!(plan.source_for_parse.contains("import Helper::add"));

        let _ = fs::remove_dir_all(temp);
    }

    #[test]
    fn script_compile_plan_rejects_include_non_literal_argument() {
        let source = r#"path = "fixtures/Helper.srt"
include path
"#;

        let err = prepare_script_compile_plan("sample.srt", source, None)
            .expect_err("non-literal include path must fail");
        assert!(err
            .message
            .contains("include expects a string literal path"));
    }

    #[test]
    fn script_compile_plan_rejects_include_in_nested_expression() {
        let source = r#"value = include 'fixtures/Helper.srt'
print(to_string(1))
"#;

        let err = prepare_script_compile_plan("sample.srt", source, None)
            .expect_err("nested include must fail");
        assert!(
            err.message
                .contains("Declarations are only allowed at the top level")
                || err.message.contains("Unexpected token")
        );
    }

    #[test]
    fn parse_script_ast_for_compile_rejects_legacy_definition_after_expression() {
        let err = parse_script_ast_for_compile(
            "print(\"start\")\ndef helper() -> Unit { () }\n",
            0,
            SourceKind::Script,
        )
        .expect_err("legacy script ordering should fail under strict parsing");

        assert!(
            err.message()
                .contains("top-level definition cannot appear after top-level expression"),
            "unexpected error: {}",
            err.message()
        );
    }

    #[test]
    fn load_error_span_uses_first_non_whitespace_character() {
        let span = load_error_span("\n  print(\"ok\")\n");
        assert_eq!(span.start, 3);
        assert_eq!(span.end, 4);
    }

    #[test]
    fn module_source_collection_error_returns_diagnostic_variant() {
        let err = module_source_collection_error_as_rune_error(
            "main.srt",
            "print(\"ok\")\n",
            "run: failed to collect definition sources: broken",
        );
        match err {
            RuneError::Diagnostic { diagnostic, .. } => {
                assert_eq!(diagnostic.phase, "resolve");
                assert_eq!(diagnostic.spec.kind, "LoadError");
                assert!(diagnostic
                    .spec
                    .message
                    .contains("failed to collect definition sources"));
            }
            other => panic!("expected diagnostic error, got {:?}", other),
        }
    }

    #[test]
    fn diagnostic_location_for_span_decodes_rebased_module_span() {
        let module_source = "defmod MahjongCli { def render() -> Unit { () } }";
        let user_source = "MahjongCli::render()";
        let mut sources = diagnostics::SourceRegistry::new();
        let module_source_id = sources.register("examples/mahjong/src/6_cli.srt", module_source);
        let user_source_id = sources.register("examples/mahjong/run.srt", user_source);
        let compile_sources = xldr::CompileSources {
            sources,
            user_source_id,
            user_module_path: "__Script::examples::mahjong::run".into(),
            builtin_source_id: module_source_id,
            builtin_module_path: Some("Bootstrap".into()),
            module_source_ids: vec![module_source_id],
            module_stages: vec![vec![StagedModule {
                source_id: module_source_id,
                module_path: "MahjongCli".into(),
                source_kind: SourceKind::DefinitionSource,
            }]],
            stdlib_variant: xldr::StdlibVariant::Default,
        };
        let base = xldr::module_span_base_for_source(module_source_id);
        let span = Span {
            start: base + 12,
            end: base + 18,
        };
        let (source_id, local_span) = diagnostic_location_for_span(&compile_sources, &span);
        assert_eq!(
            compile_sources.sources.file_name(source_id),
            Some("examples/mahjong/src/6_cli.srt")
        );
        assert_eq!(local_span.start, 12);
        assert_eq!(local_span.end, 18);
    }

    #[test]
    fn source_id_for_span_falls_back_to_user_source_when_module_does_not_cover_span() {
        let module_source = "defmod MahjongCli { def render() -> Unit { () } }";
        let user_source =
            "main = \"................................................................\"";
        let mut sources = diagnostics::SourceRegistry::new();
        let module_source_id = sources.register("examples/mahjong/src/6_cli.srt", module_source);
        let user_source_id = sources.register("examples/mahjong/run.srt", user_source);
        let compile_sources = xldr::CompileSources {
            sources,
            user_source_id,
            user_module_path: "__Script::examples::mahjong::run".into(),
            builtin_source_id: module_source_id,
            builtin_module_path: Some("Bootstrap".into()),
            module_source_ids: vec![module_source_id],
            module_stages: vec![vec![StagedModule {
                source_id: module_source_id,
                module_path: "MahjongCli".into(),
                source_kind: SourceKind::DefinitionSource,
            }]],
            stdlib_variant: xldr::StdlibVariant::Default,
        };
        let span = Span { start: 50, end: 51 };
        let source_id = source_id_for_span(&compile_sources, &span);
        assert_eq!(
            compile_sources.sources.file_name(source_id),
            Some("examples/mahjong/run.srt")
        );
    }

    #[test]
    fn compile_source_accepts_concrete_pid_annotation_in_script_worker_spawn() {
        let source = r#"defagent MyWorker {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }

  @init
  def init(seed: Int) -> Result<Int> { Ok(seed) }

  @get
  def value(state: Int) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}

defsupervisor MySup {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}

supervisor_init {
  MySup {}
}

pid: PID<MyWorker> =? MySup::spawn(MyWorker::init(1))
"#;

        let parsed = parse_script_ast_for_compile(source, 0, SourceKind::Script).unwrap();
        let (process_stage, _) = xldr::extract_process_modules_from_user_ast(parsed);
        let declaration_index = sigil::precollect_declaration_index(&[process_stage.clone()])
            .expect("process declarations should precollect");
        assert!(
            declaration_index
                .keys()
                .any(|key| key == "MyWorker::init" || key == "Global::MyWorker::init"),
            "expected process declaration index to expose MyWorker::init, got keys: {:?}",
            declaration_index.keys().collect::<Vec<_>>()
        );

        let plan = prepare_script_compile_plan("process_pid_annotation.srt", source, None).unwrap();
        let compile_sources = collect_default_script_compile_sources(
            ExecutionEnv::Check,
            "process_pid_annotation.srt",
            &plan.source_for_parse,
            &plan.include_modules,
            xldr::StdlibVariant::Default,
        )
        .unwrap();

        let compiled = compile_source(ExecutionEnv::Check, &compile_sources, &plan);
        assert!(
            compiled.is_ok(),
            "expected concrete PID annotation script to compile, got {compiled:?}"
        );
    }
}
