//! Script include syntax and occurrence layout shared by compiler entry points.
use std::path::{Path, PathBuf};

use sindr::policy::{CompileUnitKind, SourceKind};
use spire::ast::{Ast, Span};

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptIncludeDirective {
    pub file_path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptIncludeOrigin {
    pub stage_index: usize,
    pub source_index: usize,
    pub directive: ScriptIncludeDirective,
}

/// Preserve each source occurrence, including repeated paths, in one stage.
pub fn script_include_origins(
    stage_index: usize,
    directives: &[ScriptIncludeDirective],
) -> Vec<ScriptIncludeOrigin> {
    directives
        .iter()
        .enumerate()
        .map(|(source_index, directive)| ScriptIncludeOrigin {
            stage_index,
            source_index,
            directive: directive.clone(),
        })
        .collect()
}

pub fn collect_script_include_directives(
    source: &str,
    source_kind: SourceKind,
) -> Result<(String, Vec<ScriptIncludeDirective>), spire::error::ParseError> {
    let ast = spire::parse_with_context(
        source,
        spire::parser_context_for_source_policy(
            0,
            source_kind.policy(CompileUnitKind::Script, None),
            None,
        ),
    )?;
    let directives = script_include_directives(&ast);
    let mut chars = source.chars().collect::<Vec<_>>();
    for directive in &directives {
        for ch in chars
            .iter_mut()
            .take(directive.span.end)
            .skip(directive.span.start)
        {
            if *ch != '\n' {
                *ch = ' ';
            }
        }
    }
    Ok((chars.into_iter().collect(), directives))
}

pub fn script_include_directives(ast: &[Ast]) -> Vec<ScriptIncludeDirective> {
    ast.iter()
        .filter_map(|stmt| match stmt {
            Ast::Include(span, file_path) => Some(ScriptIncludeDirective {
                file_path: file_path.clone(),
                span: span.clone(),
            }),
            _ => None,
        })
        .collect()
}

pub fn resolve_script_include_file_path(script_file_path: &Path, raw_path: &str) -> PathBuf {
    let candidate = Path::new(raw_path);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    script_file_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(candidate)
}

pub fn include_policy_description(spec: &diagnostics::DiagnosticSpec) -> Option<&'static str> {
    use diagnostics::{
        DiagnosticData, DiagnosticReason, ParseDiagnosticGuidance, ParseDiagnosticReason,
    };
    let cause = spec.structured.as_ref()?;
    match cause.reason {
        DiagnosticReason::Parse(ParseDiagnosticReason::UserBuiltinDeclaration) => Some("@builtin"),
        DiagnosticReason::Parse(ParseDiagnosticReason::UserStandardTypeDeclaration) => {
            Some("@FacetPathKind Type")
        }
        DiagnosticReason::Parse(ParseDiagnosticReason::SourcePolicy) => {
            let DiagnosticData::Parse(data) = &cause.data else {
                unreachable!("parser policy data");
            };
            match data.guidance {
                Some(ParseDiagnosticGuidance::TopLevelDeclaration) => {
                    Some("this top-level declaration in a module compile unit")
                }
                Some(ParseDiagnosticGuidance::TopLevelExpression) => {
                    Some("a top-level expression in a module compile unit")
                }
                _ => None,
            }
        }
        _ => None,
    }
}
