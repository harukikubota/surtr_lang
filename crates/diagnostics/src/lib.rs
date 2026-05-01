pub use ariadne::Color;

mod debug_render;
mod heuristics;
mod parse;
mod render;
mod report;
mod resolve;
mod runtime;
mod source;
mod surtr_code;
mod typecheck;

#[cfg(test)]
mod tests;

pub use debug_render::{render_debug_report, DebugLabel};
pub use parse::parse_error_spec;
pub use render::{
    render_error, render_error_by_id, report_error, report_error_by_id,
    serializable_diagnostic_by_id, serializable_report_by_id,
};
pub use report::{
    simple_error, DiagnosticLabel, DiagnosticSpec, RuntimeDiagnosticContext,
    SerializableDiagnostic, SerializableDiagnosticReport,
};
pub use resolve::resolve_error_spec;
pub use runtime::{runtime_error_spec, runtime_error_spec_by_id, runtime_value_error_spec};
pub use source::{SourceEntry, SourceId, SourceRegistry};
pub use surtr_code::{render_surtr_code_error, surtr_assert_eq_error_spec};
pub use typecheck::{type_error_spec, type_error_spec_by_id, TypeErrorDiagnostic};
