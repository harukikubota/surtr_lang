use std::fs;
use std::path::{Path, PathBuf};

use diagnostics::{SourceId, SourceRegistry};
use forge::bytecode::{
    populate_error_template_lines, stable_hash_hex, synthesize_source_map, SourceFileEntry,
};
use sindr::policy::EntryPoint;
use spire::ast::{Ast, Span};

use crate::error::{ExecutionEnv, RuneError, RuneResult};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScriptCompilePlan {
    pub(crate) source_for_parse: String,
    pub(crate) selected_entry_name: Option<String>,
    pub(crate) normalized_entrypoint: Option<EntryPoint>,
    pub(crate) include_directives: Vec<IncludeDirective>,
}

impl ScriptCompilePlan {
    pub(crate) fn plain(source_for_parse: String) -> Self {
        Self {
            source_for_parse,
            selected_entry_name: None,
            normalized_entrypoint: None,
            include_directives: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IncludeDirective {
    pub(crate) file_path: String,
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

pub(crate) fn collect_default_script_compile_sources(
    env: ExecutionEnv,
    file_path: &str,
    source: &str,
    include_directives: &[IncludeDirective],
) -> RuneResult<xldr::CompileSources> {
    let module_inputs = xldr::collect_additional_default_std_module_inputs().map_err(|e| {
        module_source_collection_error_as_rune_error(
            file_path,
            source,
            format!(
                "{}: failed to collect module sources: {}",
                env.command_name(),
                e
            ),
        )
    })?;

    let mut module_input_stages = vec![module_inputs];
    for directive in include_directives {
        let module_input = resolve_include_module_input(file_path, source, directive)?;
        module_input_stages.push(vec![module_input]);
    }

    let module_sources = xldr::collect_module_sources_with_module_stages(&module_input_stages)
        .map_err(|e| {
            module_source_collection_error_as_rune_error(
                file_path,
                source,
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

fn resolve_include_module_input(
    script_file_path: &str,
    script_source: &str,
    directive: &IncludeDirective,
) -> RuneResult<xldr::ModuleInput> {
    let resolved_path = resolve_include_file_path(script_file_path, &directive.file_path);
    let display_path = normalize_display_path(&resolved_path);
    let module_source = fs::read_to_string(&resolved_path).map_err(|e| {
        include_runtime_error(
            script_file_path,
            script_source,
            directive.span.clone(),
            format!(
                "include failed to read `{}`: {}",
                resolved_path.display(),
                e
            ),
        )
    })?;

    let module_path = xldr::derive_primary_module_path(&module_source)
        .or_else(|| module_path_from_file_name(&display_path))
        .ok_or_else(|| {
            include_runtime_error(
                script_file_path,
                script_source,
                directive.span.clone(),
                format!(
                    "include could not derive module path from `{}`",
                    display_path
                ),
            )
        })?;

    Ok(xldr::ModuleInput {
        file_name: display_path,
        source: module_source,
        module_path,
    })
}

fn resolve_include_file_path(script_file_path: &str, raw_path: &str) -> PathBuf {
    let candidate = Path::new(raw_path);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }

    let base_dir = Path::new(script_file_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    base_dir.join(candidate)
}

fn normalize_display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn module_path_from_file_name(file_name: &str) -> Option<String> {
    let normalized = file_name.replace('\\', "/");
    let mut body = normalized.trim().trim_start_matches("./").to_string();
    if let Some(stripped) = body.strip_suffix(".srt") {
        body = stripped.to_string();
    }

    let segments = body
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }

    Some(segments.join("::"))
}

fn include_runtime_error(file_path: &str, source: &str, span: Span, message: String) -> RuneError {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register(file_path, source.to_string());
    RuneError::diagnostic(
        1,
        &sources,
        source_id,
        "runtime",
        diagnostics::simple_error("RuntimeError", &message, span, None),
    )
}

fn parse_program_with_module_sources(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
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
                    "parse",
                    diagnostics::parse_error_spec(
                        sources.source(e.source_id).unwrap_or(""),
                        e.message(),
                        e.span(),
                    ),
                )
            })?;

    let user_source = sources.source(user_source_id).unwrap_or("");
    let user_ast = match spire::parse_with_context(
        user_source,
        spire::ParserContext::script(user_source_id.0)
            .with_rules(xldr::derive_parse_rules(source_kind)),
    ) {
        Ok(ast) => ast,
        Err(script_err) => {
            if xldr::derive_primary_module_path(user_source).is_none() {
                return Err(RuneError::diagnostic(
                    1,
                    sources,
                    user_source_id,
                    "parse",
                    diagnostics::parse_error_spec(
                        user_source,
                        script_err.message(),
                        script_err.span().clone(),
                    ),
                ));
            }

            spire::parse_with_context(
                user_source,
                spire::ParserContext::module(user_source_id.0, None)
                    .with_rules(xldr::derive_parse_rules(xldr::SourceKind::Module)),
            )
            .map_err(|e| {
                RuneError::diagnostic(
                    1,
                    sources,
                    user_source_id,
                    "parse",
                    diagnostics::parse_error_spec(user_source, e.message(), e.span().clone()),
                )
            })?
        }
    };

    if is_direct_module_source(&user_ast) {
        let lowered = xldr::lower_module_source_ast(
            user_ast,
            Some(compile_sources.user_module_path.as_str()),
        );
        let mut combined_stages = staged_module_asts;
        combined_stages.push(
            lowered
                .into_iter()
                .map(|module| sigil::StagedModuleAst {
                    module_path: module.module_path,
                    ast: module.ast,
                    module_doc: module.module_doc,
                    auto_import: module.auto_import,
                })
                .collect(),
        );
        Ok((combined_stages, Vec::new()))
    } else {
        Ok((staged_module_asts, user_ast))
    }
}

fn is_direct_module_source(ast: &[Ast]) -> bool {
    !ast.is_empty()
        && ast.iter().all(|stmt| {
            matches!(
                stmt,
                Ast::Defmod(_, _, _, _)
                    | Ast::Import(_, _, _)
                    | Ast::Def(..)
                    | Ast::ExtractorDef(..)
                    | Ast::TraitDef(_, _, _, _, _)
                    | Ast::TraitImplDef(_, _, _, _, _)
                    | Ast::StructDef(_, _, _)
                    | Ast::RecordDef(_, _, _)
                    | Ast::DeferrorDef(_, _, _, _, _)
                    | Ast::EnumDef(_, _, _, _, _)
                    | Ast::ImplDef(_, _, _)
                    | Ast::BuiltinDecl(_, _, _, _, _)
                    | Ast::BuiltinExtractorDecl(_, _, _, _, _)
                    | Ast::BuiltinTypeDecl(_, _, _)
                    | Ast::ResultCtorDecl(_, _, _, _, _)
            )
        })
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

    let (module_stages, mut user_ast) = parse_program_with_module_sources(env, compile_sources)?;
    if let Some(entry_name) = compile_plan.selected_entry_name.as_deref() {
        user_ast = rewrite_script_ast_for_entry(user_ast, entry_name);
    }
    let docs = xldr::collect_doc_entries(
        &module_stages,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );

    let declaration_index = sigil::precollect_declaration_index(&module_stages).map_err(|e| {
        let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
        RuneError::diagnostic(
            1,
            sources,
            source_id,
            "resolve",
            diagnostics::resolve_error_spec(
                sources.source(source_id).unwrap_or(""),
                &e.message,
                span,
            ),
        )
    })?;

    let resolved = sigil::resolve_staged_program(
        &module_stages,
        user_ast,
        &declaration_index,
        Some(compile_sources.user_module_path.clone()),
    )
    .map_err(|e| {
        let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
        RuneError::diagnostic(
            1,
            sources,
            source_id,
            "resolve",
            diagnostics::resolve_error_spec(
                sources.source(source_id).unwrap_or(""),
                &e.message,
                span,
            ),
        )
    })?;

    let typed = scar::typecheck_with_context(
        resolved,
        scar::TypecheckContext {
            runtime_policy: xldr::derive_runtime_policy(
                compile_unit_kind,
                source_kind,
                compile_plan.normalized_entrypoint.as_ref(),
            ),
            enforce_builtin_type_contracts: true,
        },
    )
    .map_err(|e| {
        let (source_id, span) = diagnostic_location_for_span(compile_sources, &e.span);
        let mut local_error = e.clone();
        local_error.span = span;
        RuneError::diagnostic(
            1,
            sources,
            source_id,
            "typecheck",
            diagnostics::type_error_spec_by_id(sources, source_id, &local_error),
        )
    })?;

    let mut bytecode = forge::codegen(typed).map_err(|e| {
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
    let source_without_tests = spire::strip_test_annotations(source);
    let (source_for_parse, annotations) = collect_entrypoint_annotations(&source_without_tests)?;
    let (source_for_parse, include_directives) = match collect_include_directives(&source_for_parse)
    {
        Ok(collected) => collected,
        Err(err) => {
            if xldr::derive_primary_module_path(&source_for_parse).is_some() {
                (source_for_parse, Vec::new())
            } else {
                return Err(err);
            }
        }
    };

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
        EntryPoint::script_short_name(name, xldr::script_pseudo_module_path(file_path))
    });

    Ok(ScriptCompilePlan {
        source_for_parse,
        selected_entry_name,
        normalized_entrypoint,
        include_directives,
    })
}

pub(crate) fn collect_entrypoint_annotations(
    source: &str,
) -> Result<(String, Vec<spire::EntryAnnotation>), ScriptPlanError> {
    spire::collect_entrypoint_annotations(source)
        .map_err(|e| ScriptPlanError::new(e.message().to_string(), e.span().clone()))
}

fn collect_include_directives(
    source: &str,
) -> Result<(String, Vec<IncludeDirective>), ScriptPlanError> {
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::script(0).with_rules(spire::ParseRules::script()),
    )
    .map_err(|e| ScriptPlanError::new(e.message().to_string(), e.span().clone()))?;

    let mut chars = source.chars().collect::<Vec<_>>();
    let mut directives = Vec::new();
    for stmt in &ast {
        if let Ast::Include(span, file_path) = stmt {
            directives.push(IncludeDirective {
                file_path: file_path.clone(),
                span: span.clone(),
            });
            for ch in chars.iter_mut().take(span.end).skip(span.start) {
                if *ch != '\n' {
                    *ch = ' ';
                }
            }
        }
    }

    Ok((chars.into_iter().collect::<String>(), directives))
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
                    | Ast::TraitImplDef(_, _, _, _, _)
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
    use super::{
        collect_entrypoint_annotations, diagnostic_location_for_span, is_direct_module_source,
        load_error_span, module_source_collection_error_as_rune_error, prepare_script_compile_plan,
        source_id_for_span,
    };
    use crate::error::RuneError;
    use spire::ast::{Ast, Span};
    use xldr::{SourceKind, StagedModule};

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

    #[test]
    fn script_compile_plan_extracts_include_directives() {
        let source = r#"include 'fixtures/Helper.srt'
import Helper::add
print(to_string(add(1, 2)))
"#;

        let plan = prepare_script_compile_plan("sample.srt", source, None)
            .expect("compile plan must succeed");

        assert_eq!(plan.include_directives.len(), 1);
        assert_eq!(plan.include_directives[0].file_path, "fixtures/Helper.srt");
        assert_eq!(plan.source_for_parse.len(), source.len());
        assert!(!plan
            .source_for_parse
            .contains("include 'fixtures/Helper.srt'"));
        assert!(plan.source_for_parse.contains("import Helper::add"));
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
    fn direct_module_source_is_detected() {
        let ast = spire::parse_with_context(
            r#"defmod Helper {
  def add(x: Int, y: Int) -> Int {
    x + y
  }
}"#,
            spire::ParserContext::module(0, None).with_rules(spire::ParseRules::module()),
        )
        .expect("module source should parse");

        assert!(is_direct_module_source(&ast));
    }

    #[test]
    fn plain_script_is_not_treated_as_module_source() {
        let ast = spire::parse(r#"print(to_string(1))"#).expect("script source should parse");

        assert!(matches!(ast.last(), Some(Ast::App(_, _, _))));
        assert!(!is_direct_module_source(&ast));
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
            "run: failed to collect module sources: broken",
        );
        match err {
            RuneError::Diagnostic { diagnostic, .. } => {
                assert_eq!(diagnostic.phase, "resolve");
                assert_eq!(diagnostic.spec.kind, "LoadError");
                assert!(diagnostic
                    .spec
                    .message
                    .contains("failed to collect module sources"));
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
                source_kind: SourceKind::Module,
            }]],
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
                source_kind: SourceKind::Module,
            }]],
        };
        let span = Span { start: 50, end: 51 };
        let source_id = source_id_for_span(&compile_sources, &span);
        assert_eq!(
            compile_sources.sources.file_name(source_id),
            Some("examples/mahjong/run.srt")
        );
    }
}
