use std::env;
use std::fs;
use std::path::Path;
use std::process;

use eldr::value::Value;
use forge::bytecode::populate_error_template_lines;
use spire::ast::{Ast, Span};
use spire::token::Token;
mod dump;

const RUNE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().collect();

    let result = match args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("surtr {}", RUNE_VERSION);
            Ok(())
        }
        Some("run") => parse_run_options(&args[2..]).and_then(run_command),
        Some("repl") => parse_repl_options(&args[2..]).and_then(xldr::repl_command),
        Some("build") => {
            if !(3..=4).contains(&args.len()) {
                print_usage();
                Err(1)
            } else {
                build_command(&args[2], args.get(3).map(String::as_str))
            }
        }
        Some("dump") => {
            if args.len() < 3 {
                print_usage();
                Err(1)
            } else {
                dump::dump_command(&args[2], &args[3..])
            }
        }
        _ => {
            print_usage();
            Err(1)
        }
    };

    if let Err(code) = result {
        process::exit(code);
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  surtr --version");
    eprintln!("  surtr run <file.srt|file.eldr> [--entry <name>]");
    eprintln!("  surtr repl [--quiet] [--banner] [--version]");
    eprintln!("  surtr build <file.srt> [output.eldr]");
    eprintln!("  surtr dump <file.eldr> [--format json]");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunOptions {
    file_path: String,
    entry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScriptCompilePlan {
    source_for_parse: String,
    selected_entry_name: Option<String>,
    normalized_entrypoint: Option<spire::EntryPoint>,
}

#[derive(Debug, Clone, PartialEq)]
struct EntryAnnotation {
    name: String,
    span: Span,
}

#[derive(Debug, Clone, PartialEq)]
struct ScriptPlanError {
    message: String,
    span: Span,
}

impl ScriptPlanError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

fn parse_repl_options(args: &[String]) -> Result<xldr::ReplOptions, i32> {
    let mut options = xldr::ReplOptions::default();

    for arg in args {
        match arg.as_str() {
            "--quiet" => options.quiet = true,
            "--banner" => options.banner = xldr::BannerMode::Detailed,
            "--version" => options.version = true,
            other => {
                eprintln!("repl: unknown option '{}'", other);
                print_usage();
                return Err(1);
            }
        }
    }

    Ok(options)
}

fn parse_run_options(args: &[String]) -> Result<RunOptions, i32> {
    if args.is_empty() {
        print_usage();
        return Err(1);
    }

    let file_path = args[0].clone();
    let mut entry = None;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--entry" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("run: missing value for --entry");
                    print_usage();
                    return Err(1);
                }
                if entry.is_some() {
                    eprintln!("run: --entry may only be specified once");
                    return Err(1);
                }
                entry = Some(args[i].clone());
            }
            other => {
                eprintln!("run: unknown option '{}'", other);
                print_usage();
                return Err(1);
            }
        }
        i += 1;
    }

    Ok(RunOptions { file_path, entry })
}

fn run_command(options: RunOptions) -> Result<(), i32> {
    if options.file_path.ends_with(".eldr") {
        if options.entry.is_some() {
            eprintln!("run: --entry is only supported for .srt input");
            return Err(1);
        }
        run_eldr_file(&options.file_path)
    } else {
        run_source_file(&options.file_path, options.entry.as_deref())
    }
}

fn run_source_file(file_path: &str, cli_entry: Option<&str>) -> Result<(), i32> {
    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", file_path, e);
            return Err(1);
        }
    };

    let compile_plan = match prepare_script_compile_plan(file_path, &source, cli_entry) {
        Ok(plan) => plan,
        Err(e) => {
            let mut sources = diagnostics::SourceRegistry::new();
            let source_id = sources.register(file_path, source.clone());
            diagnostics::report_error_by_id(
                &sources,
                source_id,
                diagnostics::simple_error("ParseError", &e.message, e.span, None),
            );
            return Err(1);
        }
    };

    let module_sources = xldr::collect_module_sources_with_module_stages(&[]).map_err(|e| {
        eprintln!("Error collecting module sources: {}", e);
        1
    })?;
    let compile_sources = xldr::compose_script_compile_sources(
        file_path,
        &compile_plan.source_for_parse,
        module_sources,
    );
    let bytecode = compile_source(&compile_sources, &compile_plan)?;
    execute_bytecode(
        bytecode,
        compile_sources
            .sources
            .owned_context(compile_sources.user_source_id),
    )
}

fn run_eldr_file(file_path: &str) -> Result<(), i32> {
    let bytes = match fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error reading {}: {}", file_path, e);
            return Err(1);
        }
    };

    let bytecode = match forge::bytecode::Bytecode::decode(&bytes) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error decoding {}: {}", file_path, e);
            return Err(1);
        }
    };

    execute_bytecode(bytecode, None)
}

fn build_command(input_srt: &str, output_eldr: Option<&str>) -> Result<(), i32> {
    let source = match fs::read_to_string(input_srt) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", input_srt, e);
            return Err(1);
        }
    };

    let module_sources = xldr::collect_module_sources_with_module_stages(&[]).map_err(|e| {
        eprintln!("Error collecting module sources: {}", e);
        1
    })?;
    let compile_plan = ScriptCompilePlan {
        source_for_parse: source,
        selected_entry_name: None,
        normalized_entrypoint: None,
    };
    let compile_sources = xldr::compose_script_compile_sources(
        input_srt,
        &compile_plan.source_for_parse,
        module_sources,
    );
    let bytecode = compile_source(&compile_sources, &compile_plan)?;
    let bytes = match bytecode.encode() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error encoding bytecode: {}", e);
            return Err(1);
        }
    };

    let output_path = output_eldr
        .map(ToString::to_string)
        .unwrap_or_else(|| default_output_path(input_srt));
    if let Err(e) = fs::write(&output_path, bytes) {
        eprintln!("Error writing {}: {}", output_path, e);
        return Err(1);
    }
    Ok(())
}

fn default_output_path(input_srt: &str) -> String {
    let path = Path::new(input_srt);
    path.with_extension("eldr").to_string_lossy().into_owned()
}

fn parse_program_with_builtin_prelude(
    compile_sources: &xldr::CompileSources,
    compile_unit_kind: spire::CompileUnitKind,
    entrypoint: Option<&spire::EntryPoint>,
) -> Result<(Vec<Vec<sigil::StagedModuleAst>>, Vec<spire::ast::Ast>), i32> {
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let staged_module_asts =
        match xldr::parse_module_stages_from_compile_sources(compile_sources, compile_unit_kind) {
            Ok(stages) => stages,
            Err(e) => {
                diagnostics::report_error_by_id(
                    sources,
                    e.source_id,
                    diagnostics::simple_error("ParseError", e.message(), e.span(), None),
                );
                return Err(1);
            }
        };

    let user_source = sources.source(user_source_id).unwrap_or("");
    let user_ast = match spire::parse_with_context(
        user_source,
        spire::ParserContext::script(user_source_id.0).with_rules(xldr::derive_source_rules(
            compile_unit_kind,
            xldr::SourceKind::Script,
            entrypoint,
        )),
    ) {
        Ok(a) => a,
        Err(e) => {
            let message = e.message();
            diagnostics::report_error_by_id(
                sources,
                user_source_id,
                diagnostics::simple_error("ParseError", message, e.span().clone(), None),
            );
            return Err(1);
        }
    };

    Ok((staged_module_asts, user_ast))
}

fn compile_source(
    compile_sources: &xldr::CompileSources,
    compile_plan: &ScriptCompilePlan,
) -> Result<forge::bytecode::Bytecode, i32> {
    let compile_unit_kind = spire::CompileUnitKind::Script;
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let user_source = sources.source(user_source_id).unwrap_or("");

    // Phase 1: Spire — parse
    let (module_stages, mut user_ast) = parse_program_with_builtin_prelude(
        compile_sources,
        compile_unit_kind,
        compile_plan.normalized_entrypoint.as_ref(),
    )?;
    if let Some(entry_name) = compile_plan.selected_entry_name.as_deref() {
        user_ast = rewrite_script_ast_for_entry(user_ast, entry_name);
    }

    // Issue 6: precollect declaration index from staged modules before body resolution.
    let declaration_index = match sigil::precollect_declaration_index(&module_stages) {
        Ok(index) => index,
        Err(e) => {
            diagnostics::report_error_by_id(
                sources,
                user_source_id,
                diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
            );
            return Err(1);
        }
    };

    // Phase 2: Sigil — resolve names
    let resolved = match sigil::resolve_staged_program(
        &module_stages,
        user_ast,
        &declaration_index,
        Some(compile_sources.user_module_path.clone()),
    ) {
        Ok(r) => r,
        Err(e) => {
            diagnostics::report_error_by_id(
                sources,
                user_source_id,
                diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
            );
            return Err(1);
        }
    };

    // Phase 3: Scar — type check
    let typed = match scar::typecheck_with_context(
        resolved,
        scar::TypecheckContext {
            source_rules: xldr::derive_source_rules(
                compile_unit_kind,
                xldr::SourceKind::Script,
                compile_plan.normalized_entrypoint.as_ref(),
            ),
        },
    ) {
        Ok(t) => t,
        Err(e) => {
            diagnostics::report_error_by_id(
                sources,
                user_source_id,
                diagnostics::type_error_spec_by_id(sources, user_source_id, &e),
            );
            return Err(1);
        }
    };

    // Phase 4: Forge — generate bytecode
    let mut bytecode = match forge::codegen(typed) {
        Ok(b) => b,
        Err(e) => {
            diagnostics::report_error_by_id(
                sources,
                user_source_id,
                diagnostics::simple_error("CodegenError", &e.message, e.span.clone(), None),
            );
            return Err(1);
        }
    };

    populate_error_template_lines(&mut bytecode.error_templates, user_source);

    Ok(bytecode)
}

fn prepare_script_compile_plan(
    file_path: &str,
    source: &str,
    cli_entry: Option<&str>,
) -> Result<ScriptCompilePlan, ScriptPlanError> {
    let (source_for_parse, annotations) = collect_entrypoint_annotations(source)?;

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

fn collect_entrypoint_annotations(
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
                Ast::Def(_, _, _, _, _)
                    | Ast::BuiltinDecl(_, _, _, _)
                    | Ast::StructDef(_, _, _)
                    | Ast::RecordDef(_, _, _)
                    | Ast::DeferrorDef(_, _, _, _)
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

fn execute_bytecode(
    bytecode: forge::bytecode::Bytecode,
    source_context: Option<(String, String)>,
) -> Result<(), i32> {
    // Phase 5: Eldr — execute
    let mut vm = match source_context {
        Some((source, file_path)) => eldr::VM::new(bytecode).with_source(source, file_path),
        None => eldr::VM::new(bytecode),
    };
    if let Err(e) = vm.run() {
        eldr::report_runtime_error(
            &e,
            vm.source(),
            vm.source_file(),
            vm.runtime_error_location(),
        );
        return Err(1);
    }

    if report_final_result_error_if_any(&vm) {
        return Err(1);
    }

    match vm.exit_code() {
        0 => Ok(()),
        code => Err(code),
    }
}

fn report_final_result_error_if_any(vm: &eldr::VM) -> bool {
    match vm.last_value() {
        Some(Value::Tagged { tag: 1, fields }) => {
            if let Some(err_value) = fields.first() {
                report_error_value(vm, err_value);
            } else {
                eprintln!("Error: InvalidResult: missing Err payload");
            }
            true
        }
        _ => false,
    }
}

fn report_error_value(vm: &eldr::VM, value: &Value) {
    match value {
        Value::Error(rich) => {
            let start = rich.location.span_start as usize;
            let mut end = rich.location.span_end as usize;
            if end <= start {
                end = start.saturating_add(1);
            }
            match (vm.source(), vm.source_file()) {
                (Some(source), Some(file_name)) => diagnostics::report_error(
                    file_name,
                    source,
                    diagnostics::simple_error(
                        rich.kind.clone(),
                        rich.message.clone(),
                        spire::ast::Span { start, end },
                        None,
                    ),
                ),
                _ => eprintln!("Error: {}: {}", rich.kind, rich.message),
            }
        }
        other => {
            eprintln!("Error: {}", eldr::builtin::inspect_value(vm, other));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_entrypoint_annotations, parse_run_options, populate_error_template_lines,
        prepare_script_compile_plan,
    };
    use forge::bytecode::{line_column_for_offset, ErrTemplate};

    #[test]
    fn line_column_for_offset_tracks_multiline_source() {
        let source = "deferror Boom {\n  \"boom\"\n}\n";
        assert_eq!(line_column_for_offset(source, 0), (1, 1));
        assert_eq!(line_column_for_offset(source, 16), (2, 1));
    }

    #[test]
    fn populate_error_template_lines_uses_span_start() {
        let source = "deferror Boom {\n  \"boom\"\n}\n";
        let mut templates = vec![ErrTemplate {
            id: 0,
            kind: "Boom".into(),
            span_start: 16,
            span_end: 24,
            line: 0,
            column: 0,
            format: "{}".into(),
            num_params: 1,
        }];

        populate_error_template_lines(&mut templates, source);

        assert_eq!(templates[0].line, 2);
        assert_eq!(templates[0].column, 1);
    }

    #[test]
    fn run_options_parses_entry() {
        let opts = parse_run_options(&[
            "main.srt".to_string(),
            "--entry".to_string(),
            "start".to_string(),
        ])
        .expect("run options must parse");
        assert_eq!(opts.file_path, "main.srt");
        assert_eq!(opts.entry.as_deref(), Some("start"));
    }

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
