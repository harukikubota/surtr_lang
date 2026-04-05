use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use eldr::value::Value;
use forge::bytecode::populate_error_template_lines;
use spire::ast::{Ast, Span};
use spire::token::Token;
mod dump;

const RUNE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionEnv {
    Dev,
    Test,
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let result = match args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("surtr {}", RUNE_VERSION);
            Ok(())
        }
        Some("run") => parse_run_options(&args[2..])
            .and_then(|options| run_command(options, ExecutionEnv::Dev)),
        Some("repl") => parse_repl_options(&args[2..]).and_then(xldr::repl_command),
        Some("build") => {
            if !(3..=4).contains(&args.len()) {
                print_usage();
                Err(1)
            } else {
                build_command(&args[2], args.get(3).map(String::as_str), ExecutionEnv::Dev)
            }
        }
        Some("test") => parse_test_options(&args[2..]).and_then(test_command),
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
    eprintln!("  surtr test [selector]");
    eprintln!("  surtr repl [--quiet] [--banner] [--version]");
    eprintln!("  surtr build <file.srt> [output.eldr]");
    eprintln!("  surtr dump <file.eldr|entry.srt> [--format json] [--entry <name>]");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunOptions {
    file_path: String,
    entry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestOptions {
    selector: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestLocation {
    file_path: String,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestOperator {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
}

impl TestOperator {
    fn normalized_label(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Neq => "neq",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestCase {
    module_path: String,
    target_def: String,
    expr: String,
    lhs_expr: String,
    rhs_expr: String,
    op: TestOperator,
    location: TestLocation,
}

impl TestCase {
    fn display_name(&self) -> String {
        if self.module_path.is_empty() {
            self.target_def.clone()
        } else {
            format!("{}::{}", self.module_path, self.target_def)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestSelector {
    All,
    Module(String),
    Function {
        module_path: String,
        function_name: String,
    },
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

fn parse_test_options(args: &[String]) -> Result<TestOptions, i32> {
    if args.len() > 1 {
        eprintln!("test: too many arguments");
        print_usage();
        return Err(1);
    }

    let selector = args.first().cloned();
    Ok(TestOptions { selector })
}

fn run_command(options: RunOptions, _env: ExecutionEnv) -> Result<(), i32> {
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

fn parse_test_selector(raw: Option<&str>) -> TestSelector {
    let Some(selector) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return TestSelector::All;
    };

    if let Some((module_path, function_name)) = selector.rsplit_once("::") {
        if !module_path.is_empty() && !function_name.is_empty() {
            return TestSelector::Function {
                module_path: module_path.to_string(),
                function_name: function_name.to_string(),
            };
        }
    }

    TestSelector::Module(selector.to_string())
}

fn test_case_matches_selector(test: &TestCase, selector: &TestSelector) -> bool {
    match selector {
        TestSelector::All => true,
        TestSelector::Module(module) => &test.module_path == module,
        TestSelector::Function {
            module_path,
            function_name,
        } => &test.module_path == module_path && &test.target_def == function_name,
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn collect_lib_root_sources() -> Result<Vec<(PathBuf, String)>, String> {
    let lib_dir = Path::new("lib");
    let entries = fs::read_dir(lib_dir)
        .map_err(|e| format!("test: failed to read `{}`: {}", lib_dir.display(), e))?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "srt")
        {
            files.push(path);
        }
    }
    files.sort();

    let mut sources = Vec::with_capacity(files.len());
    for path in files {
        let source = fs::read_to_string(&path)
            .map_err(|e| format!("test: failed to read `{}`: {}", display_path(&path), e))?;
        sources.push((path, source));
    }
    Ok(sources)
}

fn collect_additional_std_module_inputs() -> Result<Vec<xldr::ModuleInput>, String> {
    if !Path::new("lib").exists() {
        return Ok(Vec::new());
    }
    let lib_sources = collect_lib_root_sources()?;
    let mut module_inputs = Vec::new();
    for (path, source) in lib_sources {
        let file_path = display_path(&path);
        let module_path = derive_primary_module_path(&source)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| module_path_from_file_name(&path));

        if module_path != "Bootstrap" && module_path != "Kernel" {
            module_inputs.push(xldr::ModuleInput {
                file_name: file_path,
                source,
                module_path,
            });
        }
    }

    Ok(module_inputs)
}

fn module_path_from_file_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn derive_primary_module_path(source: &str) -> Option<String> {
    let tokens = spire::lexer::tokenize(source).ok()?;
    for (idx, spanned) in tokens.iter().enumerate() {
        if !matches!(spanned.token, Token::Defmod) {
            continue;
        }

        let mut j = idx + 1;
        while matches!(tokens.get(j).map(|t| &t.token), Some(Token::Newline)) {
            j += 1;
        }

        let mut segments = Vec::new();
        match tokens.get(j).map(|sp| &sp.token) {
            Some(Token::Ident(name)) => {
                segments.push(name.clone());
                j += 1;
            }
            _ => return None,
        }

        while matches!(tokens.get(j).map(|t| &t.token), Some(Token::Colon))
            && matches!(tokens.get(j + 1).map(|t| &t.token), Some(Token::Colon))
        {
            j += 2;
            match tokens.get(j).map(|sp| &sp.token) {
                Some(Token::Ident(name)) => {
                    segments.push(name.clone());
                    j += 1;
                }
                _ => return None,
            }
        }

        if !segments.is_empty() {
            return Some(segments.join("::"));
        }
    }

    None
}

fn char_to_byte_index(source: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    source
        .char_indices()
        .nth(char_index)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(source.len())
}

fn slice_by_char_range(source: &str, start: usize, end: usize) -> &str {
    let byte_start = char_to_byte_index(source, start);
    let byte_end = char_to_byte_index(source, end);
    &source[byte_start..byte_end]
}

fn line_column_for_char_offset(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for (idx, ch) in source.chars().enumerate() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn split_test_expression(expr: &str) -> Result<(String, String, TestOperator), String> {
    let tokens = spire::lexer::tokenize(expr).map_err(|e| e.message().to_string())?;
    let mut depth_paren = 0i32;
    let mut depth_brack = 0i32;
    let mut depth_brace = 0i32;
    let mut found: Option<(TestOperator, Span)> = None;

    for token in tokens {
        match token.token {
            Token::LParen => depth_paren += 1,
            Token::RParen => depth_paren -= 1,
            Token::LBrack => depth_brack += 1,
            Token::RBrack => depth_brack -= 1,
            Token::LBrace => depth_brace += 1,
            Token::RBrace => depth_brace -= 1,
            _ => {}
        }

        if depth_paren != 0 || depth_brack != 0 || depth_brace != 0 {
            continue;
        }

        let op = match token.token {
            Token::EqEq => Some(TestOperator::Eq),
            Token::BangEq => Some(TestOperator::Neq),
            Token::Lt => Some(TestOperator::Lt),
            Token::LtEq => Some(TestOperator::Lte),
            Token::Gt => Some(TestOperator::Gt),
            Token::GtEq => Some(TestOperator::Gte),
            _ => None,
        };

        if let Some(op) = op {
            if found.is_some() {
                return Err("multiple top-level comparison operators in @@test expression".into());
            }
            found = Some((op, token.span));
        }
    }

    let (op, op_span) = found.ok_or_else(|| {
        "test expression must contain one top-level comparison operator".to_string()
    })?;

    let expr_char_len = expr.chars().count();
    let lhs = slice_by_char_range(expr, 0, op_span.start)
        .trim()
        .to_string();
    let rhs = slice_by_char_range(expr, op_span.end, expr_char_len)
        .trim()
        .to_string();
    if lhs.is_empty() || rhs.is_empty() {
        return Err("test expression requires both lhs and rhs".into());
    }
    Ok((lhs, rhs, op))
}

fn find_def_name_for_test_chain(
    tokens: &[spire::token::Spanned<Token>],
    mut index: usize,
) -> Result<String, String> {
    loop {
        while matches!(tokens.get(index).map(|t| &t.token), Some(Token::Newline)) {
            index += 1;
        }

        match tokens.get(index).map(|t| &t.token) {
            Some(Token::Annotator(_)) => {
                index += 1;
                while !matches!(
                    tokens.get(index).map(|t| &t.token),
                    Some(Token::Newline) | Some(Token::Eof) | None
                ) {
                    index += 1;
                }
            }
            Some(Token::Def) => {
                index += 1;
                while matches!(tokens.get(index).map(|t| &t.token), Some(Token::Newline)) {
                    index += 1;
                }
                if let Some(Token::Ident(name)) = tokens.get(index).map(|t| &t.token) {
                    return Ok(name.clone());
                }
                return Err("@@test must target `def <name>(...)`".into());
            }
            _ => {
                return Err("@@test must target a following function definition (`def`)".into());
            }
        }
    }
}

fn collect_test_cases_from_source(
    file_path: &str,
    source: &str,
    module_path: &str,
) -> Result<Vec<TestCase>, String> {
    let tokens = spire::lexer::tokenize(source).map_err(|e| {
        let (line, column) = line_column_for_char_offset(source, e.span().start);
        format!(
            "test: parse error in {}:{}:{}: {}",
            file_path,
            line,
            column,
            e.message()
        )
    })?;

    let mut cases = Vec::new();
    for (idx, token) in tokens.iter().enumerate() {
        let Token::Annotator(name) = &token.token else {
            continue;
        };
        if name != "test" {
            continue;
        }

        let mut expr_token_end = idx + 1;
        while !matches!(
            tokens.get(expr_token_end).map(|t| &t.token),
            Some(Token::Newline) | Some(Token::Eof) | None
        ) {
            expr_token_end += 1;
        }
        if expr_token_end == idx + 1 {
            let (line, column) = line_column_for_char_offset(source, token.span.start);
            return Err(format!(
                "test: missing expression for @@test in {}:{}:{}",
                file_path, line, column
            ));
        }

        let expr_start = token.span.end;
        let expr_end = tokens[expr_token_end - 1].span.end;
        let expr = slice_by_char_range(source, expr_start, expr_end)
            .trim()
            .to_string();
        let (lhs_expr, rhs_expr, op) = split_test_expression(&expr).map_err(|message| {
            let (line, column) = line_column_for_char_offset(source, token.span.start);
            format!(
                "test: invalid @@test in {}:{}:{}: {}",
                file_path, line, column, message
            )
        })?;
        let target_def =
            find_def_name_for_test_chain(&tokens, expr_token_end).map_err(|message| {
                let (line, column) = line_column_for_char_offset(source, token.span.start);
                format!(
                    "test: invalid @@test in {}:{}:{}: {}",
                    file_path, line, column, message
                )
            })?;
        let (line, column) = line_column_for_char_offset(source, token.span.start);

        cases.push(TestCase {
            module_path: module_path.to_string(),
            target_def,
            expr,
            lhs_expr,
            rhs_expr,
            op,
            location: TestLocation {
                file_path: file_path.to_string(),
                line,
                column,
            },
        });
    }

    Ok(cases)
}

fn build_expression_script_source(module_path: &str, expr: &str) -> String {
    let mut source = String::new();
    if !module_path.is_empty() && module_path != "Bootstrap" && module_path != "Kernel" {
        source.push_str(&format!("import {};\n", module_path));
    }
    source.push_str(expr);
    source.push('\n');
    source
}

fn evaluate_expression(
    module_sources: &xldr::ModuleSources,
    module_path: &str,
    expr: &str,
) -> Result<(Value, String), String> {
    let raw_script_source = build_expression_script_source(module_path, expr);
    let source_for_parse = xldr::strip_test_annotations(&raw_script_source);
    let compile_plan = ScriptCompilePlan {
        source_for_parse: source_for_parse.clone(),
        selected_entry_name: None,
        normalized_entrypoint: None,
    };
    let compile_sources = xldr::compose_script_compile_sources(
        "__surtr_test__.srt",
        &source_for_parse,
        module_sources.clone(),
    );
    let bytecode = compile_source(&compile_sources, &compile_plan)
        .map_err(|_| "compile error while evaluating test expression".to_string())?;

    let mut vm = eldr::VM::new(bytecode)
        .with_output_capture()
        .with_error_capture();
    vm.run()
        .map_err(|e| format!("runtime error while evaluating test expression: {}", e))?;
    let value = vm.last_value().cloned().unwrap_or(Value::Unit);
    let display = value.to_display_string(&vm.type_registry());
    Ok((value, display))
}

fn report_test_failure(module_sources: &xldr::ModuleSources, test: &TestCase, detail: &str) {
    println!(
        "[FAIL] {} ({}:{}:{})",
        test.display_name(),
        test.location.file_path,
        test.location.line,
        test.location.column
    );
    println!("  expr: {}", test.expr);

    let lhs_display = evaluate_expression(module_sources, &test.module_path, &test.lhs_expr)
        .map(|(_, display)| display)
        .unwrap_or_else(|e| format!("<error: {}>", e));
    let rhs_display = evaluate_expression(module_sources, &test.module_path, &test.rhs_expr)
        .map(|(_, display)| display)
        .unwrap_or_else(|e| format!("<error: {}>", e));
    println!("  lhs : {} => {}", test.lhs_expr, lhs_display);
    println!("  rhs : {} => {}", test.rhs_expr, rhs_display);
    println!("  op  : {}", test.op.normalized_label());
    if !detail.is_empty() {
        println!("  note: {}", detail);
    }
}

fn test_command(options: TestOptions) -> Result<(), i32> {
    let _env = ExecutionEnv::Test;
    let lib_sources = collect_lib_root_sources().map_err(|message| {
        eprintln!("{}", message);
        1
    })?;
    if lib_sources.is_empty() {
        eprintln!("test: no `.srt` files found under `./lib`");
        return Err(1);
    }

    let mut all_tests = Vec::new();
    for (path, source) in lib_sources {
        let file_path = display_path(&path);
        let module_path = derive_primary_module_path(&source)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| module_path_from_file_name(&path));
        let tests = collect_test_cases_from_source(&file_path, &source, &module_path).map_err(
            |message| {
                eprintln!("{}", message);
                1
            },
        )?;
        all_tests.extend(tests);
    }

    if all_tests.is_empty() {
        println!("No tests found.");
        return Ok(());
    }

    let selector = parse_test_selector(options.selector.as_deref());
    let selected_tests = all_tests
        .into_iter()
        .filter(|test| test_case_matches_selector(test, &selector))
        .collect::<Vec<_>>();
    if selected_tests.is_empty() {
        println!("No tests matched selector.");
        return Ok(());
    }

    let module_inputs = collect_additional_std_module_inputs().map_err(|message| {
        eprintln!("{}", message);
        1
    })?;
    let module_sources = if module_inputs.is_empty() {
        xldr::collect_module_sources_with_module_stages(&[])
    } else {
        xldr::collect_module_sources_with_std_module_stages(&[module_inputs])
    }
    .map_err(|e| {
        eprintln!("test: failed to collect module sources: {}", e);
        1
    })?;

    let mut passed = 0usize;
    let mut failed = 0usize;
    for test in selected_tests {
        match evaluate_expression(&module_sources, &test.module_path, &test.expr) {
            Ok((Value::Bool(true), _)) => {
                println!("[PASS] {}", test.display_name());
                passed += 1;
            }
            Ok((Value::Bool(false), _)) => {
                report_test_failure(&module_sources, &test, "");
                failed += 1;
            }
            Ok((_other, display)) => {
                report_test_failure(
                    &module_sources,
                    &test,
                    &format!(
                        "test expression must evaluate to Boolean (got `{}`)",
                        display
                    ),
                );
                failed += 1;
            }
            Err(message) => {
                report_test_failure(&module_sources, &test, &message);
                failed += 1;
            }
        }
    }

    let total = passed + failed;
    println!(
        "test result: passed={}, failed={}, total={}",
        passed, failed, total
    );

    if failed == 0 {
        Ok(())
    } else {
        Err(1)
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

    // E-1 contract (CLI run):
    // 1) compile-time failures (parse/resolve/typecheck/codegen) terminate immediately with exit=1.
    // 2) runtime traps terminate with exit=1.
    // 3) final `Result::Err` is reported as a language-level error and also exits with exit=1.
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

    let compile_sources =
        collect_default_script_compile_sources(file_path, &compile_plan.source_for_parse)?;
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

fn build_command(
    input_srt: &str,
    output_eldr: Option<&str>,
    _env: ExecutionEnv,
) -> Result<(), i32> {
    let source = match fs::read_to_string(input_srt) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", input_srt, e);
            return Err(1);
        }
    };

    let compile_plan = ScriptCompilePlan {
        source_for_parse: source,
        selected_entry_name: None,
        normalized_entrypoint: None,
    };
    let compile_sources =
        collect_default_script_compile_sources(input_srt, &compile_plan.source_for_parse)?;
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

fn collect_default_script_compile_sources(
    file_path: &str,
    source: &str,
) -> Result<xldr::CompileSources, i32> {
    let module_inputs = collect_additional_std_module_inputs().map_err(|message| {
        eprintln!("{}", message);
        1
    })?;
    let module_sources = if module_inputs.is_empty() {
        xldr::collect_module_sources_with_module_stages(&[])
    } else {
        xldr::collect_module_sources_with_std_module_stages(&[module_inputs])
    }
    .map_err(|e| {
        eprintln!("Error collecting module sources: {}", e);
        1
    })?;
    Ok(xldr::compose_script_compile_sources(
        file_path,
        source,
        module_sources,
    ))
}

fn parse_program_with_module_sources(
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
    let (module_stages, mut user_ast) = parse_program_with_module_sources(
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
    // E-3 note:
    // We intentionally keep this check at the CLI boundary instead of abstracting it into
    // the lower pipeline; `run` and `repl` have different UX semantics for `Result::Err`.
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
        collect_entrypoint_annotations, parse_run_options, parse_test_options,
        populate_error_template_lines, prepare_script_compile_plan,
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

    #[test]
    fn test_options_accept_selector_or_empty() {
        let with_selector =
            parse_test_options(&["Kernel::add".to_string()]).expect("selector should parse");
        assert_eq!(with_selector.selector.as_deref(), Some("Kernel::add"));

        let without_selector = parse_test_options(&[]).expect("empty selector should parse");
        assert_eq!(without_selector.selector, None);
    }
}
