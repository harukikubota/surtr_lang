use ariadne::{Color, Label, Report, ReportKind, Source};
use sindr::runtime::Location;

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
}

pub fn format_runtime_error(err: &RuntimeError) -> String {
    format!("RuntimeError: {}", err.message)
}

pub fn format_runtime_error_with_location(
    err: &RuntimeError,
    location: Option<&Location>,
) -> String {
    match location {
        Some(location) => format!(
            "{} ({}:{}:{})",
            format_runtime_error(err),
            location.file,
            location.line,
            location.column
        ),
        None => format_runtime_error(err),
    }
}

pub fn report_runtime_error(
    err: &RuntimeError,
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<Location>,
) {
    if let (Some(source), Some(location)) = (source, location.clone()) {
        let file_name = if location.file.is_empty() {
            fallback_file.unwrap_or("<runtime>")
        } else {
            location.file.as_str()
        };
        let start = location.span_start as usize;
        let end = if location.span_end > location.span_start {
            location.span_end as usize
        } else {
            start.saturating_add(1)
        };

        let report = Report::build(ReportKind::Error, (file_name, start..end))
            .with_message("RuntimeError")
            .with_label(
                Label::new((file_name, start..end))
                    .with_message(&err.message)
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((file_name, Source::from(source)));

        if report.is_ok() {
            return;
        }
    }

    eprintln!(
        "{}",
        format_runtime_error_with_location(err, location.as_ref())
    );
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_runtime_error(self))
    }
}

impl std::error::Error for RuntimeError {}

#[cfg(test)]
mod tests {
    use super::{format_runtime_error, format_runtime_error_with_location, RuntimeError};
    use sindr::runtime::Location;

    #[test]
    fn format_runtime_error_uses_shared_shape() {
        let err = RuntimeError {
            message: "boom".into(),
        };
        assert_eq!(format_runtime_error(&err), "RuntimeError: boom");
    }

    #[test]
    fn format_runtime_error_with_location_appends_file_line_column() {
        let err = RuntimeError {
            message: "boom".into(),
        };
        let location = Location {
            file: "main.srt".into(),
            func: "<runtime>".into(),
            line: 3,
            column: 7,
            span_start: 10,
            span_end: 14,
        };
        assert_eq!(
            format_runtime_error_with_location(&err, Some(&location)),
            "RuntimeError: boom (main.srt:3:7)"
        );
    }
}
