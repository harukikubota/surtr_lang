use crate::source::{line_column_for_offset, normalized_char_span};
use crate::{
    Color, DiagnosticData, DiagnosticSpec, SerializableDiagnostic, SerializableDiagnosticReport,
    SerializableSourceFact, SourceId, SourceRegistry,
};
use ariadne::{Label, Report, ReportKind};
use std::collections::HashSet;
use std::fmt;
use std::io::{self, Write};

pub fn report_error(file_name: &str, source: &str, spec: DiagnosticSpec) {
    let report = build_report(file_name, source, &spec);
    let cache = ariadne::sources([(
        RenderSourceId::Primary(file_name.to_string()),
        source.to_string(),
    )]);

    if let Err(err) = report.eprint(cache) {
        let mut stderr = io::stderr().lock();
        let _ = write_fallback_diagnostic(&mut stderr, file_name, &spec, &err);
    }
}

pub fn render_error(file_name: &str, source: &str, spec: &DiagnosticSpec) -> String {
    let report = build_report(file_name, source, spec);
    let mut buf = Vec::new();
    let cache = ariadne::sources([(
        RenderSourceId::Primary(file_name.to_string()),
        source.to_string(),
    )]);

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
    let primary_range = primary.start..primary.end;
    let suppress_primary_label = spec.structured.is_some() && !spec.labels.is_empty();
    let primary_source = RenderSourceId::Primary(file_name.to_string());
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
        let range = span.start..span.end;
        builder = builder.with_label(match label.color {
            Some(color) => Label::new((primary_source.clone(), range))
                .with_message(label.message.clone())
                .with_color(color),
            None => Label::new((primary_source.clone(), range)).with_message(label.message.clone()),
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
    Registered(SourceId, String),
}

impl fmt::Display for RenderSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderSourceId::Primary(file_name) | RenderSourceId::Registered(_, file_name) => {
                f.write_str(file_name)
            }
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
    let primary_range = primary.start..primary.end;
    let suppress_primary_label = spec.structured.is_some() && !spec.labels.is_empty();
    let primary_render_source = RenderSourceId::Registered(source_id, primary_file_name);
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

    let mut cache = vec![(primary_render_source.clone(), primary_source.to_string())];

    let mut cached_sources = HashSet::from([source_id]);
    for label in &spec.labels {
        let label_source_id = label.source_id.unwrap_or(source_id);
        let Some(label_entry) = sources.get(label_source_id) else {
            continue;
        };
        let label_span = normalized_char_span(&label_entry.source, &label.span);
        let label_range = label_span.start..label_span.end;
        let label_render_source =
            RenderSourceId::Registered(label_source_id, label_entry.file_name.clone());
        if cached_sources.insert(label_source_id) {
            cache.push((label_render_source.clone(), label_entry.source.clone()));
        }
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
    let (expected, got) = structured_expected_got(spec).unwrap_or((None, None));
    let hint = spec.help.clone();
    let (reason, origin, data, related) = match spec.structured.as_ref() {
        Some(structured) => (
            Some(structured.reason.as_str().to_string()),
            Some(structured.origin.clone()),
            structured.data_json(),
            std::iter::once(&structured.primary)
                .chain(structured.related.iter())
                .map(|fact| SerializableSourceFact {
                    role: fact.role.json_name().to_string(),
                    ordinal: fact.ordinal,
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
        DiagnosticData::ReturnTypeArgument(value) => (value.expected_type, value.actual_type),
        DiagnosticData::TypeConstructorCarrier(value) => {
            if spec.structured.as_ref()?.reason
                == crate::TypeDiagnosticReason::MissingTypeConstructorCapability
            {
                (None, None)
            } else {
                (Some(value.expected_carrier), Some(value.actual_carrier))
            }
        }
        DiagnosticData::BranchAssertion(value) => {
            (Some(value.expected_type), Some(value.actual_type))
        }
        DiagnosticData::Pattern(value) => (value.expected_type, value.actual_type),
        DiagnosticData::Policy(value) => (value.expected_type, value.actual_type),
        _ => (None, None),
    })
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::{simple_error, DiagnosticLabel};
    use spire::ast::Span;

    #[test]
    fn registry_cache_keeps_one_body_per_source_across_many_labels() {
        let mut sources = SourceRegistry::new();
        let text = "あx".repeat(4096);
        let primary = sources.register("same.srt", text.clone());
        // Distinct source identities must not collapse even with the same filename.
        let related = sources.register("same.srt", "別y");
        for label_count in [1, 10, 100] {
            let mut spec = simple_error("TypeError", "test", Span { start: 1, end: 2 }, None);
            for index in 0..label_count {
                spec.labels.push(DiagnosticLabel {
                    source_id: Some(primary),
                    span: Span { start: 1, end: 2 },
                    message: format!("primary {index}"),
                    color: None,
                });
            }
            spec.labels.push(DiagnosticLabel {
                source_id: Some(related),
                span: Span { start: 1, end: 2 },
                message: "related".into(),
                color: None,
            });
            let (report, cache) = build_report_with_registry(&sources, primary, &spec).unwrap();
            assert_eq!(cache.len(), 2, "{label_count} labels");
            assert_eq!(
                cache.iter().map(|(_, body)| body.len()).sum::<usize>(),
                text.len() + "別y".len()
            );
            let mut output = Vec::new();
            report.write(ariadne::sources(cache), &mut output).unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("primary 0"));
            assert!(output.contains("related"));
        }
    }
}
