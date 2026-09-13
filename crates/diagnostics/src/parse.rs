use crate::{
    simple_error, Color, DiagnosticData, DiagnosticOrigin, DiagnosticReason, DiagnosticSpec,
    ParseDiagnosticData, ParseDiagnosticGuidance, ParseDiagnosticReason, SourceFact, SourceId,
    SourceRole, StructuredDiagnostic,
};
use spire::ast::Span;
use spire::error::{ParseError, ParseErrorGuidance, ParseErrorReason};

pub fn parse_error_spec(source_id: SourceId, source: &str, error: &ParseError) -> DiagnosticSpec {
    let span = error.span().clone();
    let mut spec = simple_error("ParseError", error.message(), span.clone(), None);
    let data = ParseDiagnosticData {
        detail: error.detail().to_string(),
        expected_tokens: error.expected_tokens().to_vec(),
        cursor_span: error.cursor_span().clone(),
        guidance: error.guidance().map(map_guidance),
        token_kind: error.token_kind().map(str::to_owned),
    };
    let reason = map_reason(error.reason());
    spec.structured = Some(StructuredDiagnostic {
        reason: DiagnosticReason::Parse(reason),
        origin: DiagnosticOrigin::Parse,
        data: DiagnosticData::Parse(data),
        primary: SourceFact::untyped(SourceRole::Other, source_id, span.clone()),
        related: Vec::new(),
        remediation: None,
    });

    match error.guidance() {
        Some(ParseErrorGuidance::UnexpectedToken) => {
            spec.help = Some(
                "The parser stopped at this token. Check the expression immediately before it."
                    .into(),
            );
            if let Some(message) = error.token_kind().and_then(unexpected_token_label) {
                spec.labels.push(crate::DiagnosticLabel {
                    source_id: Some(source_id),
                    span: error.span().clone(),
                    message: message.into(),
                    color: Some(Color::Red),
                });
            }
        }
        Some(ParseErrorGuidance::TopLevelDeclaration) => {
            spec.help = Some(
                "Move this declaration into a module compile unit, or replace it with an expression that is allowed in this source kind."
                    .into(),
            );
            add_line_label(
                source_id,
                source,
                &span,
                "forbidden top-level declaration",
                &mut spec,
            );
        }
        Some(ParseErrorGuidance::TopLevelExpression) => {
            spec.help = Some(
                "This source kind only accepts declarations at the top level. Move the expression into a function or another executable context."
                    .into(),
            );
            add_line_label(
                source_id,
                source,
                &span,
                "top-level expression is not allowed here",
                &mut spec,
            );
        }
        Some(ParseErrorGuidance::UnitPattern) => {
            spec.help = Some("Variable bindings and the `_` wildcard pattern are allowed.".into());
        }
        Some(ParseErrorGuidance::AsPatternAlias) => {
            spec.help = Some(
                "Replace the wildcard alias with a name, for example `pattern @ value`.".into(),
            );
        }
        Some(ParseErrorGuidance::RangeLiteral) => {
            spec.help = Some("Write `[start..stop]`.".into());
        }
        Some(ParseErrorGuidance::OperatorCapture(operator)) => {
            spec.help = Some(format!("Write &`{operator}`."));
        }
        Some(ParseErrorGuidance::PairConstructorCapture) => {
            spec.help =
                Some("Write &`(,)` for a capture, or use `(,)`(right) as a pipeline RHS.".into());
        }
        Some(ParseErrorGuidance::ReturnPositionImplTrait) => {
            spec.help =
                Some("Name the return type parameter explicitly in the function signature.".into());
            add_line_label(
                source_id,
                source,
                &span,
                "return-position `impl Trait` is not supported",
                &mut spec,
            );
        }
        Some(ParseErrorGuidance::WhereClause) => {
            spec.help = Some(
                "Rewrite the constraint as explicit type parameters or defer this API shape until `where` clauses are available."
                    .into(),
            );
            add_line_label(
                source_id,
                source,
                &span,
                "`where` clauses are not available yet",
                &mut spec,
            );
        }
        Some(ParseErrorGuidance::MissingMetaState) => {
            spec.help = Some(
                "Add a state declaration inside `meta { ... }`. For example:\n\n  state: Int"
                    .into(),
            );
            add_previous_line_label(source_id, source, &span, &mut spec);
        }
        Some(ParseErrorGuidance::MissingMetaInstance) => {
            spec.help = Some(
                "Add an instance declaration inside `meta { ... }`. For example:\n\n  instance: Singleton".into(),
            );
            add_previous_line_label(source_id, source, &span, &mut spec);
        }
        Some(ParseErrorGuidance::AnonymousCaptureIdentity) => {
            if let Some(rewrite) = rewrite_line_at_span(source, &span, "&id") {
                spec.help = Some(format!(
                    "Replace this anonymous capture with:\n\n  {rewrite}"
                ));
            }
        }
        Some(ParseErrorGuidance::AnonymousCaptureRequiresHelper) => {
            if let Some(rewrite) = rewrite_line_at_span(source, &span, "&fun_name(&1, &2)") {
                spec.help = Some(format!(
                    "Extract the body into a named helper and replace this capture with:\n\n  {rewrite}"
                ));
            }
        }
        Some(ParseErrorGuidance::ImmediateAnonymousCall) => {
            spec.help = Some(
                "Bind the callable to a name before calling it. For example:\n\n  f = &add(&1, 10)\n  f(4)\n\n  f = {|x| x + 1}\n  f(4)\n\n  tmp = make()\n  tmp(4)"
                    .into(),
            );
            add_line_label(
                source_id,
                source,
                &span,
                "anonymous callable is followed by an immediate call",
                &mut spec,
            );
        }
        Some(ParseErrorGuidance::DoCarrierReturnTypeArgument) => {
            let written = crate::source::slice_chars(source, span.start, span.end);
            let written = (!written.is_empty()).then_some(written);
            let outer_constructor_variable = written
                .as_deref()
                .is_some_and(|argument| argument.starts_with('$'));
            spec.help = Some(match written.as_deref() {
                Some(argument) if argument.starts_with('<') && argument.ends_with('>') => {
                    format!("Write `do::{argument} {{ ... }}`.")
                }
                _ => "Write `do::<Either> { ... }`, `do::<Either<String, _>> { ... }`, or add an expected result type.".into(),
            });
            let label = if outer_constructor_variable {
                "this is an outer constructor variable, not a valid carrier type input"
            } else {
                "invalid `do` carrier return type argument"
            };
            if outer_constructor_variable {
                spec.notes.push(
                    "captured and fixed arguments in an applied carrier are checked against do block constraints"
                        .into(),
                );
            }
            add_line_label(source_id, source, &span, label, &mut spec);
        }
        None => {}
    }

    spec
}

pub fn parse_policy_error_spec(
    source_id: SourceId,
    _source: &str,
    detail: impl Into<String>,
    span: Span,
) -> DiagnosticSpec {
    let detail = detail.into();
    let mut spec = simple_error("ParseError", detail.clone(), span.clone(), None);
    spec.structured = Some(StructuredDiagnostic {
        reason: DiagnosticReason::Parse(ParseDiagnosticReason::SourcePolicy),
        origin: DiagnosticOrigin::Parse,
        data: DiagnosticData::Parse(ParseDiagnosticData {
            detail,
            expected_tokens: Vec::new(),
            cursor_span: span.clone(),
            guidance: None,
            token_kind: None,
        }),
        primary: SourceFact::untyped(SourceRole::Other, source_id, span),
        related: Vec::new(),
        remediation: None,
    });
    spec
}

fn map_reason(reason: ParseErrorReason) -> ParseDiagnosticReason {
    match reason {
        ParseErrorReason::IncompleteInput => ParseDiagnosticReason::IncompleteInput,
        ParseErrorReason::UnexpectedToken => ParseDiagnosticReason::UnexpectedToken,
        ParseErrorReason::DeclarationSyntax => ParseDiagnosticReason::DeclarationSyntax,
        ParseErrorReason::ExpressionSyntax => ParseDiagnosticReason::ExpressionSyntax,
        ParseErrorReason::StatementSyntax => ParseDiagnosticReason::StatementSyntax,
        ParseErrorReason::PatternSyntax => ParseDiagnosticReason::PatternSyntax,
        ParseErrorReason::TypeSyntax => ParseDiagnosticReason::TypeSyntax,
        ParseErrorReason::LiteralSyntax => ParseDiagnosticReason::LiteralSyntax,
        ParseErrorReason::PositionRule => ParseDiagnosticReason::PositionRule,
        ParseErrorReason::SourcePolicy => ParseDiagnosticReason::SourcePolicy,
        ParseErrorReason::InterpolationSyntax => ParseDiagnosticReason::InterpolationSyntax,
        ParseErrorReason::ReturnTypeArgumentArityMismatch => {
            ParseDiagnosticReason::ReturnTypeArgumentArityMismatch
        }
        ParseErrorReason::InvalidDoCarrierReturnTypeArgument => {
            ParseDiagnosticReason::InvalidDoCarrierReturnTypeArgument
        }
        ParseErrorReason::CompilerInvariant => ParseDiagnosticReason::CompilerInvariant,
    }
}

fn map_guidance(guidance: &ParseErrorGuidance) -> ParseDiagnosticGuidance {
    match guidance {
        ParseErrorGuidance::UnexpectedToken => ParseDiagnosticGuidance::UnexpectedToken,
        ParseErrorGuidance::TopLevelDeclaration => ParseDiagnosticGuidance::TopLevelDeclaration,
        ParseErrorGuidance::TopLevelExpression => ParseDiagnosticGuidance::TopLevelExpression,
        ParseErrorGuidance::UnitPattern => ParseDiagnosticGuidance::UnitPattern,
        ParseErrorGuidance::AsPatternAlias => ParseDiagnosticGuidance::AsPatternAlias,
        ParseErrorGuidance::RangeLiteral => ParseDiagnosticGuidance::RangeLiteral,
        ParseErrorGuidance::OperatorCapture(operator) => {
            ParseDiagnosticGuidance::OperatorCapture(operator.clone())
        }
        ParseErrorGuidance::PairConstructorCapture => {
            ParseDiagnosticGuidance::PairConstructorCapture
        }
        ParseErrorGuidance::ReturnPositionImplTrait => {
            ParseDiagnosticGuidance::ReturnPositionImplTrait
        }
        ParseErrorGuidance::WhereClause => ParseDiagnosticGuidance::WhereClause,
        ParseErrorGuidance::MissingMetaState => ParseDiagnosticGuidance::MissingMetaState,
        ParseErrorGuidance::MissingMetaInstance => ParseDiagnosticGuidance::MissingMetaInstance,
        ParseErrorGuidance::AnonymousCaptureIdentity => {
            ParseDiagnosticGuidance::AnonymousCaptureIdentity
        }
        ParseErrorGuidance::AnonymousCaptureRequiresHelper => {
            ParseDiagnosticGuidance::AnonymousCaptureRequiresHelper
        }
        ParseErrorGuidance::ImmediateAnonymousCall => {
            ParseDiagnosticGuidance::ImmediateAnonymousCall
        }
        ParseErrorGuidance::DoCarrierReturnTypeArgument => {
            ParseDiagnosticGuidance::DoCarrierReturnTypeArgument
        }
    }
}

fn unexpected_token_label(token_kind: &str) -> Option<&'static str> {
    match token_kind {
        "RParen" => Some("unexpected closing parenthesis"),
        "RBrace" => Some("unexpected closing brace"),
        "RBrack" => Some("unexpected closing bracket"),
        _ => None,
    }
}

fn add_line_label(
    source_id: SourceId,
    source: &str,
    span: &Span,
    message: &str,
    spec: &mut DiagnosticSpec,
) {
    if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
        spec.labels.push(crate::DiagnosticLabel {
            source_id: Some(source_id),
            span: line_span,
            message: message.into(),
            color: Some(Color::Red),
        });
    }
}

fn add_previous_line_label(
    source_id: SourceId,
    source: &str,
    span: &Span,
    spec: &mut DiagnosticSpec,
) {
    if let Some(process_decl_span) = previous_non_empty_line_span(source, span.start) {
        spec.labels.push(crate::DiagnosticLabel {
            source_id: Some(source_id),
            span: process_decl_span,
            message: "process declaration".into(),
            color: Some(Color::Blue),
        });
    }
}

fn previous_non_empty_line_span(source: &str, pos: usize) -> Option<Span> {
    let lines = line_spans(source);
    let current_idx = lines
        .iter()
        .position(|(start, end)| *start <= pos && pos <= *end)?;
    for idx in (0..current_idx).rev() {
        let Some(span) = trimmed_line_span(source, lines[idx]) else {
            continue;
        };
        if span.start < span.end {
            return Some(span);
        }
    }
    None
}

use crate::source::{
    line_spans, rewrite_line_at_span, trimmed_line_span, trimmed_line_span_containing,
};
