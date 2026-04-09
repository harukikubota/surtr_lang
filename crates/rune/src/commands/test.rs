use eldr::value::Value;
use spire::ast::Span;
use spire::token::Token;

use crate::compile::{compile_source, ScriptCompilePlan};
use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::loader::{
    collect_additional_std_module_inputs, collect_lib_root_sources, derive_primary_module_path,
    module_path_from_file_name,
};
use crate::util::{display_path, line_column_for_char_offset, slice_by_char_range};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestOptions {
    pub(crate) selector: Option<String>,
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

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    let options = parse_test_options(args)?;
    test_command(options, ExecutionEnv::Test)
}

pub(crate) fn parse_test_options(args: &[String]) -> RuneResult<TestOptions> {
    if args.len() > 1 {
        return Err(RuneError::usage("test: too many arguments"));
    }

    Ok(TestOptions {
        selector: args.first().cloned(),
    })
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

fn build_expression_script_source(module_path: &str, target_def: &str, expr: &str) -> String {
    let mut source = String::new();
    if !module_path.is_empty() && module_path != "Bootstrap" && module_path != "Kernel" {
        source.push_str(&format!("import {}::{};\n", module_path, target_def));
    }
    source.push_str(expr);
    source.push('\n');
    source
}

fn evaluate_expression(
    module_sources: &xldr::ModuleSources,
    module_path: &str,
    target_def: &str,
    expr: &str,
    env: ExecutionEnv,
) -> RuneResult<(Value, String)> {
    let raw_script_source = build_expression_script_source(module_path, target_def, expr);
    let source_for_parse = xldr::strip_test_annotations(&raw_script_source);
    let compile_plan = ScriptCompilePlan::plain(source_for_parse.clone());
    let compile_sources = xldr::compose_script_compile_sources(
        "__surtr_test__.srt",
        &source_for_parse,
        module_sources.clone(),
    );
    let bytecode = compile_source(env, &compile_sources, &compile_plan)?;

    let mut vm = eldr::VM::new(bytecode)
        .with_output_capture()
        .with_error_capture();
    vm.run().map_err(|e| {
        RuneError::message(
            1,
            format!("runtime error while evaluating test expression: {}", e),
        )
    })?;
    let value = vm.last_value().cloned().unwrap_or(Value::Unit);
    let display = value.to_display_string(vm.type_registry());
    Ok((value, display))
}

fn report_test_failure(
    module_sources: &xldr::ModuleSources,
    test: &TestCase,
    detail: &str,
    env: ExecutionEnv,
) {
    println!(
        "[FAIL] {} ({}:{}:{})",
        test.display_name(),
        test.location.file_path,
        test.location.line,
        test.location.column
    );
    println!("  expr: {}", test.expr);

    let lhs_display = evaluate_expression(
        module_sources,
        &test.module_path,
        &test.target_def,
        &test.lhs_expr,
        env,
    )
    .map(|(_, display)| display)
    .unwrap_or_else(|e| format!("<error: {}>", e.summary()));
    let rhs_display = evaluate_expression(
        module_sources,
        &test.module_path,
        &test.target_def,
        &test.rhs_expr,
        env,
    )
    .map(|(_, display)| display)
    .unwrap_or_else(|e| format!("<error: {}>", e.summary()));
    println!("  lhs : {} => {}", test.lhs_expr, lhs_display);
    println!("  rhs : {} => {}", test.rhs_expr, rhs_display);
    println!("  op  : {}", test.op.normalized_label());
    if !detail.is_empty() {
        println!("  note: {}", detail);
    }
}

fn test_command(options: TestOptions, env: ExecutionEnv) -> RuneResult<()> {
    let lib_sources = collect_lib_root_sources(env)?;
    if lib_sources.is_empty() {
        return Err(RuneError::message(
            1,
            "test: no `.srt` files found under `./lib`",
        ));
    }

    let mut all_tests = Vec::new();
    for (path, source) in lib_sources {
        let file_path = display_path(&path);
        let module_path = derive_primary_module_path(&source)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| module_path_from_file_name(&path));
        let tests = collect_test_cases_from_source(&file_path, &source, &module_path)
            .map_err(|message| RuneError::message(1, message))?;
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

    let module_inputs = collect_additional_std_module_inputs(env)?;
    let module_sources = if module_inputs.is_empty() {
        xldr::collect_module_sources_with_module_stages(&[])
    } else {
        xldr::collect_module_sources_with_std_module_stages(&[module_inputs])
    }
    .map_err(|e| RuneError::message(1, format!("test: failed to collect module sources: {}", e)))?;

    let mut passed = 0usize;
    let mut failed = 0usize;
    for test in selected_tests {
        match evaluate_expression(
            &module_sources,
            &test.module_path,
            &test.target_def,
            &test.expr,
            env,
        ) {
            Ok((Value::Bool(true), _)) => {
                println!("[PASS] {}", test.display_name());
                passed += 1;
            }
            Ok((Value::Bool(false), _)) => {
                report_test_failure(&module_sources, &test, "", env);
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
                    env,
                );
                failed += 1;
            }
            Err(error) => {
                report_test_failure(&module_sources, &test, &error.summary(), env);
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
        Err(RuneError::silent(1))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_test_options;

    #[test]
    fn test_options_accept_selector_or_empty() {
        let with_selector =
            parse_test_options(&["Kernel::add".to_string()]).expect("selector should parse");
        assert_eq!(with_selector.selector.as_deref(), Some("Kernel::add"));

        let without_selector = parse_test_options(&[]).expect("empty selector should parse");
        assert_eq!(without_selector.selector, None);
    }
}
