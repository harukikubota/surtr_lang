use crate::{
    Color, DiagnosticLabel, DiagnosticSpec, RuntimeDiagnosticContext, SourceId, SourceRegistry,
};
use ariadne::Fmt;
use spire::ast::Span;

mod labels {
    use super::*;

    include!("heuristics/labels_impl.rs");
}

mod runtime {
    use super::*;

    include!("heuristics/runtime_impl.rs");
}

mod shared {
    use super::*;

    include!("heuristics/shared_impl.rs");
}

mod type_templates {
    use super::*;

    include!("heuristics/type_templates_core.rs");
    include!("heuristics/type_templates_extra.rs");
    include!("heuristics/type_templates_tail.rs");
}

pub(crate) use labels::*;
pub(crate) use runtime::*;
pub(crate) use shared::*;
pub(crate) use type_templates::*;
