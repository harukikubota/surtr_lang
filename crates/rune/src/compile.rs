use diagnostics::SourceRegistry;
use forge::bytecode::{
    populate_error_template_lines, stable_hash_hex, synthesize_source_map, SourceFileEntry,
};
use spire::ast::{Ast, Span};
use spire::token::Token;

use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::loader::collect_additional_std_module_inputs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptCompilePlan {
    pub(crate) source_for_parse: String,
    pub(crate) selected_entry_name: Option<String>,
    pub(crate) normalized_entrypoint: Option<spire::EntryPoint>,
}

impl ScriptCompilePlan {
    pub(crate) fn plain(source_for_parse: String) -> Self {
        Self {
            source_for_parse,
            selected_entry_name: None,
            normalized_entrypoint: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntryAnnotation {
    pub(crate) name: String,
    pub(crate) span: Span,
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
        diagnostics::simple_error("ParseError", &error.message, error.span, None),
    )
}

pub(crate) fn collect_default_script_compile_sources(
    env: ExecutionEnv,
    file_path: &str,
    source: &str,
) -> RuneResult<xldr::CompileSources> {
    let module_inputs = collect_additional_std_module_inputs(env)?;
    let module_sources = if module_inputs.is_empty() {
        xldr::collect_module_sources_with_module_stages(&[])
    } else {
        xldr::collect_module_sources_with_std_module_stages(&[module_inputs])
    }
    .map_err(|e| {
        RuneError::message(
            1,
            format!(
                "{}: failed to collect module sources: {}",
                env.command_name(),
                e
            ),
        )
    })?;
    Ok(xldr::compose_script_compile_sources(
        file_path,
        source,
        module_sources,
    ))
}

fn parse_program_with_module_sources(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
    entrypoint: Option<&spire::EntryPoint>,
) -> RuneResult<(Vec<Vec<sigil::StagedModuleAst>>, Vec<spire::ast::Ast>)> {
    let compile_unit_kind = env.compile_unit_kind();
    let source_kind = env.source_kind();
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let staged_module_asts =
        xldr::parse_module_stages_from_compile_sources(compile_sources, compile_unit_kind)
            .map_err(|e| {
                RuneError::diagnostic(
                    1,
                    sources,
                    e.source_id,
                    diagnostics::simple_error("ParseError", e.message(), e.span(), None),
                )
            })?;

    let user_source = sources.source(user_source_id).unwrap_or("");
    let user_ast = spire::parse_with_context(
        user_source,
        spire::ParserContext::script(user_source_id.0).with_rules(xldr::derive_source_rules(
            compile_unit_kind,
            source_kind,
            entrypoint,
        )),
    )
    .map_err(|e| {
        RuneError::diagnostic(
            1,
            sources,
            user_source_id,
            diagnostics::simple_error("ParseError", e.message(), e.span().clone(), None),
        )
    })?;

    Ok((staged_module_asts, user_ast))
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

    let (module_stages, mut user_ast) = parse_program_with_module_sources(
        env,
        compile_sources,
        compile_plan.normalized_entrypoint.as_ref(),
    )?;
    if let Some(entry_name) = compile_plan.selected_entry_name.as_deref() {
        user_ast = rewrite_script_ast_for_entry(user_ast, entry_name);
    }
    let docs = xldr::collect_doc_entries(
        &module_stages,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );

    let declaration_index = sigil::precollect_declaration_index(&module_stages).map_err(|e| {
        RuneError::diagnostic(
            1,
            sources,
            user_source_id,
            diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
        )
    })?;

    let resolved = sigil::resolve_staged_program(
        &module_stages,
        user_ast,
        &declaration_index,
        Some(compile_sources.user_module_path.clone()),
    )
    .map_err(|e| {
        RuneError::diagnostic(
            1,
            sources,
            user_source_id,
            diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
        )
    })?;

    let typed = scar::typecheck_with_context(
        resolved,
        scar::TypecheckContext {
            source_rules: xldr::derive_source_rules(
                compile_unit_kind,
                source_kind,
                compile_plan.normalized_entrypoint.as_ref(),
            ),
            enforce_builtin_type_contracts: true,
        },
    )
    .map_err(|e| {
        RuneError::diagnostic(
            1,
            sources,
            user_source_id,
            diagnostics::type_error_spec_by_id(sources, user_source_id, &e),
        )
    })?;

    let mut bytecode = forge::codegen(typed).map_err(|e| {
        RuneError::diagnostic(
            1,
            sources,
            user_source_id,
            diagnostics::simple_error("CodegenError", &e.message, e.span.clone(), None),
        )
    })?;

    populate_error_template_lines(&mut bytecode.error_templates, user_source);
    bytecode.docs = docs;
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
    let source_without_tests = xldr::strip_test_annotations(source);
    let (source_for_parse, annotations) = collect_entrypoint_annotations(&source_without_tests)?;

    if annotations.len() > 1 {
        let second = &annotations[1];
        return Err(ScriptPlanError::new(
            format!(
                "multiple @@entrypoint annotations are not allowed (already declared as `{}`)",
                annotations[0].name
            ),
            second.span.clone(),
        ));
    }

    let selected_entry_name = match cli_entry {
        Some(name) => Some(name.to_string()),
        None => annotations.first().map(|a| a.name.clone()),
    };

    let normalized_entrypoint = selected_entry_name.as_ref().map(|name| {
        spire::EntryPoint::script_short_name(name, xldr::script_pseudo_module_path(file_path))
    });

    Ok(ScriptCompilePlan {
        source_for_parse,
        selected_entry_name,
        normalized_entrypoint,
    })
}

pub(crate) fn collect_entrypoint_annotations(
    source: &str,
) -> Result<(String, Vec<EntryAnnotation>), ScriptPlanError> {
    let tokens = spire::lexer::tokenize(source)
        .map_err(|e| ScriptPlanError::new(e.message().to_string(), e.span().clone()))?;
    let mut chars = source.chars().collect::<Vec<_>>();
    let mut annotations = Vec::new();

    let mut i = 0usize;
    while i < tokens.len() {
        let token = &tokens[i];
        if let Token::Annotator(name) = &token.token {
            if name == "entrypoint" {
                erase_span(&mut chars, &token.span);
                let mut j = i + 1;
                while j < tokens.len() && matches!(tokens[j].token, Token::Newline) {
                    j += 1;
                }
                if j >= tokens.len() || !matches!(tokens[j].token, Token::Def) {
                    return Err(ScriptPlanError::new(
                        "@@entrypoint must annotate a function definition (`def`)",
                        token.span.clone(),
                    ));
                }
                let mut k = j + 1;
                while k < tokens.len() && matches!(tokens[k].token, Token::Newline) {
                    k += 1;
                }
                let def_name = match tokens.get(k).map(|sp| &sp.token) {
                    Some(Token::Ident(name)) => name.clone(),
                    _ => {
                        return Err(ScriptPlanError::new(
                            "@@entrypoint must target `def <name>(...)`",
                            tokens[j].span.clone(),
                        ));
                    }
                };
                annotations.push(EntryAnnotation {
                    name: def_name,
                    span: token.span.clone(),
                });
            }
        }
        i += 1;
    }

    Ok((chars.into_iter().collect::<String>(), annotations))
}

fn erase_span(chars: &mut [char], span: &Span) {
    for ch in chars.iter_mut().take(span.end).skip(span.start) {
        if *ch != '\n' {
            *ch = ' ';
        }
    }
}

fn rewrite_script_ast_for_entry(user_ast: Vec<Ast>, entry_name: &str) -> Vec<Ast> {
    let mut out = user_ast
        .into_iter()
        .filter(|stmt| {
            matches!(
                stmt,
                Ast::Def(_, _, _, _, _, _)
                    | Ast::BuiltinDecl(_, _, _, _, _)
                    | Ast::StructDef(_, _, _)
                    | Ast::RecordDef(_, _, _)
                    | Ast::DeferrorDef(_, _, _, _, _)
                    | Ast::ImplDef(_, _, _)
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
    use super::{collect_entrypoint_annotations, prepare_script_compile_plan};

    #[test]
    fn collect_entrypoint_annotations_strips_annotator_and_keeps_def() {
        let source = "@@entrypoint\ndef start() -> Result<()> { Ok(()) }\n";
        let (sanitized, annotations) =
            collect_entrypoint_annotations(source).expect("annotation parsing must succeed");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].name, "start");
        assert!(sanitized.contains("def start() -> Result<()> { Ok(()) }"));
        assert!(!sanitized.contains("@@entrypoint"));
    }

    #[test]
    fn script_compile_plan_uses_cli_entry_over_annotation() {
        let source = "@@entrypoint\ndef auto() -> Result<()> { Ok(()) }\n";
        let plan = prepare_script_compile_plan("sample.srt", source, Some("manual"))
            .expect("compile plan must succeed");
        assert_eq!(plan.selected_entry_name.as_deref(), Some("manual"));
        assert_eq!(
            plan.normalized_entrypoint
                .as_ref()
                .map(|e| e.qualified_symbol.as_str()),
            Some("__Script::sample::manual")
        );
    }
}
