use crate::heuristics::{
    char_span_to_byte_range, extract_expected_got, has_annotation_assignment_labels,
    has_duplicate_definition_labels, has_duplicate_pattern_binding_labels,
    has_missing_trait_method_labels, has_parse_focus_labels, has_runtime_error_focus_labels,
    has_runtime_safebind_labels, has_total_bind_pattern_labels,
    has_trait_impl_signature_mismatch_labels, is_flow_operator_message, line_column_for_offset,
    line_index_for_span, line_spans, normalized_char_span, parse_binary_operator_error,
    serializable_callable_hint_from_labels, should_render_related_label_with_own_source,
};
use crate::{
    Color, DiagnosticData, DiagnosticSpec, SerializableDiagnostic, SerializableDiagnosticReport,
    SerializableSourceFact, SourceId, SourceRegistry,
};
use ariadne::{Label, Report, ReportKind};
use std::fmt;
use std::io::{self, Write};

pub fn report_error(file_name: &str, source: &str, spec: DiagnosticSpec) {
    let report = build_report(file_name, source, &spec);
    let cache = ariadne::sources([
        (
            RenderSourceId::Primary(file_name.to_string()),
            source.to_string(),
        ),
        (
            RenderSourceId::Related(file_name.to_string()),
            source.to_string(),
        ),
    ]);

    if let Err(err) = report.eprint(cache) {
        let mut stderr = io::stderr().lock();
        let _ = write_fallback_diagnostic(&mut stderr, file_name, &spec, &err);
    }
}

pub fn render_error(file_name: &str, source: &str, spec: &DiagnosticSpec) -> String {
    let report = build_report(file_name, source, spec);
    let mut buf = Vec::new();
    let cache = ariadne::sources([
        (
            RenderSourceId::Primary(file_name.to_string()),
            source.to_string(),
        ),
        (
            RenderSourceId::Related(file_name.to_string()),
            source.to_string(),
        ),
    ]);

    if let Err(err) = report.write(cache, &mut buf) {
        let _ = write_fallback_diagnostic(&mut buf, file_name, spec, &err);
    }

    String::from_utf8_lossy(&buf).into_owned()
}

pub fn report_error_by_id(sources: &SourceRegistry, source_id: SourceId, spec: DiagnosticSpec) {
    if let Some((report, cache)) = build_report_with_registry(sources, source_id, &spec) {
        if let Err(err) = report.eprint(ariadne::sources(cache)) {
            if let Some(entry) = sources.get(source_id) {
                let mut stderr = io::stderr().lock();
                let _ = write_fallback_diagnostic(&mut stderr, &entry.file_name, &spec, &err);
            }
        }
    } else {
        report_error("<unknown>", "", spec);
    }
}

pub fn render_error_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    spec: &DiagnosticSpec,
) -> String {
    if let Some((report, cache)) = build_report_with_registry(sources, source_id, spec) {
        let mut buf = Vec::new();
        if let Err(err) = report.write(ariadne::sources(cache), &mut buf) {
            if let Some(entry) = sources.get(source_id) {
                let _ = write_fallback_diagnostic(&mut buf, &entry.file_name, spec, &err);
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        render_error("<unknown>", "", spec)
    }
}

fn build_report(
    file_name: &str,
    source: &str,
    spec: &DiagnosticSpec,
) -> Report<'static, (RenderSourceId, std::ops::Range<usize>)> {
    let primary = normalized_char_span(source, &spec.primary_span);
    let primary_range = char_span_to_byte_range(source, &primary);
    let lines = line_spans(source);
    let primary_line = line_index_for_span(&lines, primary.start);
    let suppress_primary_label = spec.kind == "TypeError"
        && (is_flow_operator_message(&spec.message)
            || parse_binary_operator_error(&spec.message).is_some()
            || has_annotation_assignment_labels(spec))
        || has_duplicate_definition_labels(spec)
        || has_duplicate_pattern_binding_labels(spec)
        || has_missing_trait_method_labels(spec)
        || has_trait_impl_signature_mismatch_labels(spec)
        || has_total_bind_pattern_labels(spec)
        || has_parse_focus_labels(spec)
        || has_runtime_safebind_labels(spec)
        || has_runtime_error_focus_labels(spec);
    let primary_source = RenderSourceId::Primary(file_name.to_string());
    let related_source = RenderSourceId::Related(file_name.to_string());
    let mut builder = Report::build(
        ReportKind::Error,
        (primary_source.clone(), primary_range.clone()),
    )
    .with_message(format!("{}: {}", spec.kind, spec.message));

    if !suppress_primary_label {
        builder = builder.with_label(
            Label::new((primary_source.clone(), primary_range))
                .with_message(spec.message.clone())
                .with_color(Color::Red),
        );
    }

    for label in &spec.labels {
        let span = normalized_char_span(source, &label.span);
        let range = char_span_to_byte_range(source, &span);
        let label_source =
            if should_render_related_label_with_own_source(spec, primary_line, &lines, &span) {
                related_source.clone()
            } else {
                primary_source.clone()
            };
        builder = builder.with_label(match label.color {
            Some(color) => Label::new((label_source, range))
                .with_message(label.message.clone())
                .with_color(color),
            None => Label::new((label_source, range)).with_message(label.message.clone()),
        });
    }

    for note in &spec.notes {
        builder = builder.with_note(note.clone());
    }

    if let Some(h) = &spec.help {
        builder = builder.with_help(h.clone());
    }

    builder.finish()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RenderSourceId {
    Primary(String),
    Related(String),
    Auxiliary(SourceId, usize, String),
}

impl fmt::Display for RenderSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderSourceId::Primary(file_name)
            | RenderSourceId::Related(file_name)
            | RenderSourceId::Auxiliary(_, _, file_name) => f.write_str(file_name),
        }
    }
}

fn build_report_with_registry(
    sources: &SourceRegistry,
    source_id: SourceId,
    spec: &DiagnosticSpec,
) -> Option<(
    Report<'static, (RenderSourceId, std::ops::Range<usize>)>,
    Vec<(RenderSourceId, String)>,
)> {
    let primary_entry = sources.get(source_id)?;
    let primary_source = primary_entry.source.as_str();
    let primary_file_name = primary_entry.file_name.clone();
    let primary = normalized_char_span(primary_source, &spec.primary_span);
    let primary_range = char_span_to_byte_range(primary_source, &primary);
    let lines = line_spans(primary_source);
    let primary_line = line_index_for_span(&lines, primary.start);
    let suppress_primary_label = spec.kind == "TypeError"
        && (is_flow_operator_message(&spec.message)
            || parse_binary_operator_error(&spec.message).is_some()
            || has_annotation_assignment_labels(spec))
        || has_duplicate_definition_labels(spec)
        || has_duplicate_pattern_binding_labels(spec)
        || has_missing_trait_method_labels(spec)
        || has_trait_impl_signature_mismatch_labels(spec)
        || has_total_bind_pattern_labels(spec)
        || has_parse_focus_labels(spec)
        || has_runtime_safebind_labels(spec)
        || has_runtime_error_focus_labels(spec);
    let primary_render_source = RenderSourceId::Primary(primary_file_name.clone());
    let related_render_source = RenderSourceId::Related(primary_file_name.clone());
    let mut builder = Report::build(
        ReportKind::Error,
        (primary_render_source.clone(), primary_range.clone()),
    )
    .with_message(format!("{}: {}", spec.kind, spec.message));

    if !suppress_primary_label {
        builder = builder.with_label(
            Label::new((primary_render_source.clone(), primary_range))
                .with_message(spec.message.clone())
                .with_color(Color::Red),
        );
    }

    let mut cache = vec![
        (primary_render_source.clone(), primary_source.to_string()),
        (related_render_source.clone(), primary_source.to_string()),
    ];

    for (label_index, label) in spec.labels.iter().enumerate() {
        let label_source_id = label.source_id.unwrap_or(source_id);
        let Some(label_entry) = sources.get(label_source_id) else {
            continue;
        };
        let label_span = normalized_char_span(&label_entry.source, &label.span);
        let label_range = char_span_to_byte_range(&label_entry.source, &label_span);
        let label_render_source = if label.source_id.is_none() && label_source_id == source_id {
            if should_render_related_label_with_own_source(spec, primary_line, &lines, &label_span)
            {
                related_render_source.clone()
            } else {
                primary_render_source.clone()
            }
        } else {
            let render_id = RenderSourceId::Auxiliary(
                label_source_id,
                label_index,
                label_entry.file_name.clone(),
            );
            if !cache.iter().any(|(id, _)| id == &render_id) {
                cache.push((render_id.clone(), label_entry.source.clone()));
            }
            render_id
        };
        builder = builder.with_label(match label.color {
            Some(color) => Label::new((label_render_source, label_range))
                .with_message(label.message.clone())
                .with_color(color),
            None => {
                Label::new((label_render_source, label_range)).with_message(label.message.clone())
            }
        });
    }

    for note in &spec.notes {
        builder = builder.with_note(note.clone());
    }

    if let Some(h) = &spec.help {
        builder = builder.with_help(h.clone());
    }

    Some((builder.finish(), cache))
}
pub(crate) fn write_fallback_diagnostic(
    writer: &mut impl Write,
    file_name: &str,
    spec: &DiagnosticSpec,
    render_err: &io::Error,
) -> io::Result<()> {
    writeln!(writer, "diagnostic rendering failed: {}", render_err)?;
    writeln!(writer, "{}: {}", spec.kind, spec.message)?;
    writeln!(
        writer,
        "--> {}:{}-{}",
        file_name, spec.primary_span.start, spec.primary_span.end
    )?;
    for label in &spec.labels {
        writeln!(
            writer,
            "= note: {} [{}-{}]",
            label.message, label.span.start, label.span.end
        )?;
    }
    for note in &spec.notes {
        writeln!(writer, "= note: {}", note)?;
    }
    if let Some(help) = &spec.help {
        for line in help.lines() {
            writeln!(writer, "= help: {}", line)?;
        }
    }
    Ok(())
}

pub fn serializable_report_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    phase: impl Into<String>,
    spec: &DiagnosticSpec,
) -> SerializableDiagnosticReport {
    SerializableDiagnosticReport {
        errors: vec![serializable_diagnostic_by_id(
            sources, source_id, phase, spec,
        )],
    }
}

pub fn serializable_diagnostic_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    phase: impl Into<String>,
    spec: &DiagnosticSpec,
) -> SerializableDiagnostic {
    let phase = phase.into();
    let source = sources.source(source_id).unwrap_or("");
    let (line, column) = line_column_for_offset(source, spec.primary_span.start);
    let (expected, got) = structured_expected_got(spec)
        .filter(|(expected, got)| expected.is_some() || got.is_some())
        .or_else(|| Some(extract_expected_got(&spec.message)))
        .unwrap_or((None, None));
    let hint = spec
        .help
        .clone()
        .or_else(|| serializable_callable_hint_from_labels(spec));
    let (reason, origin, data, related) = match spec.structured.as_ref() {
        Some(structured) => (
            Some(structured.reason.as_str().to_string()),
            Some(structured.origin.clone()),
            structured.data.to_json_value(),
            structured
                .related
                .iter()
                .map(|fact| SerializableSourceFact {
                    role: fact.role.as_str().to_string(),
                    source_id: fact.source_id.0,
                    span: [fact.span.start as u32, fact.span.end as u32],
                    ty: fact.ty.clone(),
                    declaration_identity: fact.declaration_identity.clone(),
                })
                .collect(),
        ),
        None => (None, None, serde_json::Value::Null, Vec::new()),
    };
    SerializableDiagnostic {
        kind: spec.kind.clone(),
        phase,
        line,
        column,
        span: [spec.primary_span.start as u32, spec.primary_span.end as u32],
        message: spec.message.clone(),
        expected,
        got,
        hint,
        reason,
        origin,
        data,
        related,
    }
}

fn structured_expected_got(spec: &DiagnosticSpec) -> Option<(Option<String>, Option<String>)> {
    let data = spec.structured.as_ref()?.data.clone();
    Some(match data {
        DiagnosticData::ArgumentRelation(value) => (value.expected_type, value.actual_type),
        DiagnosticData::ReturnTypeArgument(value) => {
            (Some(value.expected_type), Some(value.actual_type))
        }
        DiagnosticData::TypeConstructorCarrier(value) => {
            (Some(value.expected_carrier), Some(value.actual_carrier))
        }
        DiagnosticData::BranchAssertion(value) => {
            (Some(value.expected_type), Some(value.actual_type))
        }
        _ => (None, None),
    })
}
