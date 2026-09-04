pub use ariadne::Color;

mod data;
mod debug_render;
mod heuristics;
mod parse;
mod render;
mod repl;
mod report;
mod resolve;
mod runtime;
mod source;
mod surtr_code;
mod typecheck;

#[cfg(test)]
mod tests;

pub use data::{
    ArgumentRelationData, BranchAssertionData, ConstraintSubjectData, DeclarationIdentity,
    DiagnosticData, DiagnosticOrigin, Remediation, ReturnTypeArgumentData, RuntimeData,
    SafeBindRelationData, SourceFact, SourceRole, StructuredDiagnostic, TraitDispatchData,
    TraitObligationData, TypeConstructorCarrierData, TypeDiagnosticReason,
};
pub use debug_render::{render_debug_report, DebugLabel};
pub use parse::parse_error_spec;
pub use render::{
    render_error, render_error_by_id, report_error, report_error_by_id,
    serializable_diagnostic_by_id, serializable_report_by_id,
};
pub use repl::{repl_command_parse_error_spec, repl_query_parse_error_spec};
pub use report::{
    simple_error, DiagnosticLabel, DiagnosticSpec, RuntimeDiagnosticContext,
    SerializableDiagnostic, SerializableDiagnosticReport, SerializableSourceFact,
};
pub use resolve::{
    resolve_error_spec, resolve_error_spec_with_labels, resolve_related_label_color,
};
pub use runtime::{runtime_error_spec, runtime_error_spec_by_id, runtime_value_error_spec};
pub use source::{SourceEntry, SourceId, SourceRegistry};
pub use surtr_code::{render_surtr_code_error, surtr_assert_eq_error_spec};
pub use typecheck::{
    structured_type_error_spec, type_error_spec, type_error_spec_by_id,
    type_error_spec_from_structured, TypeErrorDiagnostic,
};
