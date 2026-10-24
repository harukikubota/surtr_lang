use diagnostics::{
    DiagnosticData, DiagnosticLabel, DiagnosticReason, DiagnosticSpec, ParseDiagnosticReason,
    SourceFact, SourceId, SourceRegistry, SourceRole,
};

use crate::{CompileSources, ModuleInput, ScriptIncludeDirective};

/// Attach include origins to producer-classified parser source restrictions.
/// Other parser failures retain their original location.
pub fn attach_include_diagnostic(
    sources: &SourceRegistry,
    script_source_id: SourceId,
    source_id: SourceId,
    spec: DiagnosticSpec,
    directives: &[ScriptIncludeDirective],
    modules: &[ModuleInput],
) -> (SourceId, DiagnosticSpec) {
    let Some(cause) = spec.structured.as_ref() else {
        return (source_id, spec);
    };
    let description = match cause.reason {
        DiagnosticReason::Parse(ParseDiagnosticReason::UserBuiltinDeclaration) => "@builtin",
        DiagnosticReason::Parse(ParseDiagnosticReason::UserStandardTypeDeclaration) => {
            "@FacetPathKind Type"
        }
        DiagnosticReason::Parse(ParseDiagnosticReason::SourcePolicy) => {
            let DiagnosticData::Parse(data) = &cause.data else {
                unreachable!("parser policy data")
            };
            match data.guidance {
                Some(diagnostics::ParseDiagnosticGuidance::TopLevelDeclaration) => {
                    "this top-level declaration in a module compile unit"
                }
                Some(diagnostics::ParseDiagnosticGuidance::TopLevelExpression) => {
                    "a top-level expression in a module compile unit"
                }
                _ => return (source_id, spec),
            }
        }
        _ => return (source_id, spec),
    };
    let Some((directive, _)) = directives
        .iter()
        .zip(modules)
        .find(|(_, module)| sources.file_name(source_id) == Some(module.file_name.as_str()))
    else {
        return (source_id, spec);
    };
    attach_at(
        script_source_id,
        source_id,
        spec,
        directive,
        Some(description),
    )
}

/// Resolve collision locations while the resolver's source occurrence is still
/// available. A SourceId alone does not distinguish repeated includes.
pub fn attach_resolve_include_diagnostic(
    compile_sources: &CompileSources,
    error: &sigil::error::ResolveError,
    source_id: SourceId,
    spec: DiagnosticSpec,
    directives: &[ScriptIncludeDirective],
) -> (SourceId, DiagnosticSpec) {
    if error.diagnostic.reason != sigil::error::ResolveErrorReason::DeclarationCollision {
        return (source_id, spec);
    }
    assert_eq!(
        error.related_labels.len(),
        2,
        "owner collisions retain both declarations"
    );
    error.related_labels[0]
        .source
        .expect("first declaration provenance");
    let conflicting = error.related_labels[1]
        .source
        .expect("conflicting declaration provenance");
    let include_start = compile_sources
        .module_stages
        .len()
        .checked_sub(directives.len())
        .expect("each include occupies a module stage");
    if conflicting.stage_index < include_start
        || conflicting.stage_index >= compile_sources.module_stages.len()
    {
        return (source_id, spec);
    }
    let stage = &compile_sources.module_stages[conflicting.stage_index];
    assert_eq!(stage.len(), 1, "each include stage has one source");
    assert_eq!(conflicting.source_index, 0, "include source index");
    assert_eq!(
        stage[0].source_id, source_id,
        "conflicting source occurrence"
    );
    let directive = &directives[conflicting.stage_index - include_start];
    attach_at(
        compile_sources.user_source_id,
        source_id,
        spec,
        directive,
        None,
    )
}

fn attach_at(
    script_source_id: SourceId,
    source_id: SourceId,
    mut spec: DiagnosticSpec,
    directive: &ScriptIncludeDirective,
    description: Option<&str>,
) -> (SourceId, DiagnosticSpec) {
    let cause = spec
        .structured
        .as_ref()
        .expect("classified structured include cause");
    spec.message = if let Some(description) = description {
        format!(
            "Cannot include '{}': {description} is not allowed in user-defined source.",
            directive.file_path
        )
    } else {
        let DiagnosticData::Resolve(data) = &cause.data else {
            unreachable!("resolver collision data")
        };
        let symbol = data.subject.as_deref().expect("collision subject");
        format!(
            "Cannot include '{}': symbol '{symbol}' is already defined.",
            directive.file_path
        )
    };
    spec.help = Some(if description.is_none() {
        "Remove the include when the existing definition is already available, or give the user-defined symbol a distinct name."
    } else if matches!(cause.reason, DiagnosticReason::Parse(ParseDiagnosticReason::SourcePolicy)) {
        "include loads user-defined module source. Put helper functions inside defmod and executable expressions inside function bodies."
    } else {
        "include loads user-defined source, which cannot declare standard/internal definitions."
    }.into());
    for label in &mut spec.labels {
        if label.source_id.is_none() {
            label.source_id = Some(source_id);
        }
    }
    spec.labels.insert(
        0,
        DiagnosticLabel {
            source_id: Some(source_id),
            span: spec.primary_span.clone(),
            message: if description.is_none() {
                "conflicting declaration"
            } else {
                "declaration rejected in user-defined source"
            }
            .into(),
            color: Some(diagnostics::Color::Red),
        },
    );
    spec.primary_span = directive.span.clone();
    spec.labels.insert(
        0,
        DiagnosticLabel {
            source_id: Some(script_source_id),
            span: directive.span.clone(),
            message: spec.message.clone(),
            color: Some(diagnostics::Color::Red),
        },
    );
    let cause = spec
        .structured
        .as_mut()
        .expect("classified structured cause");
    let original = std::mem::replace(
        &mut cause.primary,
        SourceFact::untyped(SourceRole::Other, script_source_id, directive.span.clone()),
    );
    cause.related.insert(
        0,
        SourceFact {
            role: SourceRole::Declaration,
            ..original
        },
    );
    if let DiagnosticData::Resolve(data) = &mut cause.data {
        data.related_labels
            .insert(0, "conflicting declaration".into());
    }
    (script_source_id, spec)
}
