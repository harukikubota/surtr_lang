use spire::{
    ast::{Ast, AstPattern, InterpolatedPart, Lit, Span},
    error::ParseErrorReason,
    parse, parse_tolerant_with_context, ParserContext,
};

fn string(source: &str) -> String {
    match parse(source).unwrap().remove(0) {
        Ast::Lit(_, Lit::Str(value)) => value,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn unicode_escape_scalar_boundaries_and_quote_symmetry() {
    for quote in ['"', '\''] {
        for (digits, expected) in [
            ("0", '\0'),
            ("001B", '\u{1b}'),
            ("7f", '\u{7f}'),
            ("80", '\u{80}'),
            ("d7ff", '\u{d7ff}'),
            ("e000", '\u{e000}'),
            ("10ffff", '\u{10ffff}'),
            ("FDD0", '\u{fdd0}'),
            ("1f600", '😀'),
        ] {
            assert_eq!(
                string(&format!("{quote}\\u{{{digits}}}{quote}")),
                expected.to_string()
            );
        }
        assert_eq!(
            string(&format!("{quote}\\n\\t\\\"\\'\\\\{quote}")),
            "\n\t\"'\\"
        );
    }
}

#[test]
fn invalid_escape_diagnostics_and_original_spans() {
    for (escape, message) in [
        (r"\q", "Unsupported string escape"),
        (r"\e", "Unsupported string escape"),
        (r"\r", "Unsupported string escape"),
        (r"\0", "Unsupported string escape"),
        (r"\x1b", "Unsupported string escape"),
        (r"\#", "Unsupported string escape"),
        (r"\u1234", "Expected '{'"),
        (r"\u{1b", "Expected '}'"),
        (r"\u{}", "at least one"),
        (r"\u{xyz}", "hexadecimal"),
        (r"\u{1_b}", "hexadecimal"),
        (r"\u{ 1b}", "hexadecimal"),
        (r"\u{000001b}", "six"),
        (r"\u{110000}", "range"),
        (r"\u{d800}", "surrogate"),
        (r"\u{dfff}", "surrogate"),
    ] {
        let source = format!("\"あ{escape}\"");
        let error = parse(&source).expect_err(&source);
        assert_eq!(
            error.reason(),
            ParseErrorReason::LiteralSyntax,
            "{source}: {error}"
        );
        assert!(error.message().contains(message), "{source}: {error}");
        assert_eq!(error.span().start, 2, "{source}: {error}");
    }
    let error = parse(r##""あ\u{d800}""##).unwrap_err();
    assert_eq!(*error.span(), Span { start: 2, end: 10 });
}

#[test]
fn incomplete_escape_prefixes_and_impossible_prefixes() {
    for source in [
        "\"",
        "\"\\",
        "\"\\u",
        "\"\\u{",
        "\"\\u{1b",
        "\"\\u{d800",
        "\"\\#",
    ] {
        assert!(parse(source).unwrap_err().is_incomplete(), "{source}");
    }
    for source in [
        "\"\\q",
        "\"\\uX",
        "\"\\u{g",
        "\"\\u{0000000",
        "\"\\u{110000",
        "\"\\u{00d800",
        "\"#{'\\q",
        "\"\\#x",
        "\"\\#\"",
    ] {
        let error = parse(source).unwrap_err();
        assert_eq!(
            error.reason(),
            ParseErrorReason::LiteralSyntax,
            "{source}: {error}"
        );
    }
}

#[test]
fn interpolation_uses_source_boundaries_and_backslash_parity() {
    for (source, value) in [
        (r##""\#{name}""##, "#{name}"),
        (r##""\u{23}{name}""##, "#{name}"),
        (r##""#\u{7b}name}""##, "#{name}"),
        (r##""\u{22}""##, "\""),
        (r##""\\\#{name}""##, "\\#{name}"),
        (r##""\\u{1b}""##, r"\u{1b}"),
    ] {
        assert_eq!(string(source), value);
    }
    for source in [r##""\\#{name}""##, r##""\u{5c}#{name}""##] {
        let parsed = parse(source).unwrap();
        assert!(
            matches!(&parsed[0], Ast::InterpolatedStr(_, parts) if matches!(&parts[..], [InterpolatedPart::Text(text), InterpolatedPart::Expr(_)] if text == "\\"))
        );
    }
}

#[test]
fn interpolation_inner_literals_are_independent_and_keep_source_spans() {
    let source = r##""\u{1b}#{"\u{7d}"}""##;
    let parsed = parse(source).unwrap();
    match &parsed[0] {
        Ast::InterpolatedStr(_, parts) => match &parts[1] {
            InterpolatedPart::Expr(expr) => assert!(
                matches!(expr.as_ref(), Ast::Lit(span, Lit::Str(value)) if value == "}" && span.start == 9)
            ),
            _ => panic!(),
        },
        _ => panic!(),
    }
    let source = r##""あ\u{1b}#{'\q'}""##;
    let error = parse(source).unwrap_err();
    assert_eq!(error.reason(), ParseErrorReason::LiteralSyntax);
    assert_eq!(
        error.span().start,
        source.chars().position(|c| c == '\'').unwrap() + 1
    );
}

#[test]
fn static_string_consumers_decode_without_evaluating_interpolation() {
    let parsed = parse(r##"include "\u{61}\#{name}.srt""##).unwrap();
    assert!(matches!(&parsed[0], Ast::Include(_, path) if path == "a#{name}.srt"));
    let parsed = parse(r##"match value { "\u{1b}\#{name}" => 1 }"##).unwrap();
    assert!(
        matches!(&parsed[0], Ast::Match(_, _, arms) if matches!(&arms[0].pattern, AstPattern::StrLit(_, value) if value == "\u{1b}#{name}"))
    );
}

#[test]
fn regex_uses_normal_string_escape_contract() {
    assert!(parse(r##"re"\\d""##).is_ok());
    assert!(parse(r##"re"\u{1b}""##).is_ok());
    assert_eq!(
        parse(r##"re"\d""##).unwrap_err().reason(),
        ParseErrorReason::LiteralSyntax
    );
}

#[test]
fn tolerant_parser_shares_values_diagnostics_and_recovery() {
    let source = r##"valid = "\u{1b}""##;
    let result = parse_tolerant_with_context(source, ParserContext::script(0), None);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(
        matches!(&result.ast[0], Ast::Bind(_, _, expr) if matches!(expr.as_ref(), Ast::Lit(_, Lit::Str(value)) if value == "\u{1b}"))
    );
    let source = "bad = \"\\q\"\nnext = 2";
    let result = parse_tolerant_with_context(source, ParserContext::script(0), None);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.error.reason() == ParseErrorReason::LiteralSyntax && d.error.span().start == 7));
    assert!(!result
        .ast
        .iter()
        .any(|node| matches!(node, Ast::Bind(_, AstPattern::Var(_, name), _) if name == "bad")));
    assert!(result
        .ast
        .iter()
        .any(|node| matches!(node, Ast::Bind(_, AstPattern::Var(_, name), _) if name == "next")));
}

#[test]
fn static_consumers_reject_source_interpolation() {
    for (source, context) in [
        (r##"include "#{name}""##, ParserContext::script(0)),
        (
            r##"match value { "#{name}" => 1 }"##,
            ParserContext::script(0),
        ),
        (
            r##"supervisor_init { Logger { handlers { out: FileOutHandler(path: "#{name}") } } }"##,
            ParserContext::project(0),
        ),
    ] {
        let error = spire::parse_with_context(source, context).unwrap_err();
        assert_eq!(error.reason(), ParseErrorReason::LiteralSyntax);
        assert!(
            error
                .message()
                .contains("String interpolation is not allowed in static string literals"),
            "{error}"
        );
        let marker = source
            .chars()
            .collect::<Vec<_>>()
            .windows(2)
            .position(|chars| chars == ['#', '{'])
            .unwrap();
        assert_eq!(
            *error.span(),
            Span {
                start: marker,
                end: marker + "#{name}".len()
            }
        );
    }
}

#[test]
fn malformed_source_markers_are_rejected_and_escaped_markers_are_static() {
    for content in ["#{", "#{#}", "#{\\u{61}", "#{\\q}"] {
        assert!(parse(&format!("include \"{content}\"")).is_err());
        // Escaping the source marker keeps the complete contents static, with
        // normal Unicode decoding and unknown-escape rejection in its text.
        let source = format!("include \"\\{content}\"");
        if content == "#{\\q}" {
            assert_eq!(
                parse(&source).unwrap_err().reason(),
                ParseErrorReason::LiteralSyntax
            );
        } else {
            let expected = content.replace(r"\u{61}", "a");
            assert!(
                matches!(&parse(&source).unwrap()[0], Ast::Include(_, value) if value == &expected)
            );
        }
    }
}

#[test]
fn unicode_generated_markers_are_static_for_every_consumer() {
    let ast = parse(r##"include "\u{23}{name}""##).unwrap();
    assert!(matches!(&ast[0], Ast::Include(_, value) if value == "#{name}"));
    let ast = parse(r##"match value { "#\u{7b}name}" => 1 }"##).unwrap();
    assert!(
        matches!(&ast[0], Ast::Match(_, _, arms) if matches!(&arms[0].pattern, AstPattern::StrLit(_, value) if value == "#{name}"))
    );
    let ast = spire::parse_with_context(r##"supervisor_init { Logger { handlers { out: FileOutHandler(path: "\u{23}{name}") } } }"##, ParserContext::project(0)).unwrap();
    assert!(
        matches!(&ast[0], Ast::SupervisorInit(_, spec) if spec.entries[0].handlers[0].target.named_args[0].value == "#{name}")
    );
}

#[test]
fn tolerant_parser_rejects_static_interpolation_without_evaluating_it() {
    let source = "include \"#{name}\"\nnext = 2";
    let result = parse_tolerant_with_context(source, ParserContext::script(0), None);
    assert!(result
        .diagnostics
        .iter()
        .any(
            |diagnostic| diagnostic.error.reason() == ParseErrorReason::LiteralSyntax
                && diagnostic
                    .error
                    .message()
                    .contains("String interpolation is not allowed in static string literals")
        ));
    assert!(!result
        .ast
        .iter()
        .any(|node| matches!(node, Ast::Include(_, _))));
    assert!(result
        .ast
        .iter()
        .any(|node| matches!(node, Ast::Bind(_, AstPattern::Var(_, name), _) if name == "next")));
}

#[test]
fn raw_literals_and_doc_bodies_preserve_unicode_and_unknown_escapes() {
    assert_eq!(string("\"\"\"\\u{1b}\\q\"\"\""), r"\u{1b}\q");
    assert_eq!(string("\"\"\"\\#{name}\"\"\""), "#{name}");
    assert!(parse("@doc \"\"\"\\u{1b}\\q\"\"\"\ndef example() -> Unit { () }").is_ok());
}

#[test]
fn interpolation_spans_use_source_character_offsets_after_decoding() {
    let source = r##""あ\u{1b}#{value}""##;
    let ast = parse(source).unwrap();
    assert!(
        matches!(&ast[0], Ast::InterpolatedStr(_, parts) if matches!(&parts[1], InterpolatedPart::Expr(expr) if matches!(expr.as_ref(), Ast::Var(span, value) if *span == (Span {start: 10, end: 15}) && value == "value")))
    );
    let source = r##""あ\u{1b}#{)}""##;
    let error = parse(source).unwrap_err();
    assert_eq!(*error.span(), Span { start: 10, end: 11 });
}

#[test]
fn interpolation_braces_in_comments_do_not_close_expression() {
    let ast = parse("\"#{value # } ignored\n}\"").unwrap();
    assert!(
        matches!(&ast[0], Ast::InterpolatedStr(_, parts) if matches!(&parts[0], InterpolatedPart::Expr(expr) if matches!(expr.as_ref(), Ast::Var(_, value) if value == "value")))
    );
}

#[test]
fn nested_string_interpolation_obeys_parse_nesting_limit() {
    let source = format!("{}0{}", "\"#{".repeat(40), "}\"".repeat(40));
    let error = parse(&source).unwrap_err();
    assert_eq!(error.reason(), ParseErrorReason::PositionRule);
    assert!(error.message().contains("maximum parse nesting depth"));
}

#[test]
fn nested_interpolation_retains_parent_brace_nesting_budget() {
    let mut source = "0".to_string();
    for _ in 0..5 {
        source = format!("\"#{{{}{}{}}}\"", "{".repeat(8), source, "}".repeat(8));
    }
    let error = parse(&source).unwrap_err();
    assert_eq!(error.reason(), ParseErrorReason::PositionRule);
    assert!(error.message().contains("maximum parse nesting depth"));
}

#[test]
fn static_declaration_string_arguments_share_escape_rules() {
    let source = r##"supervisor_init {
  Logger {
    handlers {
      out: FileOutHandler(path: "\u{61}\#{name}")
    }
  }
}"##;
    let ast = spire::parse_with_context(source, ParserContext::project(0)).unwrap();
    assert!(
        matches!(&ast[0], Ast::SupervisorInit(_, spec) if spec.entries[0].handlers[0].target.named_args[0].value == "a#{name}")
    );
    let source = source.replace(r"\u{61}", r"\q");
    assert_eq!(
        spire::parse_with_context(&source, ParserContext::project(0))
            .unwrap_err()
            .reason(),
        ParseErrorReason::LiteralSyntax
    );
}

#[test]
fn raw_interpolation_dedent_maps_multiline_ast_to_original_source() {
    let source = "    \"\"\"あ\n    #{{left\n      right}}\n    #{last}\n    \"\"\"";
    let ast = parse(source).unwrap();
    let Ast::InterpolatedStr(_, parts) = &ast[0] else {
        panic!("{ast:?}")
    };
    let expressions: Vec<_> = parts
        .iter()
        .filter_map(|part| match part {
            InterpolatedPart::Expr(expr) => Some(expr.as_ref()),
            _ => None,
        })
        .collect();
    let source_span = |needle: &str| {
        let start = source[..source.find(needle).unwrap()].chars().count();
        Span {
            start,
            end: start + needle.chars().count(),
        }
    };
    let Ast::Closure(span, _, body) = expressions[0] else {
        panic!("{expressions:?}")
    };
    let Ast::Block(_, values) = body.as_ref() else {
        panic!("{body:?}")
    };
    assert_eq!(*span, source_span("{left\n      right}"));
    assert!(matches!(&values[0], Ast::Var(span, _) if *span == source_span("left")));
    assert!(matches!(&values[1], Ast::Var(span, _) if *span == source_span("right")));
    assert!(matches!(expressions[1], Ast::Var(span, _) if *span == source_span("last")));
    let tolerant = parse_tolerant_with_context(source, ParserContext::script(0), None);
    assert!(
        tolerant.diagnostics.is_empty(),
        "{:?}",
        tolerant.diagnostics
    );
    assert_eq!(tolerant.ast, ast);
}

#[test]
fn raw_interpolation_dedent_preserves_parse_error_metadata() {
    let source = "    \"\"\"あ\n    #{\n      )}\n    \"\"\"";
    let error = parse(source).unwrap_err();
    let direct = parse("\n  )").unwrap_err();
    let start = source[..source.find(')').unwrap()].chars().count();
    assert_eq!(
        *error.span(),
        Span {
            start,
            end: start + 1
        }
    );
    assert_eq!(
        *error.cursor_span(),
        Span {
            start,
            end: start + 1
        }
    );
    assert_eq!(error.reason(), ParseErrorReason::InterpolationSyntax);
    assert_eq!(
        error.message(),
        format!("Invalid interpolation expression: {}", direct.message())
    );
    assert_eq!(error.expected_tokens(), direct.expected_tokens());
    assert!(!error.is_incomplete());
    let tolerant = parse_tolerant_with_context(source, ParserContext::script(0), None);
    assert!(
        tolerant.diagnostics.iter().any(|diag| diag.error == error),
        "{:?}",
        tolerant.diagnostics
    );
}

#[test]
fn raw_interpolation_maps_tabs_blank_lines_and_empty_error_cursor() {
    let source = "\t\"\"\"あ\r\n\t  \r\n\t#{value}\r\n\t\"\"\"";
    let ast = parse(source).unwrap();
    let Ast::InterpolatedStr(_, parts) = &ast[0] else {
        panic!("{ast:?}")
    };
    let start = source[..source.find("value").unwrap()].chars().count();
    assert!(parts.iter().any(|part| matches!(part, InterpolatedPart::Expr(expr)
        if matches!(expr.as_ref(), Ast::Var(span, _) if *span == (Span { start, end: start + 5 })))));
    let tolerant = parse_tolerant_with_context(source, ParserContext::script(0), None);
    assert_eq!(tolerant.ast, ast);
    assert!(tolerant.diagnostics.is_empty());

    let source = "    \"\"\"\n    #{(}\n    \"\"\"";
    let error = parse(source).unwrap_err();
    let direct = parse("(").unwrap_err();
    let start = source.find('}').unwrap();
    assert_eq!(*error.cursor_span(), Span { start, end: start });
    assert!(!error.is_incomplete());
    assert_eq!(error.reason(), ParseErrorReason::InterpolationSyntax);
    assert_eq!(
        error.message(),
        format!("Invalid interpolation expression: {}", direct.message())
    );
    assert_eq!(error.expected_tokens(), direct.expected_tokens());
}

#[test]
fn tolerant_raw_strings_reject_under_indentation_and_recover() {
    let source = "    \"\"\"\n  invalid\n    \"\"\"\nnext = 2";
    let error = parse(source).unwrap_err();
    let tolerant = parse_tolerant_with_context(source, ParserContext::script(0), None);
    assert!(tolerant.diagnostics.iter().any(|diag| diag.error == error));
    assert!(tolerant
        .ast
        .iter()
        .any(|ast| matches!(ast, Ast::Bind(_, _, _))));
}
