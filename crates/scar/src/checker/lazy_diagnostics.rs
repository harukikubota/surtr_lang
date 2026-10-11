//! Function-specific advice from resolved capture provenance and checked signatures.
//! Binding provenance follows UID aliases and is saved with REPL state and candidate probes.
//! Active capture state is temporary; failed probes must restore it with the type state.
//! Unknown signatures and unselected source roles are never guessed from names or type shapes.
//! Advice augments the original reason and uses only requirements known by normalization.

use super::*;
use diagnostics::{DiagnosticData, DiagnosticOrigin, TypeDiagnosticReason};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LazyCaptureDiagnostic {
    kind: LazyCaptureKind,
    parameters: Vec<ResolvedLazyCaptureParam>,
    signature: Ty,
}

#[derive(Clone)]
pub(super) struct ActiveLazyCapture {
    parameters: Vec<ResolvedClosureParam>,
    expected: Option<Ty>,
    body_span: Span,
    normalized_signature: Option<Ty>,
}

impl ActiveLazyCapture {
    pub(super) fn new(
        parameters: &[ResolvedClosureParam],
        expected: Option<&Ty>,
        body_span: Span,
    ) -> Option<Self> {
        parameters
            .iter()
            .any(|param| param.lazy_capture.is_some())
            .then(|| Self {
                parameters: parameters.to_vec(),
                expected: expected.cloned(),
                body_span,
                normalized_signature: None,
            })
    }
}

trait LazyCaptureText {
    fn name(self) -> &'static str;
    fn explanation(self, source_ordinal: u32) -> &'static str;
}

impl LazyCaptureText for LazyCaptureKind {
    fn name(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::If => "if",
            Self::IfThen => "if_then",
            Self::IfLet => "if_let",
            Self::IfLetThen => "if_let_then",
            Self::Require => "require",
            Self::Ensure => "ensure",
            Self::MapErr => "Result::map_err",
            Self::Cause => "Result::cause",
        }
    }

    // Each function owns its explanation, including its evaluation condition.
    fn explanation(self, source_ordinal: u32) -> &'static str {
        match self {
            Self::And => "and captures its right operand as a zero-argument function; it runs only when the left operand is True.",
            Self::Or => "or captures its right operand as a zero-argument function; it runs only when the left operand is False.",
            Self::If => "if captures direct branch placeholders as normalized zero-argument functions; only the selected branch runs, and its result is returned after one call.",
            Self::IfThen => "if_then captures its branch as (-> Unit); the branch runs only when the condition is True.",
            Self::IfLet if source_ordinal == 2 => "if_let captures a binding-free success branch as a normalized zero-argument function; it runs only when the Pattern matches.",
            Self::IfLet => "if_let captures its failure branch as a normalized zero-argument function; it runs only when the Pattern does not match.",
            Self::IfLetThen => "if_let_then captures a binding-free success branch as (-> Unit); it runs only when the Pattern matches.",
            Self::Require => "require captures its error argument as (-> Error); it runs only when the condition is False.",
            Self::Ensure => "ensure captures its error argument as (-> Error); it runs only when the predicate returns False.",
            Self::MapErr => "Result::map_err captures the replacement error as (-> Error); it runs only for Err.",
            Self::Cause => "Result::cause captures the cause error as (-> Error); it runs only for Err.",
        }
    }
}

impl Checker {
    fn lazy_source_parameters(node: &Resolved) -> Option<&[ResolvedClosureParam]> {
        match node {
            Resolved::CaptureClosure(_, params, _, _)
                if params.iter().any(|param| param.lazy_capture.is_some()) =>
            {
                Some(params)
            }
            Resolved::Grouped(_, inner) => Self::lazy_source_parameters(inner),
            _ => None,
        }
    }

    fn lazy_binding_origin(&self, node: &Resolved) -> Option<LazyCaptureDiagnostic> {
        match node {
            Resolved::Var(_, id) => self.lazy_capture_bindings.get(&id.unique_id).cloned(),
            Resolved::Grouped(_, inner) => self.lazy_binding_origin(inner),
            Resolved::Capture(_, target, args) if args.is_empty() => {
                self.lazy_binding_origin(target)
            }
            _ => None,
        }
    }

    pub(super) fn bind_lazy_capture_diagnostic(
        &mut self,
        pattern: &TypedPattern,
        source: &Resolved,
        typed: &TypedNode,
    ) {
        let origin = Self::lazy_source_parameters(source)
            .map(|params| LazyCaptureDiagnostic {
                kind: params[0]
                    .lazy_capture
                    .as_ref()
                    .expect("capture metadata")
                    .kind,
                parameters: params
                    .iter()
                    .map(|param| {
                        param
                            .lazy_capture
                            .clone()
                            .expect("all generated parameters have capture metadata")
                    })
                    .collect(),
                signature: self.resolve_ty(&typed.ty),
            })
            .or_else(|| self.lazy_binding_origin(source));
        fn bind(
            pattern: &TypedPattern,
            origin: &Option<LazyCaptureDiagnostic>,
            bindings: &mut HashMap<u32, LazyCaptureDiagnostic>,
        ) {
            match pattern {
                TypedPattern::Located(_, inner) => bind(inner, origin, bindings),
                TypedPattern::Var(_, id) => {
                    if let Some(origin) = origin {
                        bindings.insert(id.unique_id, origin.clone());
                    } else {
                        bindings.remove(&id.unique_id);
                    }
                }
                TypedPattern::As(_, inner, id) => {
                    if let Some(origin) = origin {
                        bindings.insert(id.unique_id, origin.clone());
                    } else {
                        bindings.remove(&id.unique_id);
                    }
                    bind(inner, origin, bindings);
                }
                _ => {}
            }
        }
        bind(pattern, &origin, &mut self.lazy_capture_bindings);
    }

    pub(super) fn lazy_placeholder_error(
        &self,
        error: TypeError,
        required: &Ty,
        node: &TypedNode,
        requirements: &[(&TypedNode, &Ty)],
    ) -> TypeError {
        let Some(active) = &self.active_lazy_capture else {
            return error;
        };
        let TypedInner::Var(id) = &node.node else {
            return error;
        };
        let Some((index, param)) = active
            .parameters
            .iter()
            .enumerate()
            .find(|(_, param)| param.id.unique_id == id.unique_id)
        else {
            return error;
        };
        let Some(metadata) = &param.lazy_capture else {
            return error;
        };
        let Some((ordinal, _)) = metadata
            .lazy_uses
            .iter()
            .find(|(_, span)| *span == node.span)
        else {
            return error;
        };
        if metadata.ordinary {
            return error.with_hint(format!("{} uses &{} as both an ordinary value and a Lazy argument. Use different placeholder numbers for these incompatible requirements.", metadata.kind.name(), index + 1));
        }
        let mut parameters = active
            .parameters
            .iter()
            .map(|param| {
                self.env
                    .lookup_var(param.id.unique_id)
                    .map(|ty| self.resolve_ty(ty))
                    .unwrap_or(Ty::Hole)
            })
            .collect::<Vec<_>>();
        for (node, required) in requirements {
            if let TypedInner::Var(id) = &node.node {
                if let Some(index) = active
                    .parameters
                    .iter()
                    .position(|param| param.id.unique_id == id.unique_id)
                {
                    parameters[index] = self.resolve_ty(required);
                }
            }
        }
        let output = match metadata.kind {
            LazyCaptureKind::And | LazyCaptureKind::Or => Ty::Bool,
            LazyCaptureKind::IfThen | LazyCaptureKind::IfLetThen => Ty::Unit,
            LazyCaptureKind::Require => Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)),
            LazyCaptureKind::If | LazyCaptureKind::IfLet => match self.resolve_ty(required) {
                Ty::Func(_, ret) => *ret,
                _ => Ty::Hole,
            },
            _ => match active.expected.as_ref() {
                Some(Ty::Func(_, ret)) => self.resolve_ty(ret),
                _ => match self.resolve_ty(required) {
                    Ty::Func(_, ret) => *ret,
                    _ => Ty::Hole,
                },
            },
        };
        let signature = Ty::Func(parameters, Box::new(output));
        let mut unresolved = Vec::new();
        Self::collect_ty_vars(&signature, &mut unresolved);
        let complete = unresolved
            .iter()
            .all(|variable| self.rigid_tyvars.contains(variable))
            && !matches!(
                metadata.kind,
                LazyCaptureKind::Ensure | LazyCaptureKind::MapErr | LazyCaptureKind::Cause
            );
        let signature = if complete {
            format!(
                " Generated signature: {}.",
                self.diagnostic_ty_name(&signature)
            )
        } else {
            String::new()
        };
        error.with_hint(format!("{} Lazy capture parameter {} requires {}. {}{} Use the normalized parameter type in the capture annotation.", metadata.kind.name(), index + 1, self.diagnostic_ty_name(required), metadata.kind.explanation(*ordinal), signature))
    }

    pub(super) fn lazy_unknown_binding_error(
        &self,
        error: TypeError,
        source: &Resolved,
    ) -> TypeError {
        let Some(params) = Self::lazy_source_parameters(source) else {
            return error;
        };
        let metadata = params[0].lazy_capture.as_ref().expect("capture metadata");
        if !params.iter().any(|param| {
            param
                .lazy_capture
                .as_ref()
                .is_some_and(|metadata| !metadata.lazy_uses.is_empty())
        }) {
            return error;
        }
        error.with_hint(format!("{} Lazy capture needs a concrete callable annotation to determine the normalized branch types. For example, annotate the generated parameters with the required zero-argument function types and specify the return type.", metadata.kind.name()))
    }

    pub(super) fn record_lazy_capture_signature(&mut self, span: &Span, output: &Ty) {
        let Some(active) = &self.active_lazy_capture else {
            return;
        };
        if active.body_span != *span {
            return;
        }
        let parameters = active
            .parameters
            .iter()
            .map(|param| {
                self.env
                    .lookup_var(param.id.unique_id)
                    .map(|ty| self.resolve_ty(ty))
                    .unwrap_or(Ty::Hole)
            })
            .collect();
        let signature = Ty::Func(parameters, Box::new(self.resolve_ty(output)));
        self.active_lazy_capture
            .as_mut()
            .expect("active capture")
            .normalized_signature = Some(signature);
    }

    pub(super) fn lazy_capture_annotation_error(&self, error: TypeError) -> TypeError {
        let Some(active) = &self.active_lazy_capture else {
            return error;
        };
        if active.expected.is_none() {
            return error;
        }
        let Some(signature) = &active.normalized_signature else {
            return error;
        };
        let Some(diagnostic) = &error.structured else {
            return error;
        };
        let DiagnosticData::BranchAssertion(relation) = &diagnostic.data else {
            return error;
        };
        if relation
            .left_origin
            .as_ref()
            .is_none_or(|fact| fact.span != active.body_span)
        {
            return error;
        }
        let kind = active.parameters[0]
            .lazy_capture
            .as_ref()
            .expect("capture metadata")
            .kind;
        error.with_hint(format!("{} Lazy capture has generated signature {}. The annotation must match the actual normalized branch result; it cannot change which closure shell is consumed.", kind.name(), self.diagnostic_ty_name(signature)))
    }

    pub(super) fn lazy_call_error(
        &self,
        error: TypeError,
        call_span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> TypeError {
        let Some(origin) = self.lazy_binding_origin(func) else {
            return error;
        };
        let Some(diagnostic) = &error.structured else {
            return error;
        };
        if diagnostic.reason.type_reason() != Some(TypeDiagnosticReason::ArgumentTypeMismatch)
            || diagnostic.origin != DiagnosticOrigin::Call
        {
            return error;
        }
        let DiagnosticData::ArgumentRelation(relation) = &diagnostic.data else {
            return error;
        };
        // Preserve nested-call failures: this assertion must belong to this
        // application and its complete argument, not a descendant expression.
        if relation.callable != "function" {
            return error;
        }
        let index = relation.ordinal as usize;
        let Some(ResolvedRecordLitArg::Positional(arg)) = args.get(index) else {
            return error;
        };
        if relation
            .expected_origin
            .as_ref()
            .is_none_or(|fact| fact.span != *call_span)
        {
            return error;
        }
        let _ = arg;
        let Some(parameter) = origin.parameters.get(index) else {
            return error;
        };
        let Some((ordinal, _)) = parameter.lazy_uses.first() else {
            return error;
        };
        let signature = self.resolve_ty(&origin.signature);
        let Ty::Func(parameters, _) = &signature else {
            return error;
        };
        let expected = &parameters[index];
        error.with_hint(format!("{} Lazy capture parameter {} requires {}. {} Generated signature: {}. Pass a zero-argument function, for example {{ || value }}, with the required return type.", origin.kind.name(), index + 1, self.diagnostic_ty_name(expected), origin.kind.explanation(*ordinal), self.diagnostic_ty_name(&signature)))
    }
}
